//! Detect installed agents and their versions.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use contextforge_core::process;
use serde::{Deserialize, Serialize};

use crate::{Env, Provider};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentInstall {
    /// `claude_code`, `codex`, `antigravity_cli`, `antigravity_desktop`.
    pub id: String,
    pub label: String,
    pub provider: Provider,
    pub installed: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub detail: Option<String>,
}

fn first_existing(cands: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    cands.into_iter().find(|p| p.is_file())
}

pub fn find_claude_cli(home: &Path) -> Option<PathBuf> {
    process::which("claude").or_else(|| {
        first_existing([
            home.join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" }),
            dirs::data_dir().map(|d| d.join("npm").join("claude.cmd")).unwrap_or_default(),
        ])
    })
}

pub fn find_codex_cli() -> Option<PathBuf> {
    process::which("codex").or_else(|| {
        first_existing([dirs::data_local_dir()
            .map(|d| d.join("Programs").join("OpenAI").join("Codex").join("bin").join("codex.exe"))
            .unwrap_or_default()])
    })
}

pub fn find_agy_cli() -> Option<PathBuf> {
    process::which("agy")
        .or_else(|| first_existing([dirs::data_local_dir().map(|d| d.join("agy").join("bin").join("agy.exe")).unwrap_or_default()]))
}

/// The Antigravity desktop application (App/IDE), Windows per-user install location.
pub fn find_antigravity_desktop() -> Option<PathBuf> {
    first_existing([dirs::data_local_dir().map(|d| d.join("Programs").join("Antigravity").join("Antigravity.exe")).unwrap_or_default()])
}

/// Run `<exe> --version` (or similar) and extract the first version-like token.
pub fn version_of(exe: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = if exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(exe);
        c
    } else {
        Command::new(exe)
    };
    cmd.args(args);
    let out = process::run(cmd, None, Duration::from_secs(15), 64 * 1024).ok()?;
    let text = format!("{}\n{}", out.stdout_str(), out.stderr_str());
    extract_version(&text)
}

pub fn extract_version(text: &str) -> Option<String> {
    for token in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let t = token.trim_start_matches('v');
        let parts: Vec<&str> = t.split('.').collect();
        if parts.len() >= 2 && parts.iter().take(3).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            return Some(t.to_string());
        }
    }
    None
}

fn desktop_version(exe: &Path) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let script = format!("(Get-Item -LiteralPath '{}').VersionInfo.ProductVersion", exe.to_string_lossy().replace('\'', "''"));
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    let out = process::run(cmd, None, Duration::from_secs(15), 4096).ok()?;
    extract_version(&out.stdout_str()).map(|v| v.trim_end_matches(".0").to_string())
}

/// Detect every supported agent. Version probes run in parallel (each has a timeout).
pub fn detect_all(env: &Env) -> Vec<AgentInstall> {
    let claude = env.claude_cli.clone();
    let codex = env.codex_cli.clone();
    let agy = env.agy_cli.clone();
    let desktop = find_antigravity_desktop();
    let h1 = std::thread::spawn(move || claude.as_ref().and_then(|p| version_of(p, &["--version"])));
    let h2 = std::thread::spawn(move || codex.as_ref().and_then(|p| version_of(p, &["--version"])));
    let h3 = std::thread::spawn(move || agy.as_ref().and_then(|p| version_of(p, &["--version"])));
    let d2 = desktop.clone();
    let h4 = std::thread::spawn(move || d2.as_ref().and_then(|p| desktop_version(p)));
    let (v1, v2, v3, v4) = (h1.join().ok().flatten(), h2.join().ok().flatten(), h3.join().ok().flatten(), h4.join().ok().flatten());
    let s = |p: &Option<PathBuf>| p.as_ref().map(|x| x.to_string_lossy().into_owned());
    let ide_data = env.home.join(".gemini").join("antigravity-ide").is_dir();
    vec![
        AgentInstall {
            id: "claude_code".into(),
            label: "Claude Code".into(),
            provider: Provider::Claude,
            installed: env.claude_cli.is_some(),
            executable: s(&env.claude_cli),
            version: v1,
            detail: None,
        },
        AgentInstall {
            id: "codex".into(),
            label: "OpenAI Codex (CLI and desktop app)".into(),
            provider: Provider::Codex,
            installed: env.codex_cli.is_some(),
            executable: s(&env.codex_cli),
            version: v2,
            detail: Some("The Codex desktop app and CLI share ~/.codex".into()),
        },
        AgentInstall {
            id: "antigravity_cli".into(),
            label: "Antigravity CLI (agy)".into(),
            provider: Provider::Antigravity,
            installed: env.agy_cli.is_some(),
            executable: s(&env.agy_cli),
            version: v3,
            detail: None,
        },
        AgentInstall {
            id: "antigravity_desktop".into(),
            label: "Antigravity App / IDE".into(),
            provider: Provider::Antigravity,
            installed: desktop.is_some(),
            executable: s(&desktop),
            version: v4,
            detail: Some(if ide_data {
                "Shares ~/.gemini/config with the CLI; IDE data folder found".into()
            } else {
                "Shares ~/.gemini/config with the CLI".into()
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_versions_from_cli_banners() {
        assert_eq!(extract_version("2.1.306 (Claude Code)").as_deref(), Some("2.1.306"));
        assert_eq!(extract_version("codex-cli 0.167.0").as_deref(), Some("0.167.0"));
        assert_eq!(extract_version("1.5.5\n").as_deref(), Some("1.5.5"));
        assert_eq!(extract_version("version v2.31.0.0").as_deref(), Some("2.31.0.0"));
        assert_eq!(extract_version("no version here"), None);
    }
}
