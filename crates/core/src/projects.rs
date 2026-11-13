//! Project registration and identification (path → project, including Git worktrees).

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::model::AgentKind;
use crate::paths::{canonical, path_key};
use crate::{Error, Result, Store, git, util};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectSettings {
    /// Capture hook events for this project.
    pub capture_enabled: bool,
    /// Extra sensitive/excluded globs for this project (project-relative).
    pub excluded_globs: Vec<String>,
    /// Override the startup context budget for this project.
    pub startup_budget_tokens: Option<usize>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings { capture_enabled: true, excluded_globs: Vec::new(), startup_budget_tokens: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectRoot {
    pub path: String,
    pub kind: String,
    pub branch: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub is_git: bool,
    pub repo_fingerprint: Option<String>,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    pub settings: ProjectSettings,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: Option<String>,
    pub roots: Vec<ProjectRoot>,
}

impl Project {
    pub fn root(&self) -> PathBuf {
        PathBuf::from(&self.root_path)
    }
    /// All checkout roots (main + linked worktrees).
    pub fn all_roots(&self) -> Vec<PathBuf> {
        let mut v = vec![self.root()];
        for r in &self.roots {
            let p = PathBuf::from(&r.path);
            if path_key(&p) != path_key(&self.root()) {
                v.push(p);
            }
        }
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectSummary {
    pub project: Project,
    pub memory_count: i64,
    pub open_tasks: i64,
    pub open_bugs: i64,
    pub needs_review: i64,
    pub session_count: i64,
    pub agents: Vec<AgentKind>,
}

/// Result of resolving a filesystem path to a registered project.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub project: Project,
    /// The checkout root that contains the path (main root or a worktree root).
    pub checkout_root: PathBuf,
}

fn row_to_project(r: &Row<'_>) -> rusqlite::Result<Project> {
    let settings_json: String = r.get("settings_json")?;
    Ok(Project {
        id: r.get("id")?,
        name: r.get("name")?,
        root_path: r.get("root_path")?,
        is_git: r.get::<_, i64>("is_git")? != 0,
        repo_fingerprint: r.get("repo_fingerprint")?,
        remote_url: r.get("remote_url")?,
        default_branch: r.get("default_branch")?,
        settings: serde_json::from_str(&settings_json).unwrap_or_default(),
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        last_activity_at: r.get("last_activity_at")?,
        roots: Vec::new(),
    })
}

fn is_too_broad(path: &Path) -> bool {
    if path.parent().is_none() {
        return true; // filesystem root such as C:\ or /
    }
    if let Some(home) = dirs::home_dir()
        && path_key(&canonical(&home)) == path_key(path)
    {
        return true;
    }
    false
}

impl Store {
    /// Register a directory as a project. Inside a Git repository the repository top-level is used, and a
    /// linked worktree of an already-registered repository is attached to that project.
    /// Returns the project and whether it was newly created.
    pub fn register_project(&mut self, path: &Path, name: Option<&str>) -> Result<(Project, bool)> {
        let dir = canonical(path);
        if !dir.is_dir() {
            return Err(Error::invalid(format!("'{}' is not an existing directory", path.display())));
        }
        let repo = git::discover(&dir);
        let root = repo.as_ref().map(|r| r.toplevel.clone()).unwrap_or_else(|| dir.clone());
        if is_too_broad(&root) {
            return Err(Error::invalid(format!(
                "'{}' is too broad to register (filesystem root or home directory); choose a project folder",
                root.display()
            )));
        }
        let key = path_key(&root);
        if let Some(existing) = self.project_by_root_key(&key)? {
            return Ok((existing, false));
        }
        let common_key = repo.as_ref().map(|r| path_key(&r.common_dir));
        if let (Some(info), Some(ck)) = (&repo, &common_key)
            && let Some(owner) = self.project_by_common_dir(ck)?
        {
            let branch = git::status(&info.toplevel).ok().and_then(|w| w.branch);
            self.add_root(&owner.id, &info.toplevel, if info.is_worktree { "worktree" } else { "main" }, branch.as_deref())?;
            return Ok((self.get_project(&owner.id)?, false));
        }

        let id = util::new_id("prj");
        let now = util::now_ts();
        let display_name = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into()));
        if display_name.chars().count() > 120 {
            return Err(Error::invalid("project name must be at most 120 characters"));
        }
        let (fingerprint, remote, default_branch, branch) = match &repo {
            Some(info) => {
                let roots = git::root_commits(&info.toplevel);
                let fp = if roots.is_empty() { None } else { Some(util::sha256_hex(roots.join(",").as_bytes())[..16].to_string()) };
                let wt = git::status(&info.toplevel).ok();
                (fp, git::remote_url(&info.toplevel), git::default_branch(&info.toplevel), wt.and_then(|w| w.branch))
            }
            None => (None, None, None, None),
        };
        let root_str = root.to_string_lossy().into_owned();
        let kind = if repo.as_ref().is_some_and(|r| r.is_worktree) { "worktree" } else { "main" };
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO projects(id, name, root_path, path_key, is_git, git_common_dir_key, repo_fingerprint, remote_url,
                                      default_branch, settings_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    id,
                    display_name,
                    root_str,
                    key,
                    repo.is_some() as i64,
                    common_key,
                    fingerprint,
                    remote,
                    default_branch.clone().or(branch.clone()),
                    serde_json::to_string(&ProjectSettings::default())?,
                    now
                ],
            )?;
            tx.execute(
                "INSERT INTO project_roots(project_id, path, path_key, kind, branch, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, root_str, key, kind, branch, now],
            )?;
            Ok(())
        })?;
        Ok((self.get_project(&id)?, true))
    }

    fn add_root(&mut self, project_id: &str, root: &Path, kind: &str, branch: Option<&str>) -> Result<()> {
        let key = path_key(root);
        let path = root.to_string_lossy().into_owned();
        let now = util::now_ts();
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO project_roots(project_id, path, path_key, kind, branch, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path_key) DO UPDATE SET branch = excluded.branch",
                params![project_id, path, key, kind, branch, now],
            )?;
            Ok(())
        })
    }

    fn project_by_root_key(&self, key: &str) -> Result<Option<Project>> {
        let id: Option<String> =
            self.conn.query_row("SELECT project_id FROM project_roots WHERE path_key = ?1", [key], |r| r.get(0)).optional()?;
        match id {
            Some(id) => Ok(Some(self.get_project(&id)?)),
            None => Ok(None),
        }
    }

    fn project_by_common_dir(&self, common_key: &str) -> Result<Option<Project>> {
        let id: Option<String> =
            self.conn.query_row("SELECT id FROM projects WHERE git_common_dir_key = ?1 LIMIT 1", [common_key], |r| r.get(0)).optional()?;
        match id {
            Some(id) => Ok(Some(self.get_project(&id)?)),
            None => Ok(None),
        }
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        let mut project = self
            .conn
            .query_row("SELECT * FROM projects WHERE id = ?1", [id], row_to_project)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("project '{id}'")))?;
        let mut stmt = self
            .conn
            .prepare_cached("SELECT path, kind, branch, added_at FROM project_roots WHERE project_id = ?1 ORDER BY kind, added_at")?;
        project.roots = stmt
            .query_map([id], |r| Ok(ProjectRoot { path: r.get(0)?, kind: r.get(1)?, branch: r.get(2)?, added_at: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(project)
    }

    /// Look up a project by id, exact registered root, or any path inside one of its checkouts.
    pub fn find_project(&self, id_or_path: &str) -> Result<Option<Project>> {
        let s = id_or_path.trim();
        if s.starts_with("prj_") {
            return match self.get_project(s) {
                Ok(p) => Ok(Some(p)),
                Err(Error::NotFound(_)) => Ok(None),
                Err(e) => Err(e),
            };
        }
        Ok(self.match_registered_root(&canonical(Path::new(s)))?.map(|r| r.project))
    }

    fn match_registered_root(&self, path: &Path) -> Result<Option<Resolved>> {
        let key = path_key(path);
        let mut stmt = self.conn.prepare_cached("SELECT project_id, path, path_key FROM project_roots")?;
        let rows: Vec<(String, String, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let best = rows.into_iter().filter(|(_, _, rk)| key == *rk || key.starts_with(&format!("{rk}/"))).max_by_key(|(_, _, rk)| rk.len());
        match best {
            Some((pid, root, _)) => Ok(Some(Resolved { project: self.get_project(&pid)?, checkout_root: PathBuf::from(root) })),
            None => Ok(None),
        }
    }

    /// Resolve the project that owns `path` (a session cwd or workspace folder).
    ///
    /// Fast path: prefix match against registered roots (no process spawn). Slow path: ask Git for the
    /// repository's common dir so new linked worktrees of a registered repository are recognised.
    /// With `auto_register`, unregistered Git repositories are registered on the fly.
    pub fn resolve_project(&mut self, path: &Path, auto_register: bool) -> Result<Option<Resolved>> {
        let dir = canonical(path);
        if let Some(r) = self.match_registered_root(&dir)? {
            return Ok(Some(r));
        }
        let any_git: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE is_git = 1)", [], |r| r.get(0))?;
        if !any_git && !auto_register {
            return Ok(None);
        }
        let Some(info) = git::discover(&dir) else {
            return Ok(None);
        };
        if let Some(owner) = self.project_by_common_dir(&path_key(&info.common_dir))? {
            let branch = git::status(&info.toplevel).ok().and_then(|w| w.branch);
            self.add_root(&owner.id, &info.toplevel, if info.is_worktree { "worktree" } else { "main" }, branch.as_deref())?;
            return Ok(Some(Resolved { project: self.get_project(&owner.id)?, checkout_root: info.toplevel }));
        }
        if auto_register && !is_too_broad(&info.toplevel) {
            let (project, _) = self.register_project(&info.toplevel, None)?;
            return Ok(Some(Resolved { checkout_root: project.root(), project }));
        }
        Ok(None)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        let mut stmt = self.conn.prepare("SELECT id FROM projects ORDER BY COALESCE(last_activity_at, created_at) DESC")?;
        let ids: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|id| self.project_summary(id)).collect()
    }

    pub fn project_summary(&self, id: &str) -> Result<ProjectSummary> {
        let project = self.get_project(id)?;
        let (memory_count, open_tasks, open_bugs, needs_review): (i64, i64, i64, i64) = self.conn.query_row(
            "SELECT
                COUNT(*) FILTER (WHERE freshness != 'superseded'),
                COUNT(*) FILTER (WHERE kind = 'task' AND status IN ('pending', 'in_progress') AND freshness != 'superseded'),
                COUNT(*) FILTER (WHERE kind = 'bug' AND status = 'open' AND freshness != 'superseded'),
                COUNT(*) FILTER (WHERE freshness IN ('potentially_stale', 'needs_review'))
             FROM memories WHERE project_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let session_count: i64 = self.conn.query_row("SELECT COUNT(*) FROM sessions WHERE project_id = ?1", [id], |r| r.get(0))?;
        let mut stmt = self
            .conn
            .prepare_cached("SELECT agent FROM sessions WHERE project_id = ?1 GROUP BY agent ORDER BY MAX(last_activity_at) DESC")?;
        let agents = stmt.query_map([id], |r| r.get::<_, AgentKind>(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ProjectSummary { project, memory_count, open_tasks, open_bugs, needs_review, session_count, agents })
    }

    pub fn update_project(&mut self, id: &str, name: Option<&str>, settings: Option<&ProjectSettings>) -> Result<Project> {
        let current = self.get_project(id)?;
        let new_name = match name {
            Some(n) if n.trim().is_empty() => return Err(Error::invalid("project name cannot be empty")),
            Some(n) if n.trim().chars().count() > 120 => return Err(Error::invalid("project name must be at most 120 characters")),
            Some(n) => n.trim().to_string(),
            None => current.name.clone(),
        };
        let new_settings = settings.cloned().unwrap_or(current.settings.clone());
        for g in &new_settings.excluded_globs {
            crate::privacy::validate_glob(g)?;
        }
        if let Some(b) = new_settings.startup_budget_tokens
            && !(200..=8000).contains(&b)
        {
            return Err(Error::invalid("project startup budget must be between 200 and 8000 tokens"));
        }
        let json = serde_json::to_string(&new_settings)?;
        self.write_tx(|tx| {
            tx.execute(
                "UPDATE projects SET name = ?2, settings_json = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, new_name, json, util::now_ts()],
            )?;
            Ok(())
        })?;
        self.get_project(id)
    }

    /// Remove a project and all ContextForge data about it. Project files on disk are never touched.
    pub fn remove_project(&mut self, id: &str) -> Result<()> {
        self.get_project(id)?;
        self.write_tx(|tx| {
            // Break self-references first so cascading deletes do not trip over superseded_by.
            tx.execute("UPDATE memories SET superseded_by = NULL WHERE project_id = ?1", [id])?;
            tx.execute("DELETE FROM projects WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    pub(crate) fn touch_project(tx: &rusqlite::Transaction<'_>, project_id: &str, at: &str) -> Result<()> {
        tx.execute(
            "UPDATE projects SET last_activity_at = ?2 WHERE id = ?1 AND (last_activity_at IS NULL OR last_activity_at < ?2)",
            params![project_id, at],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;

    #[test]
    fn register_plain_directory_and_resolve_subdirectories() {
        let data = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let proj_dir = work.path().join("demo");
        std::fs::create_dir_all(proj_dir.join("src/deep")).unwrap();
        let mut store = Store::open_in(data.path(), OpenOptions::interactive()).unwrap();
        let (p, created) = store.register_project(&proj_dir, Some("Demo")).unwrap();
        assert!(created);
        assert_eq!(p.name, "Demo");
        let (again, created) = store.register_project(&proj_dir, None).unwrap();
        assert!(!created);
        assert_eq!(again.id, p.id);
        let r = store.resolve_project(&proj_dir.join("src/deep"), false).unwrap().unwrap();
        assert_eq!(r.project.id, p.id);
        assert!(store.resolve_project(work.path(), false).unwrap().is_none());
        assert_eq!(store.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn refuses_missing_and_overly_broad_paths() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open_in(data.path(), OpenOptions::interactive()).unwrap();
        assert!(store.register_project(Path::new("Z:/definitely/missing/dir"), None).is_err());
        if let Some(home) = dirs::home_dir() {
            assert!(store.register_project(&home, None).is_err());
        }
    }

    #[test]
    fn remove_project_deletes_only_contextforge_data() {
        let data = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("keep.txt"), "hello").unwrap();
        let mut store = Store::open_in(data.path(), OpenOptions::interactive()).unwrap();
        let (p, _) = store.register_project(work.path(), None).unwrap();
        store.remove_project(&p.id).unwrap();
        assert!(store.get_project(&p.id).is_err());
        assert!(work.path().join("keep.txt").exists());
    }
}
