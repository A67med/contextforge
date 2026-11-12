#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use contextforge_core::memories::{EvidenceInput, FileRefInput, NewMemory};
use contextforge_core::model::{Actor, AgentKind, EvidenceKind, MemoryKind, Origin};
use contextforge_core::{OpenOptions, Store};
use tempfile::TempDir;

pub fn store() -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in(dir.path(), OpenOptions::interactive()).unwrap();
    (dir, store)
}

pub fn reopen(dir: &TempDir) -> Store {
    Store::open_in(dir.path(), OpenOptions::interactive()).unwrap()
}

pub fn claude() -> Actor {
    Actor::agent(AgentKind::ClaudeCode, Origin::Mcp, None)
}
pub fn codex() -> Actor {
    Actor::agent(AgentKind::Codex, Origin::Mcp, None)
}
pub fn antigravity() -> Actor {
    Actor::agent(AgentKind::AntigravityCli, Origin::Mcp, None)
}

/// A throwaway Git repository with a local identity (never touches global git config).
pub struct TestRepo {
    pub dir: TempDir,
}

impl TestRepo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = TestRepo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "ContextForge Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo.write("README.md", "# Demo\n");
        repo.write("src/db.rs", &numbered_lines("db", 40));
        repo.write("src/api.rs", &numbered_lines("api", 40));
        repo.commit_all("initial commit");
        repo
    }

    pub fn path(&self) -> PathBuf {
        contextforge_core::paths::canonical(self.dir.path())
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn remove(&self, rel: &str) {
        std::fs::remove_file(self.dir.path().join(rel)).unwrap();
    }

    pub fn git(&self, args: &[&str]) -> String {
        git_in(self.dir.path(), args)
    }

    pub fn commit_all(&self, msg: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.head()
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }
}

pub fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").args(args).output().expect("git runs");
    assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn numbered_lines(prefix: &str, n: usize) -> String {
    (1..=n).map(|i| format!("// {prefix} line {i}\n")).collect()
}

pub fn fact(project_id: &str, title: &str, content: &str, files: &[&str]) -> NewMemory {
    let mut m = NewMemory::new(project_id, MemoryKind::Fact, title, content);
    m.files = files.iter().map(|f| FileRefInput::path(*f)).collect();
    m
}

pub fn command_evidence(cmd: &str) -> EvidenceInput {
    EvidenceInput { kind: EvidenceKind::Command, reference: cmd.into(), excerpt: Some("exit 0".into()) }
}
