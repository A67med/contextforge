//! Git-aware freshness evaluation.
//!
//! Each memory stores a snapshot of the files it references (content hash, line endings normalised) and the
//! HEAD commit at validation time. A check compares the snapshot with the checkout being examined:
//!
//! | observation                                  | finding          | freshness          |
//! |----------------------------------------------|------------------|--------------------|
//! | referenced file unchanged                    | —                | current            |
//! | referenced file modified                     | `file_modified`  | potentially stale  |
//! | file renamed (git rename detection)          | `file_renamed`   | reference follows  |
//! | referenced file deleted                      | `file_deleted`   | needs review       |
//! | referenced commit no longer exists (rebase)  | `commit_missing` | needs review       |
//!
//! Decisions and conventions describe intent, so they are only flagged for modifications that change at least
//! half of the file. Findings are recomputed on every check: when the checkout matches the snapshot again
//! (e.g. after switching back to a branch) automatic findings are resolved as `auto_cleared`. Memory content is
//! never modified or deleted by a freshness check.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::model::{Actor, EventKind, FindingKind, Freshness, MemoryKind};
use crate::{Error, Result, Store, git, memories::hash_file, paths, util};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreshnessChange {
    pub memory_id: String,
    pub title: String,
    pub from: Freshness,
    pub to: Freshness,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreshnessReport {
    pub project_id: String,
    pub root: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub checked: usize,
    pub changes: Vec<FreshnessChange>,
    pub renames_followed: usize,
    /// Memories currently potentially stale or needing review (after this check).
    pub flagged_total: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
struct Issue {
    kind: FindingKind,
    path: Option<String>,
    detail: String,
}

struct MemRow {
    id: String,
    kind: MemoryKind,
    title: String,
    freshness: Freshness,
    branch: Option<String>,
    validated_commit: Option<String>,
    stale_reason: Option<String>,
}

impl Store {
    /// Evaluate freshness for every non-superseded memory of a project against `root`
    /// (defaults to the project's main checkout).
    pub fn check_freshness(&mut self, project_id: &str, root: Option<&Path>, actor: &Actor) -> Result<FreshnessReport> {
        self.check_freshness_with(project_id, root, None, actor)
    }

    /// Like [`Store::check_freshness`], reusing an already computed `git status` of `root`.
    pub fn check_freshness_with(
        &mut self,
        project_id: &str,
        root: Option<&Path>,
        status: Option<git::WorkingTree>,
        actor: &Actor,
    ) -> Result<FreshnessReport> {
        let started = Instant::now();
        let project = self.get_project(project_id)?;
        let root: PathBuf = root.map(Path::to_path_buf).unwrap_or_else(|| project.root());
        if !root.is_dir() {
            return Err(Error::not_found(format!("project checkout '{}' no longer exists", root.display())));
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, kind, title, freshness, branch, validated_commit, stale_reason FROM memories m
             WHERE project_id = ?1 AND freshness != 'superseded'
               AND (EXISTS (SELECT 1 FROM memory_files f WHERE f.memory_id = m.id)
                    OR EXISTS (SELECT 1 FROM memory_commits c WHERE c.memory_id = m.id))",
        )?;
        let mems: Vec<MemRow> = stmt
            .query_map([project_id], |r| {
                Ok(MemRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    freshness: r.get(3)?,
                    branch: r.get(4)?,
                    validated_commit: r.get(5)?,
                    stale_reason: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);

        let mut files_by_mem: HashMap<String, Vec<(String, Option<String>, bool)>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT f.memory_id, f.path, f.content_hash, f.sensitive FROM memory_files f JOIN memories m ON m.id = f.memory_id
             WHERE m.project_id = ?1 AND m.freshness != 'superseded'",
        )?;
        for row in stmt.query_map([project_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)? != 0))
        })? {
            let (mid, path, hash, sensitive) = row?;
            files_by_mem.entry(mid).or_default().push((path, hash, sensitive));
        }
        drop(stmt);
        let mut commits_by_mem: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT c.memory_id, c.sha FROM memory_commits c JOIN memories m ON m.id = c.memory_id
             WHERE m.project_id = ?1 AND m.freshness != 'superseded'",
        )?;
        for row in stmt.query_map([project_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (mid, sha) = row?;
            commits_by_mem.entry(mid).or_default().push(sha);
        }
        drop(stmt);

        // Git state of the checkout being examined.
        let wt = match status {
            Some(s) if project.is_git => Some(s),
            _ if project.is_git => git::status(&root).ok(),
            _ => None,
        };
        let head = wt.as_ref().and_then(|w| w.head.clone());
        let branch = wt.as_ref().and_then(|w| w.branch.clone());
        let dirty: HashSet<String> = wt.as_ref().map(|w| w.changed_paths()).unwrap_or_default();

        // Current hashes for every referenced, non-sensitive path.
        let mut current: HashMap<String, Option<String>> = HashMap::new();
        for files in files_by_mem.values() {
            for (path, _, sensitive) in files {
                if !*sensitive && !current.contains_key(path) {
                    current.insert(path.clone(), hash_file(&paths::join_rel(&root, path)));
                }
            }
        }

        // Rename detection, only for missing files, grouped by snapshot commit.
        let mut renames: HashMap<String, String> = HashMap::new();
        if project.is_git && wt.is_some() {
            let mut commits_needing_renames: Vec<String> = Vec::new();
            for m in &mems {
                let has_missing = files_by_mem
                    .get(&m.id)
                    .is_some_and(|fs| fs.iter().any(|(p, h, s)| !s && h.is_some() && current.get(p).is_some_and(|c| c.is_none())));
                if has_missing
                    && let Some(c) = &m.validated_commit
                    && !commits_needing_renames.contains(c)
                {
                    commits_needing_renames.push(c.clone());
                }
            }
            let missing = git::missing_commits(&root, &commits_needing_renames).unwrap_or_default();
            for c in commits_needing_renames.iter().filter(|c| !missing.contains(*c)).take(10) {
                if let Ok(list) = git::renames_since(&root, c) {
                    for (old, new) in list {
                        renames.entry(old).or_insert(new);
                    }
                }
            }
        }

        // Referenced commits that no longer exist (history rewritten, or a different clone).
        let all_shas: Vec<String> = commits_by_mem.values().flatten().cloned().collect::<HashSet<_>>().into_iter().collect();
        let missing_commits: HashSet<String> =
            if project.is_git && wt.is_some() { git::missing_commits(&root, &all_shas).unwrap_or_default() } else { HashSet::new() };

        let now = util::now_ts();
        let mut changes = Vec::new();
        let mut renames_followed = 0usize;
        let mut checked = 0usize;
        let mut new_hashes_for_renames: HashMap<String, Option<String>> = HashMap::new();

        for m in &mems {
            if let Some(scope) = &m.branch
                && branch.as_deref() != Some(scope.as_str())
            {
                continue; // branch-scoped memory: only evaluated on its own branch
            }
            checked += 1;
            let mut issues: Vec<Issue> = Vec::new();
            let mut follow: Vec<(String, String)> = Vec::new();
            for (path, snap, sensitive) in files_by_mem.get(&m.id).map(|v| v.as_slice()).unwrap_or(&[]) {
                if *sensitive {
                    continue;
                }
                let Some(snap) = snap else { continue };
                let cur = current.get(path).cloned().flatten();
                match cur {
                    Some(c) if &c == snap => {}
                    Some(_) => {
                        if let Some(issue) = modified_issue(&root, m, path, dirty.contains(path), project.is_git) {
                            issues.push(issue);
                        }
                    }
                    None => {
                        if let Some(new_path) = renames.get(path) {
                            follow.push((path.clone(), new_path.clone()));
                            issues.push(Issue {
                                kind: FindingKind::FileRenamed,
                                path: Some(path.clone()),
                                detail: format!("'{path}' was renamed to '{new_path}'; the reference now follows the new path"),
                            });
                            let new_hash = new_hashes_for_renames
                                .entry(new_path.clone())
                                .or_insert_with(|| hash_file(&paths::join_rel(&root, new_path)))
                                .clone();
                            if new_hash.as_deref() != Some(snap.as_str())
                                && let Some(issue) = modified_issue(&root, m, new_path, dirty.contains(new_path), project.is_git)
                            {
                                issues.push(issue);
                            }
                        } else {
                            issues.push(Issue {
                                kind: FindingKind::FileDeleted,
                                path: Some(path.clone()),
                                detail: format!("'{path}' no longer exists in this checkout"),
                            });
                        }
                    }
                }
            }
            for sha in commits_by_mem.get(&m.id).map(|v| v.as_slice()).unwrap_or(&[]) {
                if missing_commits.contains(sha) {
                    issues.push(Issue {
                        kind: FindingKind::CommitMissing,
                        path: None,
                        detail: format!(
                            "commit {} is not in this repository (history rewritten or different clone)",
                            &sha[..sha.len().min(10)]
                        ),
                    });
                }
            }

            let open = self.findings_for(&m.id, false)?;
            let manual_severity = open
                .iter()
                .filter(|f| !f.kind.is_automatic())
                .map(|f| f.kind.implied_freshness())
                .max_by_key(|f| f.severity())
                .unwrap_or(Freshness::Current);
            let auto_severity =
                issues.iter().map(|i| i.kind.implied_freshness()).max_by_key(|f| f.severity()).unwrap_or(Freshness::Current);
            let target = if manual_severity.severity() > auto_severity.severity() { manual_severity } else { auto_severity };

            let reasons: Vec<String> = issues.iter().filter(|i| i.kind != FindingKind::FileRenamed).map(|i| i.detail.clone()).collect();
            let manual_reason = open.iter().find(|f| !f.kind.is_automatic()).map(|f| f.detail.clone());
            let stale_reason = if !reasons.is_empty() { Some(util::truncate_chars(&reasons.join("; "), 400)) } else { manual_reason };

            let issue_keys: HashSet<(FindingKind, Option<String>)> = issues.iter().map(|i| (i.kind, i.path.clone())).collect();
            let open_keys: HashSet<(FindingKind, Option<String>)> = open.iter().map(|f| (f.kind, f.path.clone())).collect();
            let head_c = head.clone();
            let branch_c = branch.clone();
            let mid = m.id.clone();
            let changed_state = target != m.freshness || stale_reason != m.stale_reason;
            let any_finding_change = issues.iter().any(|i| !open_keys.contains(&(i.kind, i.path.clone())))
                || open.iter().any(|f| f.kind.is_automatic() && !issue_keys.contains(&(f.kind, f.path.clone())))
                || !follow.is_empty();
            if !changed_state && !any_finding_change {
                continue;
            }
            self.write_tx(|tx| {
                for issue in &issues {
                    if open_keys.contains(&(issue.kind, issue.path.clone())) {
                        tx.execute(
                            "UPDATE freshness_findings SET detail = ?3 WHERE memory_id = ?1 AND kind = ?2 AND path IS ?4 AND resolved_at IS NULL",
                            params![mid, issue.kind, issue.detail, issue.path],
                        )?;
                        continue;
                    }
                    let (resolved_at, resolution) =
                        if issue.kind == FindingKind::FileRenamed { (Some(now.clone()), Some("auto_followed")) } else { (None, None) };
                    tx.execute(
                        "INSERT INTO freshness_findings(memory_id, detected_at, kind, path, detail, head_commit, branch, resolved_at, resolution)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![mid, now, issue.kind, issue.path, issue.detail, head_c, branch_c, resolved_at, resolution],
                    )?;
                }
                for f in &open {
                    if f.kind.is_automatic() && !issue_keys.contains(&(f.kind, f.path.clone())) {
                        tx.execute(
                            "UPDATE freshness_findings SET resolved_at = ?2, resolution = 'auto_cleared' WHERE id = ?1",
                            params![f.id, now],
                        )?;
                    }
                }
                for (old, new) in &follow {
                    let dup: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_files WHERE memory_id = ?1 AND path = ?2)",
                        params![mid, new],
                        |r| r.get(0),
                    )?;
                    if dup {
                        tx.execute("DELETE FROM memory_files WHERE memory_id = ?1 AND path = ?2", params![mid, old])?;
                    } else {
                        tx.execute("UPDATE memory_files SET path = ?3 WHERE memory_id = ?1 AND path = ?2", params![mid, old, new])?;
                    }
                }
                if changed_state {
                    tx.execute(
                        "UPDATE memories SET freshness = ?2, stale_reason = ?3 WHERE id = ?1",
                        params![mid, target, stale_reason],
                    )?;
                }
                Ok(())
            })?;
            renames_followed += follow.len();
            if target != m.freshness {
                changes.push(FreshnessChange { memory_id: m.id.clone(), title: m.title.clone(), from: m.freshness, to: target, reasons });
            }
        }

        let flagged_total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE project_id = ?1 AND freshness IN ('potentially_stale', 'needs_review')",
            [project_id],
            |r| r.get(0),
        )?;
        let duration_ms = started.elapsed().as_millis() as u64;
        if !changes.is_empty() || renames_followed > 0 {
            let flagged = changes.iter().filter(|c| c.to.severity() > 0).count();
            let cleared = changes.iter().filter(|c| c.to == Freshness::Current).count();
            let summary = format!(
                "Freshness check: {checked} checked, {flagged} newly flagged, {cleared} cleared, {renames_followed} renames followed"
            );
            let detail = serde_json::json!({ "changes": changes, "head": head, "branch": branch });
            self.write_tx(|tx| {
                crate::sessions::insert_event(
                    tx,
                    project_id,
                    actor.session_id.as_deref(),
                    actor.agent,
                    EventKind::FreshnessCheck,
                    &now,
                    &summary,
                    &detail,
                )?;
                Ok(())
            })?;
        }
        Ok(FreshnessReport {
            project_id: project_id.to_string(),
            root: root.to_string_lossy().into_owned(),
            head,
            branch,
            checked,
            changes,
            renames_followed,
            flagged_total: flagged_total as usize,
            duration_ms,
        })
    }
}

fn modified_issue(root: &Path, m: &MemRow, path: &str, dirty: bool, is_git: bool) -> Option<Issue> {
    let where_ = if dirty { " (uncommitted working-tree change)" } else { "" };
    if m.kind.tolerant_to_edits() {
        // Intent-level memories: only substantial rewrites are flagged.
        let fraction = match (&m.validated_commit, is_git) {
            (Some(c), true) => git::numstat_since(root, c, path).map(|(added, removed)| {
                let lines = std::fs::read(paths::join_rel(root, path)).map(|b| b.iter().filter(|&&x| x == b'\n').count()).unwrap_or(0);
                added.max(removed) as f64 / (lines.max(1)) as f64
            }),
            _ => None,
        };
        match fraction {
            Some(f) if f < 0.5 => return None,
            Some(f) => {
                return Some(Issue {
                    kind: FindingKind::FileModified,
                    path: Some(path.to_string()),
                    detail: format!(
                        "'{path}' changed substantially (~{:.0}% of lines) since this {} was recorded{where_}",
                        (f * 100.0).min(100.0),
                        m.kind
                    ),
                });
            }
            None => {}
        }
    }
    Some(Issue {
        kind: FindingKind::FileModified,
        path: Some(path.to_string()),
        detail: format!("'{path}' was modified after this memory was last validated{where_}"),
    })
}
