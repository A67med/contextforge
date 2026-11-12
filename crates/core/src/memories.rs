//! Memory records: creation with de-duplication, updates with revision history, supersession,
//! revalidation, pinning and deletion — all with provenance and permission checks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::model::*;
use crate::privacy::{self, SensitiveMatcher};
use crate::projects::Project;
use crate::search::{MatchMode, fts_expression};
use crate::{Error, Result, Store, git, paths, util};

pub(crate) const MEMORY_COLUMNS_M: &str = "m.id, m.project_id, m.kind, m.status, m.certainty, m.freshness, m.title, m.content, \
     m.tags_json, m.pinned, m.user_locked, m.branch, m.origin_branch, m.source_agent, m.source_session_id, m.source_origin, \
     m.created_at, m.updated_at, m.last_validated_at, m.validated_commit, m.superseded_by, m.stale_reason, m.injection_flag, \
     m.revision, m.reinforced_count, m.metadata_json";

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_CONTENT_CHARS: usize = 20_000;
const MAX_TAGS: usize = 20;
const MAX_FILES: usize = 50;
const MAX_COMMITS: usize = 20;
const MAX_EVIDENCE: usize = 20;
const MAX_FILE_BYTES_TO_HASH: u64 = 20 * 1024 * 1024;
/// Combined title/content similarity at or above which a new memory is treated as a duplicate.
const DUPLICATE_THRESHOLD: f64 = 0.85;
/// Similarity at or above which an existing memory is reported as related.
const SIMILAR_THRESHOLD: f64 = 0.5;

pub(crate) fn row_to_memory(r: &Row<'_>) -> rusqlite::Result<Memory> {
    let tags_json: String = r.get("tags_json")?;
    let metadata_json: String = r.get("metadata_json")?;
    Ok(Memory {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        kind: r.get("kind")?,
        status: r.get("status")?,
        certainty: r.get("certainty")?,
        freshness: r.get("freshness")?,
        title: r.get("title")?,
        content: r.get("content")?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        pinned: r.get::<_, i64>("pinned")? != 0,
        user_locked: r.get::<_, i64>("user_locked")? != 0,
        branch: r.get("branch")?,
        origin_branch: r.get("origin_branch")?,
        source_agent: r.get("source_agent")?,
        source_session_id: r.get("source_session_id")?,
        source_origin: r.get("source_origin")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        last_validated_at: r.get("last_validated_at")?,
        validated_commit: r.get("validated_commit")?,
        superseded_by: r.get("superseded_by")?,
        stale_reason: r.get("stale_reason")?,
        injection_flag: r.get::<_, i64>("injection_flag")? != 0,
        revision: r.get("revision")?,
        reinforced_count: r.get("reinforced_count")?,
        metadata: serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Object(Default::default())),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FileRefInput {
    pub path: String,
    #[serde(default)]
    pub line_start: Option<i64>,
    #[serde(default)]
    pub line_end: Option<i64>,
}

impl FileRefInput {
    pub fn path(p: impl Into<String>) -> Self {
        FileRefInput { path: p.into(), line_start: None, line_end: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceInput {
    pub kind: EvidenceKind,
    pub reference: String,
    #[serde(default)]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub project_id: String,
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub status: Option<MemoryStatus>,
    pub certainty: Option<Certainty>,
    pub tags: Vec<String>,
    pub files: Vec<FileRefInput>,
    pub commits: Vec<String>,
    pub evidence: Vec<EvidenceInput>,
    pub branch: Option<String>,
    pub pinned: bool,
    pub links: Vec<(String, LinkRelation)>,
    pub metadata: Option<serde_json::Value>,
    /// Directory used to resolve relative file references (usually the agent's cwd).
    pub base_dir: Option<PathBuf>,
}

impl NewMemory {
    pub fn new(project_id: impl Into<String>, kind: MemoryKind, title: impl Into<String>, content: impl Into<String>) -> Self {
        NewMemory {
            project_id: project_id.into(),
            kind,
            title: title.into(),
            content: content.into(),
            status: None,
            certainty: None,
            tags: Vec::new(),
            files: Vec::new(),
            commits: Vec::new(),
            evidence: Vec::new(),
            branch: None,
            pinned: false,
            links: Vec::new(),
            metadata: None,
            base_dir: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveAction {
    Created,
    /// An equivalent memory already existed; it was reinforced instead of duplicated.
    Reinforced,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimilarMemory {
    pub id: String,
    pub title: String,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveOutcome {
    pub memory_id: String,
    pub action: SaveAction,
    pub certainty: Certainty,
    pub status: MemoryStatus,
    /// Human-readable notes about adjustments (redactions, downgrades, skipped references).
    pub notes: Vec<String>,
    pub similar: Vec<SimilarMemory>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryPatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub status: Option<MemoryStatus>,
    pub certainty: Option<Certainty>,
    pub kind: Option<MemoryKind>,
    pub tags: Option<Vec<String>>,
    pub pinned: Option<bool>,
    /// `Some(None)` clears the branch scope.
    pub branch: Option<Option<String>>,
    pub metadata: Option<serde_json::Value>,
    pub add_files: Vec<FileRefInput>,
    pub remove_files: Vec<String>,
    pub add_commits: Vec<String>,
    pub add_evidence: Vec<EvidenceInput>,
    pub reason: Option<String>,
    #[serde(skip)]
    pub base_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedFile {
    pub path: String,
    pub hash: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub sensitive: bool,
}

#[derive(Debug, Clone)]
struct PreparedEvidence {
    kind: EvidenceKind,
    reference: String,
    excerpt: Option<String>,
    counts: bool,
}

/// Hash a file for snapshot purposes (None if missing, unreadable or too large).
pub(crate) fn hash_file(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES_TO_HASH {
        return None;
    }
    std::fs::read(path).ok().map(|b| util::content_hash_normalized(&b))
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().to_lowercase().replace(' ', "-");
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > 40 || !t.chars().all(|c| c.is_alphanumeric() || "-_.:/#".contains(c)) {
            return Err(Error::invalid(format!("invalid tag '{t}' (max 40 chars; letters, digits, - _ . : / #)")));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(Error::invalid(format!("at most {MAX_TAGS} tags are allowed")));
    }
    Ok(out)
}

fn normalize_commit(sha: &str) -> Result<String> {
    let s = sha.trim().to_lowercase();
    if !(7..=64).contains(&s.len()) || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::invalid(format!("'{sha}' is not a commit hash (7–64 hex characters)")));
    }
    Ok(s)
}

pub(crate) fn content_hash(kind: MemoryKind, title: &str, content: &str) -> String {
    let norm =
        format!("{}\u{1f}{}\u{1f}{}", kind.as_str(), util::collapse_ws(&title.to_lowercase()), util::collapse_ws(&content.to_lowercase()));
    util::sha256_hex(norm.as_bytes())
}

fn similarity(a_title: &str, a_content: &str, b_title: &str, b_content: &str) -> f64 {
    let ta = util::word_tokens(a_title);
    let tb = util::word_tokens(b_title);
    let all_a = util::word_tokens(&format!("{a_title} {a_content}"));
    let all_b = util::word_tokens(&format!("{b_title} {b_content}"));
    0.6 * util::jaccard(&ta, &tb) + 0.4 * util::jaccard(&all_a, &all_b)
}

fn validate_text(title: &str, content: &str) -> Result<()> {
    let tc = title.trim().chars().count();
    if tc == 0 {
        return Err(Error::invalid("title cannot be empty"));
    }
    if tc > MAX_TITLE_CHARS {
        return Err(Error::invalid(format!("title must be at most {MAX_TITLE_CHARS} characters")));
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(Error::invalid(format!("content must be at most {MAX_CONTENT_CHARS} characters")));
    }
    Ok(())
}

fn check_status(kind: MemoryKind, status: MemoryStatus) -> Result<()> {
    if !kind.allowed_statuses().contains(&status) {
        let allowed: Vec<&str> = kind.allowed_statuses().iter().map(|s| s.as_str()).collect();
        return Err(Error::invalid(format!("status '{status}' is not valid for a {kind}; use one of: {}", allowed.join(", "))));
    }
    Ok(())
}

/// Snapshot context gathered before a write transaction (file hashes, git state).
struct Snapshot {
    head: Option<String>,
    branch: Option<String>,
    checkout_root: PathBuf,
}

impl Store {
    fn snapshot_for(&self, project: &Project, base_dir: Option<&Path>) -> Snapshot {
        let checkout_root = base_dir
            .and_then(|b| project.all_roots().into_iter().find(|r| paths::relative_to_root(r, &paths::canonical(b)).is_some()))
            .unwrap_or_else(|| project.root());
        let (head, branch) = if project.is_git { git::head_info(&checkout_root) } else { (None, None) };
        Snapshot { head, branch, checkout_root }
    }

    fn matcher_for(&self, project: &Project) -> Result<SensitiveMatcher> {
        self.sensitive_matcher(&project.settings.excluded_globs)
    }

    pub(crate) fn prepare_files(
        &self,
        project: &Project,
        snap_root: &Path,
        base: Option<&Path>,
        inputs: &[FileRefInput],
        matcher: &SensitiveMatcher,
        notes: &mut Vec<String>,
    ) -> Vec<PreparedFile> {
        let extra: Vec<PathBuf> = project.all_roots().into_iter().skip(1).collect();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for input in inputs.iter().take(MAX_FILES) {
            let rel = match paths::normalize_file_ref(&project.root(), &extra, base, &input.path) {
                Ok(r) => r,
                Err(e) => {
                    notes.push(format!("skipped file reference: {e}"));
                    continue;
                }
            };
            if !seen.insert(rel.to_lowercase()) {
                continue;
            }
            let (mut ls, mut le) = (input.line_start, input.line_end);
            if let Some(s) = ls
                && s < 1
            {
                ls = None;
            }
            if let (Some(s), Some(e)) = (ls, le)
                && e < s
            {
                le = None;
            }
            let sensitive = matcher.is_sensitive(&rel);
            let hash = if sensitive { None } else { hash_file(&paths::join_rel(snap_root, &rel)) };
            if sensitive {
                notes.push(format!("'{rel}' matches a sensitive-file pattern: only the path is stored"));
            }
            out.push(PreparedFile { path: rel, hash, line_start: ls, line_end: le, sensitive });
        }
        if inputs.len() > MAX_FILES {
            notes.push(format!("only the first {MAX_FILES} file references were kept"));
        }
        out
    }

    fn prepare_evidence(
        &self,
        project: &Project,
        snap: &Snapshot,
        inputs: &[EvidenceInput],
        matcher: &SensitiveMatcher,
        notes: &mut Vec<String>,
    ) -> Vec<PreparedEvidence> {
        let mut out = Vec::new();
        let commit_refs: Vec<String> =
            inputs.iter().filter(|e| e.kind == EvidenceKind::Commit).filter_map(|e| normalize_commit(&e.reference).ok()).collect();
        let missing = if project.is_git && !commit_refs.is_empty() {
            git::missing_commits(&snap.checkout_root, &commit_refs).unwrap_or_default()
        } else {
            HashSet::new()
        };
        for e in inputs.iter().take(MAX_EVIDENCE) {
            let reference = privacy::redact_str(&util::truncate_chars(e.reference.trim(), 500));
            if reference.is_empty() {
                notes.push("skipped evidence with an empty reference".into());
                continue;
            }
            let mut excerpt =
                e.excerpt.as_deref().map(|x| privacy::redact_str(&util::truncate_chars(x.trim(), 2000))).filter(|x| !x.is_empty());
            let counts = match e.kind {
                EvidenceKind::File => {
                    let extra: Vec<PathBuf> = project.all_roots().into_iter().skip(1).collect();
                    match paths::normalize_file_ref(&project.root(), &extra, None, &reference) {
                        Ok(rel) => {
                            if matcher.is_sensitive(&rel) {
                                excerpt = None;
                            }
                            let exists = paths::join_rel(&snap.checkout_root, &rel).is_file();
                            if !exists {
                                notes.push(format!("evidence file '{rel}' does not exist; it does not count as verification"));
                            }
                            exists
                        }
                        Err(err) => {
                            notes.push(format!("evidence file rejected: {err}"));
                            false
                        }
                    }
                }
                EvidenceKind::Commit => match normalize_commit(&reference) {
                    Ok(sha) if !project.is_git => {
                        notes.push(format!("commit {sha} cannot be checked (project is not a Git repository)"));
                        false
                    }
                    Ok(sha) if missing.contains(&sha) => {
                        notes.push(format!("commit {sha} was not found in the repository; it does not count as verification"));
                        false
                    }
                    Ok(_) => true,
                    Err(err) => {
                        notes.push(err.to_string());
                        false
                    }
                },
                EvidenceKind::AgentCheck => {
                    let ok = excerpt.as_deref().is_some_and(|x| x.chars().count() >= 10);
                    if !ok {
                        notes.push("agent_check evidence needs an excerpt describing how it was checked".into());
                    }
                    ok
                }
                _ => true,
            };
            out.push(PreparedEvidence { kind: e.kind, reference, excerpt, counts });
        }
        out
    }

    fn prepare_commits(&self, project: &Project, snap: &Snapshot, commits: &[String], notes: &mut Vec<String>) -> Vec<(String, bool)> {
        let mut list: Vec<String> = Vec::new();
        for c in commits.iter().take(MAX_COMMITS) {
            match normalize_commit(c) {
                Ok(s) if !list.contains(&s) => list.push(s),
                Ok(_) => {}
                Err(e) => notes.push(format!("skipped commit reference: {e}")),
            }
        }
        let missing = if project.is_git { git::missing_commits(&snap.checkout_root, &list).unwrap_or_default() } else { HashSet::new() };
        list.into_iter()
            .map(|s| {
                let exists = project.is_git && !missing.contains(&s);
                if project.is_git && !exists {
                    notes.push(format!("commit {s} was not found in the repository"));
                }
                (s, exists)
            })
            .collect()
    }

    /// Create a memory, or reinforce an equivalent existing one instead of duplicating it.
    pub fn save_memory(&mut self, new: NewMemory, actor: &Actor) -> Result<SaveOutcome> {
        self.save_memory_inner(new, actor, None)
    }

    fn save_memory_inner(&mut self, new: NewMemory, actor: &Actor, skip_dedupe_against: Option<&str>) -> Result<SaveOutcome> {
        let project = self.get_project(&new.project_id)?;
        validate_text(&new.title, &new.content)?;
        let mut notes = Vec::new();

        let (title, n1) = privacy::redact(new.title.trim());
        let (content, n2) = privacy::redact(new.content.trim());
        if n1 + n2 > 0 {
            notes.push(format!("redacted {} secret-like value(s)", n1 + n2));
        }
        let status = new.status.unwrap_or(new.kind.default_status());
        check_status(new.kind, status)?;
        let tags = normalize_tags(&new.tags)?;
        if new.pinned && !actor.is_user() {
            return Err(Error::Forbidden("only the user can pin memories".into()));
        }
        if let Some(meta) = &new.metadata
            && !meta.is_object()
        {
            return Err(Error::invalid("metadata must be a JSON object"));
        }

        let matcher = self.matcher_for(&project)?;
        let snap = self.snapshot_for(&project, new.base_dir.as_deref());
        let files = self.prepare_files(&project, &snap.checkout_root, new.base_dir.as_deref(), &new.files, &matcher, &mut notes);
        let commits = self.prepare_commits(&project, &snap, &new.commits, &mut notes);
        let mut evidence = self.prepare_evidence(&project, &snap, &new.evidence, &matcher, &mut notes);

        let mut certainty = new.certainty.unwrap_or(new.kind.default_certainty());
        if certainty == Certainty::Verified {
            let backed = evidence.iter().any(|e| e.counts) || commits.iter().any(|(_, exists)| *exists);
            if !backed {
                if actor.is_user() {
                    evidence.push(PreparedEvidence {
                        kind: EvidenceKind::UserNote,
                        reference: "Marked verified by the user".into(),
                        excerpt: None,
                        counts: true,
                    });
                } else {
                    certainty = Certainty::Unverified;
                    notes.push(
                        "certainty downgraded to 'unverified': 'verified' requires at least one valid evidence item or existing commit"
                            .into(),
                    );
                }
            }
        }

        let branch = new.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(str::to_string);
        let hash = content_hash(new.kind, &title, &content);
        let injection = privacy::injection_risk(&title) || privacy::injection_risk(&content);
        if injection {
            notes.push("content contains instruction-like text; it will be withheld from automatic context until reviewed".into());
        }

        // --- de-duplication ---
        let (duplicate, similar) = self.find_duplicates(&project.id, new.kind, &title, &content, &hash, skip_dedupe_against)?;
        if let Some(existing_id) = duplicate {
            let outcome = self.reinforce(&existing_id, new.status, certainty, &files, &commits, &evidence, &tags, actor, notes)?;
            return Ok(outcome);
        }

        let id = util::new_id("mem");
        let now = util::now_ts();
        let metadata = new.metadata.unwrap_or_else(|| serde_json::json!({}));
        let user_locked = actor.is_user();
        let links = new.links.clone();
        let project_id = project.id.clone();
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO memories(id, project_id, kind, status, certainty, freshness, title, content, tags_json, pinned,
                    user_locked, branch, origin_branch, source_agent, source_session_id, source_origin, created_at, updated_at,
                    last_validated_at, validated_commit, injection_flag, content_hash, revision, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'current', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16, ?16, ?17, ?18, ?19, 1, ?20)",
                params![
                    id,
                    project_id,
                    new.kind,
                    status,
                    certainty,
                    title,
                    content,
                    serde_json::to_string(&tags)?,
                    new.pinned as i64,
                    user_locked as i64,
                    branch,
                    snap.branch,
                    actor.agent,
                    actor.session_id,
                    actor.origin,
                    now,
                    snap.head,
                    injection as i64,
                    hash,
                    serde_json::to_string(&metadata)?,
                ],
            )?;
            insert_files(tx, &id, &files)?;
            for (sha, _) in &commits {
                tx.execute("INSERT OR IGNORE INTO memory_commits(memory_id, sha) VALUES (?1, ?2)", params![id, sha])?;
            }
            insert_evidence(tx, &id, &evidence, actor, &now)?;
            for (to, rel) in &links {
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1 AND project_id = ?2)",
                    params![to, project_id],
                    |r| r.get(0),
                )?;
                if exists {
                    tx.execute(
                        "INSERT OR IGNORE INTO memory_links(from_id, to_id, relation, created_at) VALUES (?1, ?2, ?3, ?4)",
                        params![id, to, rel, now],
                    )?;
                }
            }
            tx.execute(
                "INSERT INTO memory_revisions(memory_id, revision, title, content, status, certainty, changed_by, changed_at, reason)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, 'created')",
                params![id, title, content, status, certainty, actor.agent.as_str(), now],
            )?;
            crate::sessions::insert_event(
                tx,
                &project_id,
                actor.session_id.as_deref(),
                actor.agent,
                EventKind::MemorySaved,
                &now,
                &format!("{} saved: {}", new.kind.label(), util::truncate_chars(&title, 120)),
                &serde_json::json!({ "memory_id": id, "kind": new.kind, "certainty": certainty }),
            )?;
            Store::touch_project(tx, &project_id, &now)?;
            Ok(())
        })?;
        Ok(SaveOutcome { memory_id: id, action: SaveAction::Created, certainty, status, notes, similar })
    }

    fn find_duplicates(
        &self,
        project_id: &str,
        kind: MemoryKind,
        title: &str,
        content: &str,
        hash: &str,
        skip: Option<&str>,
    ) -> Result<(Option<String>, Vec<SimilarMemory>)> {
        let exact: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM memories WHERE project_id = ?1 AND content_hash = ?2 AND freshness != 'superseded'
                   AND id != COALESCE(?3, '') ORDER BY created_at LIMIT 1",
                params![project_id, hash, skip],
                |r| r.get(0),
            )
            .optional()?;
        if exact.is_some() {
            return Ok((exact, Vec::new()));
        }
        let Some(expr) = fts_expression(title, MatchMode::Any) else {
            return Ok((None, Vec::new()));
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT m.id, m.title, m.content FROM memories m JOIN memories_fts ON memories_fts.rowid = m.seq
             WHERE memories_fts MATCH ?1 AND m.project_id = ?2 AND m.kind = ?3 AND m.freshness != 'superseded'
               AND m.id != COALESCE(?4, '')
             ORDER BY bm25(memories_fts, 4.0, 1.0, 2.0) LIMIT 10",
        )?;
        let candidates: Vec<(String, String, String)> = stmt
            .query_map(params![expr, project_id, kind, skip], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut best: Option<(String, f64)> = None;
        let mut similar = Vec::new();
        for (id, t, c) in candidates {
            let s = similarity(title, content, &t, &c);
            if s >= DUPLICATE_THRESHOLD && best.as_ref().is_none_or(|(_, bs)| s > *bs) {
                best = Some((id.clone(), s));
            }
            if s >= SIMILAR_THRESHOLD {
                similar.push(SimilarMemory { id, title: t, similarity: (s * 100.0).round() / 100.0 });
            }
        }
        similar.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
        similar.truncate(5);
        Ok((best.map(|(id, _)| id), similar))
    }

    #[allow(clippy::too_many_arguments)]
    fn reinforce(
        &mut self,
        id: &str,
        requested_status: Option<MemoryStatus>,
        certainty: Certainty,
        files: &[PreparedFile],
        commits: &[(String, bool)],
        evidence: &[PreparedEvidence],
        tags: &[String],
        actor: &Actor,
        mut notes: Vec<String>,
    ) -> Result<SaveOutcome> {
        let existing = self.get_memory(id)?;
        let now = util::now_ts();
        let mut new_status = existing.status;
        if let Some(s) = requested_status
            && s != existing.status
            && existing.kind.allowed_statuses().contains(&s)
        {
            new_status = s;
            notes.push(format!("status updated from '{}' to '{}'", existing.status, s));
        }
        // Certainty only moves upward through reinforcement (hypothesis/unverified → verified).
        let new_certainty = if certainty == Certainty::Verified && existing.certainty != Certainty::Verified {
            notes.push("certainty upgraded to 'verified' with new evidence".into());
            Certainty::Verified
        } else {
            existing.certainty
        };
        let mut merged_tags = existing.tags.clone();
        for t in tags {
            if !merged_tags.contains(t) && merged_tags.len() < MAX_TAGS {
                merged_tags.push(t.clone());
            }
        }
        let changed = new_status != existing.status || new_certainty != existing.certainty;
        notes.push(format!("an equivalent memory already existed ({id}); it was reinforced instead of duplicated"));
        self.write_tx(|tx| {
            tx.execute(
                "UPDATE memories SET reinforced_count = reinforced_count + 1, updated_at = ?2, status = ?3, certainty = ?4,
                        tags_json = ?5, revision = revision + ?6
                 WHERE id = ?1",
                params![id, now, new_status, new_certainty, serde_json::to_string(&merged_tags)?, changed as i64],
            )?;
            for f in files {
                tx.execute(
                    "INSERT OR IGNORE INTO memory_files(memory_id, path, content_hash, line_start, line_end, sensitive)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, f.path, f.hash, f.line_start, f.line_end, f.sensitive as i64],
                )?;
            }
            for (sha, _) in commits {
                tx.execute("INSERT OR IGNORE INTO memory_commits(memory_id, sha) VALUES (?1, ?2)", params![id, sha])?;
            }
            insert_evidence(tx, id, evidence, actor, &now)?;
            if changed {
                tx.execute(
                    "INSERT INTO memory_revisions(memory_id, revision, title, content, status, certainty, changed_by, changed_at, reason)
                     SELECT id, revision, title, content, status, certainty, ?2, ?3, 'reinforced by duplicate save' FROM memories WHERE id = ?1",
                    params![id, actor.agent.as_str(), now],
                )?;
            }
            crate::sessions::insert_event(
                tx,
                &existing.project_id,
                actor.session_id.as_deref(),
                actor.agent,
                EventKind::MemoryUpdated,
                &now,
                &format!("Reinforced: {}", util::truncate_chars(&existing.title, 120)),
                &serde_json::json!({ "memory_id": id, "reinforced": true }),
            )?;
            Store::touch_project(tx, &existing.project_id, &now)?;
            Ok(())
        })?;
        Ok(SaveOutcome {
            memory_id: id.to_string(),
            action: SaveAction::Reinforced,
            certainty: new_certainty,
            status: new_status,
            notes,
            similar: Vec::new(),
        })
    }

    pub fn get_memory(&self, id: &str) -> Result<Memory> {
        self.conn
            .query_row(&format!("SELECT {MEMORY_COLUMNS_M} FROM memories m WHERE m.id = ?1"), [id.trim()], row_to_memory)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("memory '{id}'")))
    }

    pub fn get_memory_detail(&self, id: &str) -> Result<MemoryDetail> {
        let memory = self.get_memory(id)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, content_hash, line_start, line_end, sensitive FROM memory_files WHERE memory_id = ?1 ORDER BY path",
        )?;
        let files = stmt
            .query_map([id], |r| {
                Ok(MemoryFile {
                    path: r.get(0)?,
                    content_hash: r.get(1)?,
                    line_start: r.get(2)?,
                    line_end: r.get(3)?,
                    sensitive: r.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare_cached("SELECT sha FROM memory_commits WHERE memory_id = ?1 ORDER BY sha")?;
        let commits = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, reference, excerpt, agent, session_id, captured_at FROM memory_evidence WHERE memory_id = ?1 ORDER BY id",
        )?;
        let evidence = stmt
            .query_map([id], |r| {
                Ok(Evidence {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    reference: r.get(2)?,
                    excerpt: r.get(3)?,
                    agent: r.get(4)?,
                    session_id: r.get(5)?,
                    captured_at: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT l.from_id, l.to_id, l.relation, l.created_at,
                    (SELECT title FROM memories o WHERE o.id = CASE WHEN l.from_id = ?1 THEN l.to_id ELSE l.from_id END)
             FROM memory_links l WHERE l.from_id = ?1 OR l.to_id = ?1 ORDER BY l.created_at",
        )?;
        let links = stmt
            .query_map([id], |r| {
                Ok(MemoryLink { from_id: r.get(0)?, to_id: r.get(1)?, relation: r.get(2)?, created_at: r.get(3)?, other_title: r.get(4)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT revision, title, content, status, certainty, changed_by, changed_at, reason
             FROM memory_revisions WHERE memory_id = ?1 ORDER BY revision DESC",
        )?;
        let revisions = stmt
            .query_map([id], |r| {
                Ok(Revision {
                    revision: r.get(0)?,
                    title: r.get(1)?,
                    content: r.get(2)?,
                    status: r.get(3)?,
                    certainty: r.get(4)?,
                    changed_by: r.get(5)?,
                    changed_at: r.get(6)?,
                    reason: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let findings = self.findings_for(id, true)?;
        let source_session = match &memory.source_session_id {
            Some(sid) => self.get_session(sid).ok(),
            None => None,
        };
        Ok(MemoryDetail { memory, files, commits, evidence, links, revisions, findings, source_session })
    }

    pub(crate) fn findings_for(&self, memory_id: &str, include_resolved: bool) -> Result<Vec<Finding>> {
        let sql = if include_resolved {
            "SELECT id, memory_id, detected_at, kind, path, detail, head_commit, branch, resolved_at, resolution
             FROM freshness_findings WHERE memory_id = ?1 ORDER BY id DESC LIMIT 50"
        } else {
            "SELECT id, memory_id, detected_at, kind, path, detail, head_commit, branch, resolved_at, resolution
             FROM freshness_findings WHERE memory_id = ?1 AND resolved_at IS NULL ORDER BY id DESC"
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt
            .query_map([memory_id], |r| {
                Ok(Finding {
                    id: r.get(0)?,
                    memory_id: r.get(1)?,
                    detected_at: r.get(2)?,
                    kind: r.get(3)?,
                    path: r.get(4)?,
                    detail: r.get(5)?,
                    head_commit: r.get(6)?,
                    branch: r.get(7)?,
                    resolved_at: r.get(8)?,
                    resolution: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Apply a patch. Agents may change status and add evidence/files on user-owned memories, but only the
    /// user may rewrite their text, change certainty arbitrarily, pin, or re-scope them.
    pub fn update_memory(&mut self, id: &str, patch: MemoryPatch, actor: &Actor) -> Result<SaveOutcome> {
        let existing = self.get_memory(id)?;
        let project = self.get_project(&existing.project_id)?;
        let mut notes = Vec::new();
        if existing.freshness == Freshness::Superseded && (patch.title.is_some() || patch.content.is_some()) && !actor.is_user() {
            return Err(Error::Conflict(format!(
                "memory {id} is superseded by {}; update that one instead",
                existing.superseded_by.as_deref().unwrap_or("another memory")
            )));
        }
        let rewrites_text = patch.title.is_some() || patch.content.is_some() || patch.kind.is_some();
        if !actor.is_user() {
            if existing.user_locked && rewrites_text {
                return Err(Error::Forbidden(format!(
                    "memory {id} was written or edited by the user; agents may add evidence or change its status, but must use supersede_memory to propose new text"
                )));
            }
            if patch.pinned.is_some() {
                return Err(Error::Forbidden("only the user can pin or unpin memories".into()));
            }
            if existing.user_locked && patch.certainty.is_some_and(|c| c != existing.certainty) && patch.add_evidence.is_empty() {
                return Err(Error::Forbidden("changing the certainty of a user-owned memory requires new evidence".into()));
            }
        }

        let kind = patch.kind.unwrap_or(existing.kind);
        let title = match &patch.title {
            Some(t) => privacy::redact_str(t.trim()),
            None => existing.title.clone(),
        };
        let content = match &patch.content {
            Some(c) => privacy::redact_str(c.trim()),
            None => existing.content.clone(),
        };
        validate_text(&title, &content)?;
        let status = match patch.status {
            Some(s) => s,
            None if patch.kind.is_some() && !kind.allowed_statuses().contains(&existing.status) => kind.default_status(),
            None => existing.status,
        };
        check_status(kind, status)?;
        let tags = match &patch.tags {
            Some(t) => normalize_tags(t)?,
            None => existing.tags.clone(),
        };
        if let Some(meta) = &patch.metadata
            && !meta.is_object()
        {
            return Err(Error::invalid("metadata must be a JSON object"));
        }

        let matcher = self.matcher_for(&project)?;
        let snap = self.snapshot_for(&project, patch.base_dir.as_deref());
        let files = self.prepare_files(&project, &snap.checkout_root, patch.base_dir.as_deref(), &patch.add_files, &matcher, &mut notes);
        let commits = self.prepare_commits(&project, &snap, &patch.add_commits, &mut notes);
        let mut evidence = self.prepare_evidence(&project, &snap, &patch.add_evidence, &matcher, &mut notes);

        let mut certainty = patch.certainty.unwrap_or(existing.certainty);
        if certainty == Certainty::Verified && existing.certainty != Certainty::Verified {
            let existing_evidence: i64 =
                self.conn.query_row("SELECT COUNT(*) FROM memory_evidence WHERE memory_id = ?1", [id], |r| r.get(0))?;
            let backed = evidence.iter().any(|e| e.counts) || commits.iter().any(|(_, ok)| *ok) || existing_evidence > 0;
            if !backed {
                if actor.is_user() {
                    evidence.push(PreparedEvidence {
                        kind: EvidenceKind::UserNote,
                        reference: "Marked verified by the user".into(),
                        excerpt: None,
                        counts: true,
                    });
                } else {
                    certainty = existing.certainty;
                    notes.push("certainty unchanged: 'verified' requires evidence".into());
                }
            }
        }

        let pinned = patch.pinned.unwrap_or(existing.pinned);
        let branch = match &patch.branch {
            Some(b) => b.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
            None => existing.branch.clone(),
        };
        let metadata = patch.metadata.clone().unwrap_or(existing.metadata.clone());
        let text_changed = title != existing.title || content != existing.content || kind != existing.kind;
        let core_changed = text_changed || status != existing.status || certainty != existing.certainty;
        let hash = content_hash(kind, &title, &content);
        let injection = privacy::injection_risk(&title) || privacy::injection_risk(&content);
        let user_locked = existing.user_locked || (actor.is_user() && text_changed);
        let now = util::now_ts();
        let reason = patch.reason.clone().map(|r| privacy::redact_str(&util::truncate_chars(&r, 300)));
        let remove: Vec<String> = patch.remove_files.iter().map(|p| p.trim().replace('\\', "/")).collect();
        let anything = core_changed
            || pinned != existing.pinned
            || branch != existing.branch
            || tags != existing.tags
            || metadata != existing.metadata
            || !files.is_empty()
            || !commits.is_empty()
            || !evidence.is_empty()
            || !remove.is_empty();
        if !anything {
            return Ok(SaveOutcome {
                memory_id: id.into(),
                action: SaveAction::Reinforced,
                certainty,
                status,
                notes: vec!["no changes".into()],
                similar: vec![],
            });
        }

        self.write_tx(|tx| {
            tx.execute(
                "UPDATE memories SET kind = ?2, title = ?3, content = ?4, status = ?5, certainty = ?6, tags_json = ?7, pinned = ?8,
                        branch = ?9, metadata_json = ?10, content_hash = ?11, injection_flag = ?12, user_locked = ?13,
                        updated_at = ?14, revision = revision + ?15
                 WHERE id = ?1",
                params![
                    id,
                    kind,
                    title,
                    content,
                    status,
                    certainty,
                    serde_json::to_string(&tags)?,
                    pinned as i64,
                    branch,
                    serde_json::to_string(&metadata)?,
                    hash,
                    injection as i64,
                    user_locked as i64,
                    now,
                    core_changed as i64
                ],
            )?;
            for p in &remove {
                tx.execute("DELETE FROM memory_files WHERE memory_id = ?1 AND path = ?2", params![id, p])?;
            }
            insert_files(tx, id, &files)?;
            for (sha, _) in &commits {
                tx.execute("INSERT OR IGNORE INTO memory_commits(memory_id, sha) VALUES (?1, ?2)", params![id, sha])?;
            }
            insert_evidence(tx, id, &evidence, actor, &now)?;
            if core_changed {
                tx.execute(
                    "INSERT INTO memory_revisions(memory_id, revision, title, content, status, certainty, changed_by, changed_at, reason)
                     SELECT id, revision, title, content, status, certainty, ?2, ?3, ?4 FROM memories WHERE id = ?1",
                    params![id, actor.agent.as_str(), now, reason.clone().unwrap_or_else(|| "updated".into())],
                )?;
            }
            crate::sessions::insert_event(
                tx,
                &existing.project_id,
                actor.session_id.as_deref(),
                actor.agent,
                EventKind::MemoryUpdated,
                &now,
                &format!("Updated: {}", util::truncate_chars(&title, 120)),
                &serde_json::json!({ "memory_id": id, "status": status, "certainty": certainty }),
            )?;
            Store::touch_project(tx, &existing.project_id, &now)?;
            Ok(())
        })?;
        if text_changed && !files.is_empty() {
            // New text with new references: refresh the snapshot of every reference.
            self.refresh_snapshot(id, &snap.checkout_root, snap.head.as_deref())?;
        }
        Ok(SaveOutcome { memory_id: id.into(), action: SaveAction::Created, certainty, status, notes, similar: vec![] })
    }

    /// Replace a memory with a new one. The old memory is kept (history is never destroyed) and marked superseded.
    /// If an agent tries to supersede a user-owned memory, the old one is flagged for the user's review instead.
    pub fn supersede_memory(&mut self, old_id: &str, mut replacement: NewMemory, reason: &str, actor: &Actor) -> Result<SaveOutcome> {
        let old = self.get_memory(old_id)?;
        if old.freshness == Freshness::Superseded {
            return Err(Error::Conflict(format!(
                "memory {old_id} is already superseded by {}",
                old.superseded_by.as_deref().unwrap_or("another memory")
            )));
        }
        if replacement.project_id != old.project_id {
            return Err(Error::invalid("a memory can only be superseded within the same project"));
        }
        replacement.links.push((old_id.to_string(), LinkRelation::Supersedes));
        let mut outcome = self.save_memory_inner(replacement, actor, Some(old_id))?;
        if outcome.memory_id == old_id {
            return Err(Error::Conflict("the replacement is identical to the memory being superseded".into()));
        }
        let now = util::now_ts();
        let reason = privacy::redact_str(&util::truncate_chars(reason.trim(), 300));
        let agent_vs_user = old.user_locked && !actor.is_user();
        let new_id = outcome.memory_id.clone();
        self.write_tx(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO memory_links(from_id, to_id, relation, created_at) VALUES (?1, ?2, 'supersedes', ?3)",
                params![new_id, old_id, now],
            )?;
            if agent_vs_user {
                let detail = format!("{} proposed a replacement ({new_id}): {reason}", actor.agent.label());
                tx.execute(
                    "UPDATE memories SET freshness = 'needs_review', stale_reason = ?2, updated_at = ?3 WHERE id = ?1",
                    params![old_id, detail, now],
                )?;
                tx.execute(
                    "INSERT INTO freshness_findings(memory_id, detected_at, kind, path, detail) VALUES (?1, ?2, 'replacement_proposed', NULL, ?3)",
                    params![old_id, now, detail],
                )?;
            } else {
                tx.execute(
                    "UPDATE memories SET freshness = 'superseded', superseded_by = ?2, stale_reason = ?3, updated_at = ?4,
                            revision = revision + 1 WHERE id = ?1",
                    params![old_id, new_id, if reason.is_empty() { "superseded".to_string() } else { reason.clone() }, now],
                )?;
                tx.execute(
                    "INSERT INTO memory_revisions(memory_id, revision, title, content, status, certainty, changed_by, changed_at, reason)
                     SELECT id, revision, title, content, status, certainty, ?2, ?3, ?4 FROM memories WHERE id = ?1",
                    params![old_id, actor.agent.as_str(), now, format!("superseded by {new_id}")],
                )?;
                tx.execute(
                    "UPDATE freshness_findings SET resolved_at = ?2, resolution = 'superseded' WHERE memory_id = ?1 AND resolved_at IS NULL",
                    params![old_id, now],
                )?;
            }
            Ok(())
        })?;
        if agent_vs_user {
            outcome.notes.push(format!("{old_id} is user-owned, so it was flagged 'needs_review' instead of being superseded"));
        } else {
            outcome.notes.push(format!("{old_id} is now superseded by {}", outcome.memory_id));
        }
        Ok(outcome)
    }

    /// Confirm a memory still matches the code: refresh file snapshots to the current state, record the
    /// validation as evidence, and mark it current.
    pub fn revalidate_memory(&mut self, id: &str, note: Option<&str>, evidence: Vec<EvidenceInput>, actor: &Actor) -> Result<SaveOutcome> {
        let memory = self.get_memory(id)?;
        if memory.freshness == Freshness::Superseded {
            return Err(Error::Conflict(format!("memory {id} is superseded; revalidate its replacement instead")));
        }
        let project = self.get_project(&memory.project_id)?;
        let mut notes = Vec::new();
        let matcher = self.matcher_for(&project)?;
        let snap = self.snapshot_for(&project, None);
        let mut ev = self.prepare_evidence(&project, &snap, &evidence, &matcher, &mut notes);
        let note_text = note.map(|n| privacy::redact_str(&util::truncate_chars(n.trim(), 1000))).filter(|n| !n.is_empty());
        let kind = if actor.is_user() { EvidenceKind::UserNote } else { EvidenceKind::AgentCheck };
        ev.push(PreparedEvidence { kind, reference: format!("Revalidated by {}", actor.agent.label()), excerpt: note_text, counts: true });
        let now = util::now_ts();
        self.write_tx(|tx| {
            insert_evidence(tx, id, &ev, actor, &now)?;
            tx.execute(
                "UPDATE memories SET freshness = 'current', stale_reason = NULL, last_validated_at = ?2, validated_commit = ?3,
                        updated_at = ?2 WHERE id = ?1",
                params![id, now, snap.head],
            )?;
            tx.execute(
                "UPDATE freshness_findings SET resolved_at = ?2, resolution = 'revalidated' WHERE memory_id = ?1 AND resolved_at IS NULL",
                params![id, now],
            )?;
            crate::sessions::insert_event(
                tx,
                &memory.project_id,
                actor.session_id.as_deref(),
                actor.agent,
                EventKind::MemoryUpdated,
                &now,
                &format!("Revalidated: {}", util::truncate_chars(&memory.title, 120)),
                &serde_json::json!({ "memory_id": id, "revalidated": true }),
            )?;
            Ok(())
        })?;
        self.refresh_snapshot(id, &snap.checkout_root, snap.head.as_deref())?;
        Ok(SaveOutcome {
            memory_id: id.into(),
            action: SaveAction::Reinforced,
            certainty: memory.certainty,
            status: memory.status,
            notes,
            similar: vec![],
        })
    }

    /// Re-hash every referenced file against `root` (after revalidation or at the end of the creating session).
    pub(crate) fn refresh_snapshot(&mut self, id: &str, root: &Path, head: Option<&str>) -> Result<()> {
        let mut stmt = self.conn.prepare_cached("SELECT path, sensitive FROM memory_files WHERE memory_id = ?1")?;
        let files: Vec<(String, bool)> =
            stmt.query_map([id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))?.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let hashes: Vec<(String, Option<String>)> = files
            .into_iter()
            .map(|(p, sensitive)| {
                let h = if sensitive { None } else { hash_file(&paths::join_rel(root, &p)) };
                (p, h)
            })
            .collect();
        let now = util::now_ts();
        self.write_tx(|tx| {
            for (p, h) in &hashes {
                tx.execute("UPDATE memory_files SET content_hash = ?3 WHERE memory_id = ?1 AND path = ?2", params![id, p, h])?;
            }
            if let Some(h) = head {
                tx.execute("UPDATE memories SET validated_commit = ?2, last_validated_at = ?3 WHERE id = ?1", params![id, h, now])?;
            }
            Ok(())
        })
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool, actor: &Actor) -> Result<()> {
        if !actor.is_user() {
            return Err(Error::Forbidden("only the user can pin or unpin memories".into()));
        }
        let m = self.get_memory(id)?;
        self.write_tx(|tx| {
            tx.execute("UPDATE memories SET pinned = ?2, updated_at = ?3 WHERE id = ?1", params![id, pinned as i64, util::now_ts()])?;
            Store::touch_project(tx, &m.project_id, &util::now_ts())?;
            Ok(())
        })
    }

    /// Permanently delete a memory and its provenance. Only the user may delete; agents supersede instead.
    pub fn delete_memory(&mut self, id: &str, actor: &Actor) -> Result<()> {
        if !actor.is_user() {
            return Err(Error::Forbidden("agents cannot delete memories; use supersede_memory to replace outdated ones".into()));
        }
        self.get_memory(id)?;
        self.write_tx(|tx| {
            tx.execute("UPDATE memories SET superseded_by = NULL WHERE superseded_by = ?1", [id])?;
            tx.execute("DELETE FROM memories WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Memories created by a session: refresh their snapshots when that session's turn ends, so edits made by
    /// the same session that recorded them do not flag them as stale.
    pub(crate) fn settle_session_memories(&mut self, session_id: &str, root: &Path) -> Result<usize> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id FROM memories WHERE source_session_id = ?1 AND freshness = 'current'
               AND EXISTS (SELECT 1 FROM memory_files f WHERE f.memory_id = memories.id)",
        )?;
        let ids: Vec<String> = stmt.query_map([session_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        if ids.is_empty() {
            return Ok(0);
        }
        let head = git::head_info(root).0;
        for id in &ids {
            self.refresh_snapshot(id, root, head.as_deref())?;
        }
        Ok(ids.len())
    }

    /// Recent memories for a project (used by the UI and handoffs).
    pub fn recent_memories(&self, project_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {MEMORY_COLUMNS_M} FROM memories m WHERE m.project_id = ?1 AND m.freshness != 'superseded'
             ORDER BY m.updated_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![project_id, limit as i64], row_to_memory)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

fn insert_files(tx: &rusqlite::Transaction<'_>, id: &str, files: &[PreparedFile]) -> Result<()> {
    for f in files {
        tx.execute(
            "INSERT INTO memory_files(memory_id, path, content_hash, line_start, line_end, sensitive) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(memory_id, path) DO UPDATE SET content_hash = excluded.content_hash, line_start = excluded.line_start,
                 line_end = excluded.line_end, sensitive = excluded.sensitive",
            params![id, f.path, f.hash, f.line_start, f.line_end, f.sensitive as i64],
        )?;
    }
    Ok(())
}

fn insert_evidence(tx: &rusqlite::Transaction<'_>, id: &str, evidence: &[PreparedEvidence], actor: &Actor, now: &str) -> Result<()> {
    for e in evidence {
        tx.execute(
            "INSERT INTO memory_evidence(memory_id, kind, reference, excerpt, agent, session_id, captured_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, e.kind, e.reference, e.excerpt, actor.agent, actor.session_id, now],
        )?;
    }
    Ok(())
}
