//! ContextForge provider integrations.
//!
//! * [`adapters`] — parse each provider's hook payloads and render its output format.
//! * [`claude`], [`codex`], [`antigravity`] — plan/install/uninstall/status of MCP + hook configuration.
//! * [`plan`] — previewable plans applied with backups, atomic writes and rollback.
//! * [`detect`] — find installed agents and their versions.
//! * [`doctor`] — live validation of the MCP server and hook command.

pub mod adapters;
pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod detect;
pub mod doctor;
pub mod jsoncfg;
pub mod plan;
pub mod status;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use plan::{Action, ApplyReport, InstallPlan, Scope};
pub use status::{ComponentState, ComponentStatus, ProviderStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
    Antigravity,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Claude, Provider::Codex, Provider::Antigravity];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Antigravity => "antigravity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "OpenAI Codex",
            Provider::Antigravity => "Google Antigravity",
        }
    }
}

impl std::str::FromStr for Provider {
    type Err = IntegrationError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claude_code" | "cc" => Ok(Provider::Claude),
            "codex" | "openai-codex" => Ok(Provider::Codex),
            "antigravity" | "agy" | "antigravity-cli" | "antigravity-ide" | "antigravity-app" => Ok(Provider::Antigravity),
            other => Err(IntegrationError::Config(format!("unknown provider '{other}' (expected claude, codex or antigravity)"))),
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("command failed: {0}")]
    Command(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(String),
    #[error(transparent)]
    Core(#[from] contextforge_core::Error),
}

pub type Result<T, E = IntegrationError> = std::result::Result<T, E>;

/// Resolved locations of every provider's configuration. Tests point all of these into a temp dir.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Env {
    pub home: PathBuf,
    /// `CLAUDE_CONFIG_DIR` or `~/.claude` (holds settings.json).
    pub claude_config_dir: PathBuf,
    /// `~/.claude.json` (user/local MCP servers), or `$CLAUDE_CONFIG_DIR/.claude.json`.
    pub claude_json: PathBuf,
    /// `CODEX_HOME` or `~/.codex`.
    pub codex_home: PathBuf,
    /// `~/.gemini/config` — shared by Antigravity App, IDE and CLI.
    pub gemini_config_dir: PathBuf,
    pub claude_cli: Option<PathBuf>,
    pub codex_cli: Option<PathBuf>,
    pub agy_cli: Option<PathBuf>,
    /// Where ContextForge stores configuration backups.
    pub backups_dir: PathBuf,
}

impl Env {
    /// Resolve real locations for the current user.
    pub fn detect(data_dir: &Path) -> Env {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let claude_dir_env = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from);
        let claude_config_dir = claude_dir_env.clone().unwrap_or_else(|| home.join(".claude"));
        let claude_json = match &claude_dir_env {
            Some(d) => d.join(".claude.json"),
            None => home.join(".claude.json"),
        };
        let codex_home = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
        Env {
            gemini_config_dir: home.join(".gemini").join("config"),
            claude_cli: detect::find_claude_cli(&home),
            codex_cli: detect::find_codex_cli(),
            agy_cli: detect::find_agy_cli(),
            backups_dir: data_dir.join("integration-backups"),
            claude_config_dir,
            claude_json,
            codex_home,
            home,
        }
    }

    /// Fully sandboxed environment rooted at `root` (no external CLIs).
    pub fn for_test(root: &Path) -> Env {
        Env {
            home: root.to_path_buf(),
            claude_config_dir: root.join(".claude"),
            claude_json: root.join(".claude.json"),
            codex_home: root.join(".codex"),
            gemini_config_dir: root.join(".gemini").join("config"),
            claude_cli: None,
            codex_cli: None,
            agy_cli: None,
            backups_dir: root.join("cf-data").join("integration-backups"),
        }
    }
}

/// Build an install / uninstall / repair plan for one provider.
pub fn plan(provider: Provider, action: Action, scope: &Scope, exe: &Path, env: &Env) -> Result<InstallPlan> {
    match provider {
        Provider::Claude => claude::plan(action, scope, exe, env),
        Provider::Codex => codex::plan(action, scope, exe, env),
        Provider::Antigravity => antigravity::plan(action, scope, exe, env),
    }
}

/// Current integration status for one provider (reads configuration files only).
pub fn status(provider: Provider, scope: &Scope, exe: &Path, env: &Env, installs: &[detect::AgentInstall]) -> ProviderStatus {
    let (mcp, hooks, trust) = match provider {
        Provider::Claude => (claude::mcp_status(env, scope, exe), claude::hooks_status(env, scope, exe), None),
        Provider::Codex => {
            let hooks = codex::hooks_status(env, scope, exe);
            let trust = (hooks.state != ComponentState::NotInstalled).then(|| codex::trust_status(env, scope));
            (codex::mcp_status(env, scope, exe), hooks, trust)
        }
        Provider::Antigravity => (antigravity::mcp_status(env, scope, exe), antigravity::hooks_status(env, scope, exe), None),
    };
    let mut notes = Vec::new();
    if provider == Provider::Codex {
        notes.push("Codex reports shell-command output without exit codes, so command results from Codex are recorded as unknown.".into());
    }
    if provider == Provider::Antigravity {
        notes.push("One configuration in ~/.gemini/config serves the Antigravity App, IDE and CLI.".into());
    }
    ProviderStatus {
        provider,
        label: provider.label().into(),
        installations: installs.iter().filter(|i| i.provider == provider).cloned().collect(),
        mcp,
        hooks,
        trust,
        notes,
    }
}

/// How hook commands refer to the ContextForge executable.
pub(crate) fn exe_display(exe: &Path) -> String {
    exe.to_string_lossy().into_owned()
}

/// Shell-form command line (`"C:\path\contextforge.exe" hook codex session-start`), quoted when needed.
pub(crate) fn shell_command(exe: &Path, provider: Provider, event: &str) -> String {
    let p = exe_display(exe);
    let quoted = if p.contains([' ', '&', '(', ')', '^', '%', '!', '\'']) { format!("\"{p}\"") } else { p };
    format!("{quoted} hook {} {event}", provider.as_str())
}

/// Windows 8.3 short path (`C:\PROGRA~1\...`), if the volume has short names enabled.
#[cfg(windows)]
pub fn short_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: `wide` is NUL-terminated and `buf` has the advertised capacity.
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let p = PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
    (!p.to_string_lossy().contains(' ')).then_some(p)
}

#[cfg(not(windows))]
pub fn short_path(_path: &Path) -> Option<PathBuf> {
    None
}

/// Command line for shells that cannot run a quoted executable path (Antigravity runs hooks through cmd.exe
/// in a way that breaks a leading quoted path — verified live). Paths with spaces use the 8.3 short form.
/// Returns the command and whether it had to fall back to quoting (which Antigravity will not run).
pub(crate) fn unquoted_shell_command(exe: &Path, provider: Provider, event: &str) -> (String, bool) {
    let p = exe_display(exe);
    if !p.contains([' ', '&', '(', ')', '^', '%', '!', '\'']) {
        return (format!("{p} hook {} {event}", provider.as_str()), false);
    }
    match short_path(exe) {
        Some(s) => (format!("{} hook {} {event}", s.display(), provider.as_str()), false),
        None => (shell_command(exe, provider, event), true),
    }
}

/// Does a command string / path refer to a ContextForge executable?
pub(crate) fn is_contextforge_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase().replace('\\', "/");
    let exe_part = lower.trim_start_matches('"');
    let first = exe_part.split('"').next().unwrap_or(exe_part);
    let file = first.rsplit('/').next().unwrap_or(first);
    file.starts_with("contextforge")
        || lower.contains("/contextforge.exe")
        || lower.contains("/contextforge\"")
        || lower.starts_with("contextforge ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_commands_quote_paths_with_spaces() {
        let c = shell_command(Path::new(r"C:\Program Files\ContextForge\contextforge.exe"), Provider::Codex, "session-start");
        assert_eq!(c, r#""C:\Program Files\ContextForge\contextforge.exe" hook codex session-start"#);
        let c = shell_command(Path::new(r"C:\Tools\contextforge.exe"), Provider::Antigravity, "stop");
        assert_eq!(c, r"C:\Tools\contextforge.exe hook antigravity stop");
    }

    #[test]
    fn recognises_contextforge_commands() {
        assert!(is_contextforge_command(r#""C:\Program Files\ContextForge\contextforge.exe" hook codex stop"#));
        assert!(is_contextforge_command(r"C:\Users\me\AppData\Local\ContextForge\contextforge.exe"));
        assert!(is_contextforge_command("/usr/local/bin/contextforge hook claude stop"));
        assert!(!is_contextforge_command("python3 ~/.codex/hooks/session_start.py"));
        assert!(!is_contextforge_command("node lint.js"));
    }

    #[test]
    fn provider_parsing() {
        assert_eq!("Claude-Code".parse::<Provider>().unwrap(), Provider::Claude);
        assert_eq!("agy".parse::<Provider>().unwrap(), Provider::Antigravity);
        assert!("cursor".parse::<Provider>().is_err());
    }
}
