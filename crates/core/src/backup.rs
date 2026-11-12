//! Export/import (portable JSON) and full database backup/restore.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::model::{Evidence, Memory, MemoryFile, MemoryLink, Revision};
use crate::paths::{canonical, path_key};
use crate::projects::Project;
use crate::{Error, Result, Store, util};

pub const EXPORT_FORMAT: &str = "contextforge-export";
pub const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedMemory {
    #[serde(flatten)]
    pub memory: Memory,
    pub files: Vec<MemoryFile>,
    pub commits: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub links: Vec<MemoryLink>,
    pub revisions: Vec<Revision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedProject {
    pub project: Project,
    pub memories: Vec<ExportedMemory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportDoc {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub app_version: String,
    pub projects: Vec<ExportedProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ImportReport {
    pub projects_created: usize,
    pub projects_matched: usize,
    pub memories_imported: usize,
    pub memories_skipped: usize,
    pub links_imported: usize,
}

impl Store {
    /// Export memories (with provenance) for one project or all projects.
    pub fn export_json(&self, project_id: Option<&str>) -> Result<ExportDoc> {
        let projects: Vec<Project> = match project_id {
            Some(id) => vec![self.get_project(id)?],
            None => self.list_projects()?.into_iter().map(|s| s.project).collect(),
        };
        let mut out = Vec::new();
        for p in projects {
            let mut stmt = self.conn.prepare("SELECT id FROM memories WHERE project_id = ?1 ORDER BY created_at, id")?;
            let ids: Vec<String> = stmt.query_map([&p.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let mut memories = Vec::new();
            for id in ids {
                let d = self.get_memory_detail(&id)?;
                memories.push(ExportedMemory {
                    memory: d.memory,
                    files: d.files,
                    commits: d.commits,
                    evidence: d.evidence,
                    links: d.links,
                    revisions: d.revisions,
                });
            }
            out.push(ExportedProject { project: p, memories });
        }
        Ok(ExportDoc {
            format: EXPORT_FORMAT.into(),
            version: EXPORT_VERSION,
            exported_at: util::now_ts(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            projects: out,
        })
    }

    pub fn export_to_file(&self, project_id: Option<&str>, path: &Path) -> Result<usize> {
        let doc = self.export_json(project_id)?;
        let count = doc.projects.iter().map(|p| p.memories.len()).sum();
        crate::db::write_atomic(path, serde_json::to_string_pretty(&doc)?.as_bytes())?;
        Ok(count)
    }

    /// Import an export document. Existing memories (same id) are kept untouched; projects are matched by path.
    pub fn import_json(&mut self, doc: &ExportDoc) -> Result<ImportReport> {
        if doc.format != EXPORT_FORMAT {
            return Err(Error::invalid("not a ContextForge export file"));
        }
        if doc.version > EXPORT_VERSION {
            return Err(Error::invalid(format!("export version {} is newer than supported ({EXPORT_VERSION})", doc.version)));
        }
        let mut report = ImportReport::default();
        for ep in &doc.projects {
            let root = PathBuf::from(&ep.project.root_path);
            let key = path_key(&canonical(&root));
            let existing: Option<String> =
                self.conn.query_row("SELECT project_id FROM project_roots WHERE path_key = ?1", [&key], |r| r.get(0)).optional()?;
            let project_id = match existing {
                Some(id) => {
                    report.projects_matched += 1;
                    id
                }
                None if root.is_dir() => {
                    let (p, created) = self.register_project(&root, Some(&ep.project.name))?;
                    if created {
                        report.projects_created += 1
                    } else {
                        report.projects_matched += 1
                    }
                    p.id
                }
                None => {
                    // The folder does not exist on this machine: keep the record so memories are not lost.
                    let id = util::new_id("prj");
                    let now = util::now_ts();
                    let p = &ep.project;
                    self.write_tx(|tx| {
                        tx.execute(
                            "INSERT INTO projects(id, name, root_path, path_key, is_git, repo_fingerprint, remote_url, default_branch,
                                                  settings_json, created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                            params![
                                id,
                                p.name,
                                p.root_path,
                                key,
                                p.is_git as i64,
                                p.repo_fingerprint,
                                p.remote_url,
                                p.default_branch,
                                serde_json::to_string(&p.settings)?,
                                now
                            ],
                        )?;
                        tx.execute(
                            "INSERT INTO project_roots(project_id, path, path_key, kind, added_at) VALUES (?1, ?2, ?3, 'main', ?4)",
                            params![id, p.root_path, key, now],
                        )?;
                        Ok(())
                    })?;
                    report.projects_created += 1;
                    id
                }
            };
            let mut imported_ids = Vec::new();
            for em in &ep.memories {
                let m = &em.memory;
                let exists: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)", [&m.id], |r| r.get(0))?;
                if exists {
                    report.memories_skipped += 1;
                    continue;
                }
                let pid = project_id.clone();
                let hash = crate::memories::content_hash(m.kind, &m.title, &m.content);
                self.write_tx(|tx| {
                    tx.execute(
                        "INSERT INTO memories(id, project_id, kind, status, certainty, freshness, title, content, tags_json, pinned, user_locked,
                            branch, origin_branch, source_agent, source_session_id, source_origin, created_at, updated_at, last_validated_at,
                            validated_commit, superseded_by, stale_reason, injection_flag, content_hash, revision, reinforced_count, metadata_json)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16, ?17, ?18, ?19, NULL, ?20, ?21, ?22, ?23, ?24, ?25)",
                        params![
                            m.id, pid, m.kind, m.status, m.certainty, m.freshness, m.title, m.content, serde_json::to_string(&m.tags)?,
                            m.pinned as i64, m.user_locked as i64, m.branch, m.origin_branch, m.source_agent, m.source_origin,
                            m.created_at, m.updated_at, m.last_validated_at, m.validated_commit, m.stale_reason,
                            crate::privacy::injection_risk(&m.title) || crate::privacy::injection_risk(&m.content) || m.injection_flag,
                            hash, m.revision, m.reinforced_count, serde_json::to_string(&m.metadata)?
                        ],
                    )?;
                    for f in &em.files {
                        tx.execute(
                            "INSERT OR IGNORE INTO memory_files(memory_id, path, content_hash, line_start, line_end, sensitive) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![m.id, f.path, f.content_hash, f.line_start, f.line_end, f.sensitive as i64],
                        )?;
                    }
                    for c in &em.commits {
                        tx.execute("INSERT OR IGNORE INTO memory_commits(memory_id, sha) VALUES (?1, ?2)", params![m.id, c])?;
                    }
                    for e in &em.evidence {
                        tx.execute(
                            "INSERT INTO memory_evidence(memory_id, kind, reference, excerpt, agent, session_id, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                            params![m.id, e.kind, e.reference, e.excerpt, e.agent, e.captured_at],
                        )?;
                    }
                    for r in &em.revisions {
                        tx.execute(
                            "INSERT OR IGNORE INTO memory_revisions(memory_id, revision, title, content, status, certainty, changed_by, changed_at, reason)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![m.id, r.revision, r.title, r.content, r.status, r.certainty, r.changed_by, r.changed_at, r.reason],
                        )?;
                    }
                    Ok(())
                })?;
                imported_ids.push(m.id.clone());
                report.memories_imported += 1;
            }
            // Links and supersession pointers once all memories exist.
            for em in ep.memories.iter().filter(|em| imported_ids.contains(&em.memory.id)) {
                let links = em.links.clone();
                let superseded_by = em.memory.superseded_by.clone();
                let mid = em.memory.id.clone();
                let n = self.write_tx(|tx| {
                    let mut n = 0;
                    for l in &links {
                        n += tx.execute(
                            "INSERT OR IGNORE INTO memory_links(from_id, to_id, relation, created_at)
                             SELECT ?1, ?2, ?3, ?4 WHERE EXISTS(SELECT 1 FROM memories WHERE id = ?1) AND EXISTS(SELECT 1 FROM memories WHERE id = ?2)",
                            params![l.from_id, l.to_id, l.relation, l.created_at],
                        )?;
                    }
                    if let Some(s) = &superseded_by {
                        tx.execute(
                            "UPDATE memories SET superseded_by = ?2 WHERE id = ?1 AND EXISTS(SELECT 1 FROM memories WHERE id = ?2)",
                            params![mid, s],
                        )?;
                    }
                    Ok(n)
                })?;
                report.links_imported += n;
            }
        }
        Ok(report)
    }

    pub fn import_from_file(&mut self, path: &Path) -> Result<ImportReport> {
        let meta = std::fs::metadata(path)?;
        if meta.len() > 512 * 1024 * 1024 {
            return Err(Error::invalid("import file is larger than 512 MB"));
        }
        let text = std::fs::read_to_string(path)?;
        let doc: ExportDoc = serde_json::from_str(&text).map_err(|e| Error::invalid(format!("not a valid ContextForge export: {e}")))?;
        self.import_json(&doc)
    }

    /// Consistent snapshot of the whole database (safe while other processes write).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            return Err(Error::Conflict(format!("'{}' already exists", dest.display())));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }

    /// Default location for automatic/manual backups.
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir().join("backups")
    }

    pub fn backup_now(&self) -> Result<PathBuf> {
        let name = format!("contextforge-{}.db", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"));
        let dest = self.backups_dir().join(name);
        self.backup_to(&dest)?;
        Ok(dest)
    }

    /// Replace the database contents with a backup. The current state is backed up first.
    pub fn restore_from(&mut self, src: &Path) -> Result<PathBuf> {
        let source = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| Error::invalid(format!("cannot open backup: {e}")))?;
        let ok: bool = source
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'memories')", [], |r| r.get(0))
            .map_err(|e| Error::invalid(format!("not a ContextForge database: {e}")))?;
        if !ok {
            return Err(Error::invalid("not a ContextForge database (no memories table)"));
        }
        let version: i64 = source
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))
            .map_err(|e| Error::invalid(format!("not a ContextForge database: {e}")))?;
        if version > crate::db::SCHEMA_VERSION {
            return Err(Error::Conflict("the backup was made by a newer ContextForge version".into()));
        }
        let integrity: String = source.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(Error::invalid(format!("backup failed its integrity check: {integrity}")));
        }
        let safety = self.backups_dir().join(format!("pre-restore-{}.db", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f")));
        self.backup_to(&safety)?;
        {
            let backup = rusqlite::backup::Backup::new(&source, &mut self.conn)?;
            backup.run_to_completion(256, Duration::from_millis(5), None)?;
        }
        Ok(safety)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupFile {
    pub path: String,
    pub size_bytes: u64,
    pub modified: String,
}

impl Store {
    pub fn list_backups(&self) -> Result<Vec<BackupFile>> {
        let dir = self.backups_dir();
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "db")
                    && let Ok(meta) = e.metadata()
                {
                    let modified = meta.modified().ok().map(|t| util::ts(chrono::DateTime::<chrono::Utc>::from(t))).unwrap_or_default();
                    out.push(BackupFile { path: p.to_string_lossy().into_owned(), size_bytes: meta.len(), modified });
                }
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    }
}
