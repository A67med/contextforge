#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const BIN: &str = env!("CARGO_BIN_EXE_contextforge");

/// Isolated data directory + Git repository for one test.
pub struct Sandbox {
    pub data: tempfile::TempDir,
    pub repo_dir: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        let s = Sandbox { data: tempfile::tempdir().unwrap(), repo_dir: tempfile::tempdir().unwrap() };
        s.git(&["init", "-q", "-b", "main"]);
        s.git(&["config", "user.name", "ContextForge Test"]);
        s.git(&["config", "user.email", "test@example.invalid"]);
        s.git(&["config", "commit.gpgsign", "false"]);
        s.git(&["config", "core.autocrlf", "false"]);
        s.write("app/server.py", &(1..=30).map(|i| format!("# server line {i}\n")).collect::<String>());
        s.write("README.md", "# Shop\n");
        s.git(&["add", "-A"]);
        s.git(&["commit", "-q", "-m", "initial"]);
        s
    }

    pub fn repo(&self) -> PathBuf {
        dunce::canonicalize(self.repo_dir.path()).unwrap()
    }

    pub fn repo_str(&self) -> String {
        self.repo().to_string_lossy().into_owned()
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.repo_dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(self.repo_dir.path()).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn command(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("CONTEXTFORGE_HOME", self.data.path()).env_remove("CONTEXTFORGE_LOG");
        c
    }

    /// Run a CLI command; returns stdout. Panics on failure.
    pub fn cf(&self, args: &[&str]) -> String {
        let out = self.command().current_dir(self.repo()).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "contextforge {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Run a hook exactly as an agent would: JSON on stdin, output on stdout, must exit 0.
    pub fn hook(&self, provider: &str, event: &str, payload: &serde_json::Value) -> String {
        self.hook_raw(provider, event, payload.to_string().as_bytes())
    }

    pub fn hook_raw(&self, provider: &str, event: &str, stdin: &[u8]) -> String {
        let mut child = self
            .command()
            .current_dir(std::env::temp_dir())
            .args(["hook", provider, event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook must always exit 0: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn data_path(&self) -> &Path {
        self.data.path()
    }
}
