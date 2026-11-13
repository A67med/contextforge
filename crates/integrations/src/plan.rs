//! Installation plans: previewable file changes and external commands, applied with backups, atomic writes
//! and rollback. Backups can later be restored exactly.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use contextforge_core::{db::write_atomic, process, util};
use serde::{Deserialize, Serialize};

use crate::{Env, IntegrationError, Provider, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Uninstall,
    Repair,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Uninstall => "uninstall",
            Action::Repair => "repair",
        }
    }
}

/// Where an integration is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum Scope {
    /// User-level (global) configuration.
    User,
    /// Project-level configuration inside one project directory.
    Project(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileChange {
    pub path: PathBuf,
    /// Content expected on disk right now (None = file does not exist).
    pub before: Option<String>,
    /// Content to write (None = delete the file).
    pub after: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub summary: String,
    /// Arguments (same program) that undo this command, used for rollback.
    pub undo_args: Option<Vec<String>>,
}

impl ExternalCommand {
    pub fn display(&self) -> String {
        let mut parts = vec![quote_arg(&self.program.to_string_lossy())];
        parts.extend(self.args.iter().map(|a| quote_arg(a)));
        parts.join(" ")
    }
}

fn quote_arg(a: &str) -> String {
    if a.is_empty() || a.contains(' ') || a.contains('"') { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.to_string() }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallPlan {
    pub provider: Provider,
    pub action: Action,
    pub scope: Scope,
    pub changes: Vec<FileChange>,
    pub commands: Vec<ExternalCommand>,
    /// What the user must know or do afterwards (e.g. Codex trust review).
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

impl InstallPlan {
    pub fn new(provider: Provider, action: Action, scope: Scope) -> Self {
        InstallPlan { provider, action, scope, changes: Vec::new(), commands: Vec::new(), notes: Vec::new(), warnings: Vec::new() }
    }

    pub fn is_noop(&self) -> bool {
        self.changes.is_empty() && self.commands.is_empty()
    }

    /// Unified-style preview of every change (shown in the UI and CLI before applying).
    pub fn preview(&self) -> String {
        let mut out = String::new();
        for c in &self.changes {
            out.push_str(&format!("=== {} — {}\n", c.path.display(), c.summary));
            out.push_str(&line_diff(c.before.as_deref().unwrap_or(""), c.after.as_deref().unwrap_or("")));
            out.push('\n');
        }
        for cmd in &self.commands {
            out.push_str(&format!("=== run: {} — {}\n", cmd.display(), cmd.summary));
        }
        out
    }
}

/// Minimal line diff (LCS) for previews.
pub fn line_diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a.len() * b.len() > 4_000_000 {
        return format!("- ({} lines)\n+ ({} lines)\n", a.len(), b.len());
    }
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = String::new();
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push_str(&format!("  {}\n", a[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        }
    }
    for l in &a[i..] {
        out.push_str(&format!("- {l}\n"));
    }
    for l in &b[j..] {
        out.push_str(&format!("+ {l}\n"));
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupFileRecord {
    pub original: PathBuf,
    /// File name inside the backup directory (None if the file did not exist before).
    pub stored: Option<String>,
    pub sha_before: Option<String>,
    pub sha_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupRecord {
    pub id: String,
    pub provider: Provider,
    pub action: Action,
    pub scope: Scope,
    pub created_at: String,
    pub files: Vec<BackupFileRecord>,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApplyReport {
    pub backup_id: Option<String>,
    pub files_written: Vec<PathBuf>,
    pub commands_run: Vec<String>,
    pub notes: Vec<String>,
}

fn sha(s: Option<&str>) -> Option<String> {
    s.map(|c| util::sha256_hex(c.as_bytes()))
}

pub(crate) fn read_opt(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| IntegrationError::Config(format!("{} is not valid UTF-8", path.display())))?;
            Ok(Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn run_command(program: &Path, args: &[String], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let out = process::run(cmd, None, Duration::from_secs(90), 1 << 20).map_err(|e| IntegrationError::Command(e.to_string()))?;
    if !out.success() {
        return Err(IntegrationError::Command(format!(
            "`{} {}` failed: {}{}",
            program.display(),
            args.join(" "),
            out.stderr_str().trim(),
            out.stdout_str().trim()
        )));
    }
    Ok(out.stdout_str())
}

/// Apply a plan: verify nothing changed since planning, back up, write atomically, run commands,
/// and roll everything back if any step fails.
pub fn apply(plan: &InstallPlan, env: &Env) -> Result<ApplyReport> {
    if plan.is_noop() {
        return Ok(ApplyReport { backup_id: None, files_written: vec![], commands_run: vec![], notes: plan.notes.clone() });
    }
    for c in &plan.changes {
        let current = read_opt(&c.path)?;
        if current != c.before {
            return Err(IntegrationError::Conflict(format!("{} changed since the plan was made; review the plan again", c.path.display())));
        }
    }

    let created_at = util::now_ts();
    let id = format!("{}-{}-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"), plan.provider.as_str(), plan.action.as_str());
    let dir = env.backups_dir.join(&id);
    std::fs::create_dir_all(&dir)?;
    let mut records = Vec::new();
    for (i, c) in plan.changes.iter().enumerate() {
        let stored = match &c.before {
            Some(content) => {
                let name =
                    format!("{i:02}-{}", c.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()));
                write_atomic(&dir.join(&name), content.as_bytes()).map_err(|e| IntegrationError::Config(e.to_string()))?;
                Some(name)
            }
            None => None,
        };
        records.push(BackupFileRecord {
            original: c.path.clone(),
            stored,
            sha_before: sha(c.before.as_deref()),
            sha_after: sha(c.after.as_deref()),
        });
    }
    let record = BackupRecord {
        id: id.clone(),
        provider: plan.provider,
        action: plan.action,
        scope: plan.scope.clone(),
        created_at,
        files: records,
        commands: plan.commands.iter().map(|c| c.display()).collect(),
    };
    write_atomic(&dir.join("manifest.json"), serde_json::to_string_pretty(&record)?.as_bytes())
        .map_err(|e| IntegrationError::Config(e.to_string()))?;

    let mut written: Vec<&FileChange> = Vec::new();
    let rollback_files = |written: &[&FileChange]| {
        for c in written.iter().rev() {
            let r = match &c.before {
                Some(content) => write_atomic(&c.path, content.as_bytes()).map_err(|e| e.to_string()),
                None => std::fs::remove_file(&c.path).map_err(|e| e.to_string()),
            };
            if let Err(e) = r {
                tracing::error!(path = %c.path.display(), error = %e, "rollback failed");
            }
        }
    };
    for c in &plan.changes {
        let result = match &c.after {
            Some(content) => write_atomic(&c.path, content.as_bytes()).map_err(|e| IntegrationError::Config(e.to_string())),
            None => match std::fs::remove_file(&c.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            },
        };
        if let Err(e) = result {
            rollback_files(&written);
            return Err(e);
        }
        written.push(c);
    }

    let mut ran: Vec<&ExternalCommand> = Vec::new();
    for cmd in &plan.commands {
        if let Err(e) = run_command(&cmd.program, &cmd.args, cmd.cwd.as_deref()) {
            for done in ran.iter().rev() {
                if let Some(undo) = &done.undo_args
                    && let Err(err) = run_command(&done.program, undo, done.cwd.as_deref())
                {
                    tracing::error!(error = %err, "command rollback failed");
                }
            }
            rollback_files(&written);
            return Err(e);
        }
        ran.push(cmd);
    }
    Ok(ApplyReport {
        backup_id: Some(id),
        files_written: plan.changes.iter().map(|c| c.path.clone()).collect(),
        commands_run: ran.iter().map(|c| c.display()).collect(),
        notes: plan.notes.clone(),
    })
}

pub fn list_backups(env: &Env) -> Vec<BackupRecord> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&env.backups_dir) {
        for e in entries.flatten() {
            let manifest = e.path().join("manifest.json");
            if let Ok(text) = std::fs::read_to_string(&manifest)
                && let Ok(r) = serde_json::from_str::<BackupRecord>(&text)
            {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestoreReport {
    pub restored: Vec<PathBuf>,
    pub notes: Vec<String>,
}

/// Restore every file of a backup to its pre-change content. Refuses (unless `force`) when a file was
/// modified after ContextForge wrote it, so later user edits are not silently lost.
pub fn restore_backup(env: &Env, id: &str, force: bool) -> Result<RestoreReport> {
    if id.contains(['/', '\\']) || id.contains("..") {
        return Err(IntegrationError::Config("invalid backup id".into()));
    }
    let dir = env.backups_dir.join(id);
    let text =
        std::fs::read_to_string(dir.join("manifest.json")).map_err(|_| IntegrationError::Config(format!("backup '{id}' not found")))?;
    let record: BackupRecord = serde_json::from_str(&text)?;
    let mut modified = Vec::new();
    for f in &record.files {
        let current = read_opt(&f.original)?;
        if sha(current.as_deref()) != f.sha_after {
            modified.push(f.original.display().to_string());
        }
    }
    if !modified.is_empty() && !force {
        return Err(IntegrationError::Conflict(format!(
            "these files changed after ContextForge modified them: {}. Restore anyway to discard those changes.",
            modified.join(", ")
        )));
    }
    let mut restored = Vec::new();
    for f in &record.files {
        match &f.stored {
            Some(name) => {
                let content = std::fs::read(dir.join(name))?;
                write_atomic(&f.original, &content).map_err(|e| IntegrationError::Config(e.to_string()))?;
            }
            None => {
                if f.original.exists() {
                    std::fs::remove_file(&f.original)?;
                }
            }
        }
        restored.push(f.original.clone());
    }
    let mut notes = Vec::new();
    if !record.commands.is_empty() {
        notes.push(format!(
            "This backup also ran external commands ({}); they are not reverted by a file restore — use Uninstall to remove the MCP registration.",
            record.commands.join("; ")
        ));
    }
    Ok(RestoreReport { restored, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_marks_additions_and_removals() {
        let d = line_diff("a\nb\nc", "a\nx\nc");
        assert!(d.contains("- b") && d.contains("+ x") && d.contains("  a"));
    }

    #[test]
    fn apply_backs_up_and_restore_reverts() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let target = root.path().join("cfg.json");
        std::fs::write(&target, "{\"user\":true}").unwrap();
        let mut plan = InstallPlan::new(Provider::Antigravity, Action::Install, Scope::User);
        plan.changes.push(FileChange {
            path: target.clone(),
            before: Some("{\"user\":true}".into()),
            after: Some("{\"user\":true,\"cf\":1}".into()),
            summary: "add".into(),
        });
        let new_file = root.path().join("new.json");
        plan.changes.push(FileChange { path: new_file.clone(), before: None, after: Some("{}".into()), summary: "create".into() });
        let report = apply(&plan, &env).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"user\":true,\"cf\":1}");
        let id = report.backup_id.unwrap();
        assert_eq!(list_backups(&env).len(), 1);
        // Re-applying the same plan is refused: the files no longer match `before`.
        assert!(matches!(apply(&plan, &env), Err(IntegrationError::Conflict(_))));
        restore_backup(&env, &id, false).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"user\":true}");
        assert!(!new_file.exists());
    }

    #[test]
    fn restore_refuses_when_user_edited_afterwards() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let target = root.path().join("cfg.json");
        let mut plan = InstallPlan::new(Provider::Codex, Action::Install, Scope::User);
        plan.changes.push(FileChange { path: target.clone(), before: None, after: Some("ours".into()), summary: "create".into() });
        let id = apply(&plan, &env).unwrap().backup_id.unwrap();
        std::fs::write(&target, "ours + user edit").unwrap();
        assert!(matches!(restore_backup(&env, &id, false), Err(IntegrationError::Conflict(_))));
        restore_backup(&env, &id, true).unwrap();
        assert!(!target.exists());
        assert!(restore_backup(&env, "../escape", true).is_err());
    }

    #[test]
    fn failed_write_rolls_back_earlier_files() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let ok = root.path().join("ok.json");
        std::fs::write(&ok, "original").unwrap();
        // A path whose parent is a file cannot be written.
        let blocker = root.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let bad = blocker.join("child.json");
        let mut plan = InstallPlan::new(Provider::Claude, Action::Install, Scope::User);
        plan.changes.push(FileChange {
            path: ok.clone(),
            before: Some("original".into()),
            after: Some("modified".into()),
            summary: "".into(),
        });
        plan.changes.push(FileChange { path: bad, before: None, after: Some("y".into()), summary: "".into() });
        assert!(apply(&plan, &env).is_err());
        assert_eq!(std::fs::read_to_string(&ok).unwrap(), "original", "first file restored after failure");
    }
}
