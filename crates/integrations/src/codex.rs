//! OpenAI Codex integration.
//!
//! * Hooks: `~/.codex/hooks.json` (or `<repo>/.codex/hooks.json`; project hooks load only in trusted projects).
//!   On Windows Codex runs hook commands through `%COMSPEC% /C "<command>"`, so `command`/`commandWindows`
//!   is a quoted command line.
//! * MCP: `[mcp_servers.contextforge]` in `config.toml`, edited with a format-preserving TOML editor.
//! * Trust: Codex runs non-managed hooks only after the user reviews and trusts them (`/hooks` in the CLI,
//!   Settings → Hooks in the desktop app). ContextForge never writes trust state; it only *reads*
//!   `[hooks.state."<file>:<event>:<group>:<handler>"].trusted_hash` to show whether review is pending.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::jsoncfg::{array_entry, change_if_different, object_entry, read_object};
use crate::plan::{Action, FileChange, InstallPlan, Scope, read_opt};
use crate::status::{ComponentState, ComponentStatus};
use crate::{Env, IntegrationError, Provider, Result, is_contextforge_command, shell_command};

pub const MCP_NAME: &str = "contextforge";
const FILE_DESCRIPTION: &str = "Codex lifecycle hooks. ContextForge manages only its own entries; other hooks are preserved.";

pub struct HookSpec {
    pub event: &'static str,
    pub label: &'static str,
    pub matcher: Option<&'static str>,
    pub timeout: u32,
    pub arg: &'static str,
    pub status_message: Option<&'static str>,
    pub injects_context: bool,
}

pub const HOOKS: &[HookSpec] = &[
    HookSpec {
        event: "SessionStart",
        label: "session_start",
        matcher: Some("startup|resume|clear|compact"),
        timeout: 30,
        arg: "session-start",
        status_message: Some("ContextForge: loading project memory"),
        injects_context: true,
    },
    HookSpec {
        event: "UserPromptSubmit",
        label: "user_prompt_submit",
        matcher: None,
        timeout: 15,
        arg: "user-prompt-submit",
        status_message: None,
        injects_context: true,
    },
    HookSpec {
        event: "PostToolUse",
        label: "post_tool_use",
        matcher: Some("Bash|apply_patch"),
        timeout: 15,
        arg: "post-tool-use",
        status_message: None,
        injects_context: false,
    },
    HookSpec { event: "Stop", label: "stop", matcher: None, timeout: 15, arg: "stop", status_message: None, injects_context: false },
    // SessionEnd hooks are limited to 3 seconds by Codex.
    HookSpec {
        event: "SessionEnd",
        label: "session_end",
        matcher: None,
        timeout: 3,
        arg: "session-end",
        status_message: None,
        injects_context: false,
    },
];

pub fn hooks_path(env: &Env, scope: &Scope) -> PathBuf {
    match scope {
        Scope::User => env.codex_home.join("hooks.json"),
        Scope::Project(p) => p.join(".codex").join("hooks.json"),
    }
}

pub fn config_path(env: &Env, scope: &Scope) -> PathBuf {
    match scope {
        Scope::User => env.codex_home.join("config.toml"),
        Scope::Project(p) => p.join(".codex").join("config.toml"),
    }
}

pub fn handler(exe: &Path, spec: &HookSpec) -> Value {
    let cmd = shell_command(exe, Provider::Codex, spec.arg);
    let mut h = json!({ "type": "command", "command": cmd, "commandWindows": cmd, "timeout": spec.timeout });
    if let Some(m) = spec.status_message {
        h["statusMessage"] = json!(m);
    }
    if spec.injects_context {
        // ContextForge already budgets its context (default ≈1,200 tokens); avoid Codex spilling it to disk
        // when users configure a larger budget.
        h["additionalContextLimit"] = json!(8500);
    }
    h
}

fn is_ours(h: &Value) -> bool {
    let cmd = h.get("command").and_then(Value::as_str).unwrap_or("");
    let win = h.get("commandWindows").and_then(Value::as_str).unwrap_or("");
    (is_contextforge_command(cmd) && cmd.contains(" hook codex ")) || (is_contextforge_command(win) && win.contains(" hook codex "))
}

fn strip_ours(hooks: &mut Map<String, Value>) {
    let events: Vec<String> = hooks.keys().cloned().collect();
    for ev in events {
        let Some(Value::Array(groups)) = hooks.get_mut(&ev) else { continue };
        for g in groups.iter_mut() {
            if let Some(Value::Array(handlers)) = g.get_mut("hooks") {
                handlers.retain(|h| !is_ours(h));
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|a| !a.is_empty()));
        if groups.is_empty() {
            hooks.remove(&ev);
        }
    }
}

/// Positions of our handlers: (spec label, group index, handler index, command).
fn our_positions(hooks: &Map<String, Value>) -> Vec<(String, usize, usize, String)> {
    let mut out = Vec::new();
    for spec in HOOKS {
        if let Some(groups) = hooks.get(spec.event).and_then(Value::as_array) {
            for (gi, g) in groups.iter().enumerate() {
                for (hi, h) in g.get("hooks").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    if is_ours(h) {
                        let cmd = h.get("commandWindows").or_else(|| h.get("command")).and_then(Value::as_str).unwrap_or("").to_string();
                        out.push((spec.label.to_string(), gi, hi, cmd));
                    }
                }
            }
        }
    }
    out
}

fn read_toml(path: &Path) -> Result<(Option<String>, DocumentMut)> {
    let raw = read_opt(path)?;
    let text = raw.as_deref().unwrap_or("").trim_start_matches('\u{feff}');
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|e| IntegrationError::Toml(format!("{} is not valid TOML ({e}); ContextForge will not modify it", path.display())))?;
    Ok((raw, doc))
}

fn inline_hook_definitions(doc: &DocumentMut) -> bool {
    doc.get("hooks").and_then(Item::as_table_like).is_some_and(|t| HOOKS.iter().any(|s| t.contains_key(s.event)))
}

fn mcp_entry_command(doc: &DocumentMut) -> Option<String> {
    doc.get("mcp_servers")?.as_table_like()?.get(MCP_NAME)?.as_table_like()?.get("command")?.as_str().map(str::to_string)
}

pub fn plan(action: Action, scope: &Scope, exe: &Path, env: &Env) -> Result<InstallPlan> {
    let mut plan = InstallPlan::new(Provider::Codex, action, scope.clone());

    // ---- hooks.json ----
    let hpath = hooks_path(env, scope);
    let (before, mut root) = read_object(&hpath)?;
    let created = before.is_none() || root.is_empty();
    if created && action != Action::Uninstall {
        root.insert("description".into(), Value::String(FILE_DESCRIPTION.into()));
    }
    {
        let hooks = object_entry(&mut root, "hooks", &hpath)?;
        strip_ours(hooks);
        if action != Action::Uninstall {
            for spec in HOOKS {
                let mut group = Map::new();
                if let Some(m) = spec.matcher {
                    group.insert("matcher".into(), Value::String(m.into()));
                }
                group.insert("hooks".into(), Value::Array(vec![handler(exe, spec)]));
                array_entry(hooks, spec.event, &hpath)?.push(Value::Object(group));
            }
        }
    }
    // Delete the file on uninstall only if ContextForge created it (our description) and nothing else is left.
    let only_ours_left = root.get("hooks").and_then(Value::as_object).is_some_and(|h| h.is_empty())
        && root.get("description").and_then(Value::as_str) == Some(FILE_DESCRIPTION)
        && root.keys().all(|k| k == "hooks" || k == "description");
    if action == Action::Uninstall && only_ours_left && before.is_some() {
        plan.changes.push(FileChange {
            path: hpath.clone(),
            before,
            after: None,
            summary: "remove hooks.json (it only contained ContextForge hooks)".into(),
        });
    } else if let Some(change) = change_if_different(
        &hpath,
        before,
        &root,
        if action == Action::Uninstall { "remove ContextForge hooks" } else { "add ContextForge lifecycle hooks" },
    ) {
        plan.changes.push(change);
    }

    // ---- config.toml MCP server ----
    let cpath = config_path(env, scope);
    let (cbefore, mut doc) = read_toml(&cpath)?;
    if inline_hook_definitions(&doc) {
        plan.warnings.push(format!("{} also defines hooks inline; Codex merges both forms and prints a startup warning", cpath.display()));
    }
    let existing = mcp_entry_command(&doc);
    let foreign = existing.as_deref().is_some_and(|c| !is_contextforge_command(c));
    if foreign {
        plan.warnings.push(format!(
            "an MCP server named '{MCP_NAME}' that is not ContextForge exists in {}; it was left untouched",
            cpath.display()
        ));
    } else {
        let servers_missing = doc.get("mcp_servers").is_none();
        if servers_missing && action != Action::Uninstall {
            let mut t = Table::new();
            t.set_implicit(true);
            doc.insert("mcp_servers", Item::Table(t));
        }
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) {
            match action {
                Action::Uninstall => {
                    servers.remove(MCP_NAME);
                }
                _ => {
                    let exe_s = exe.to_string_lossy().into_owned();
                    match servers.get_mut(MCP_NAME).and_then(Item::as_table_like_mut) {
                        // Update in place so the table keeps its position and any extra keys the user added.
                        Some(existing) => {
                            if existing.get("command").and_then(Item::as_str) != Some(exe_s.as_str()) {
                                existing.insert("command", value(exe_s));
                            }
                            let args_ok = existing
                                .get("args")
                                .and_then(Item::as_array)
                                .is_some_and(|a| a.len() == 1 && a.get(0).and_then(|v| v.as_str()) == Some("mcp"));
                            if !args_ok {
                                existing.insert("args", value(toml_edit::Array::from_iter(["mcp"])));
                            }
                        }
                        None => {
                            let mut t = Table::new();
                            t["command"] = value(exe_s);
                            t["args"] = value(toml_edit::Array::from_iter(["mcp"]));
                            t["startup_timeout_sec"] = value(20);
                            t["tool_timeout_sec"] = value(60);
                            servers.insert(MCP_NAME, Item::Table(t));
                        }
                    }
                }
            }
        } else if action != Action::Uninstall {
            return Err(IntegrationError::Config(format!("'mcp_servers' in {} is not a table", cpath.display())));
        }
    }
    let after = doc.to_string();
    if cbefore.as_deref().unwrap_or("") != after {
        plan.changes.push(FileChange {
            path: cpath,
            before: cbefore,
            after: Some(after),
            summary: if action == Action::Uninstall {
                "remove [mcp_servers.contextforge]".into()
            } else {
                "add [mcp_servers.contextforge]".into()
            },
        });
    }
    if action != Action::Uninstall {
        plan.notes.push(
            "Codex runs hooks only after you review and trust them: start Codex and open /hooks (CLI) or Settings → Hooks (desktop app), then trust the ContextForge hooks. ContextForge never marks hooks as trusted on your behalf.".into(),
        );
        if matches!(scope, Scope::Project(_)) {
            plan.notes.push("Project-level .codex/ configuration is only loaded when Codex trusts this project.".into());
        }
    }
    Ok(plan)
}

pub fn hooks_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    let path = hooks_path(env, scope);
    let (_, root) = match read_object(&path) {
        Ok(r) => r,
        Err(e) => return ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&path)),
    };
    let empty = Map::new();
    let hooks = root.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
    let found = our_positions(hooks);
    if found.is_empty() {
        return ComponentStatus::new(ComponentState::NotInstalled, "no ContextForge hooks configured", Some(&path));
    }
    let expected = shell_command(exe, Provider::Codex, "");
    if found.iter().any(|(_, _, _, cmd)| !cmd.to_lowercase().starts_with(expected.trim_end().to_lowercase().trim_end_matches(' '))) {
        return ComponentStatus::new(
            ComponentState::Outdated,
            "hooks point to a different ContextForge executable; run Repair",
            Some(&path),
        );
    }
    if found.len() >= HOOKS.len() {
        ComponentStatus::new(ComponentState::Installed, format!("{} lifecycle hooks configured", found.len()), Some(&path))
    } else {
        ComponentStatus::new(ComponentState::Partial, format!("{} of {} hooks configured", found.len(), HOOKS.len()), Some(&path))
    }
}

/// Read-only: has the user trusted the ContextForge hook definitions in Codex?
pub fn trust_status(env: &Env, scope: &Scope) -> ComponentStatus {
    let hpath = hooks_path(env, scope);
    let cpath = config_path(env, &Scope::User);
    let Ok((_, root)) = read_object(&hpath) else {
        return ComponentStatus::new(ComponentState::Error, "hooks.json unreadable", Some(&hpath));
    };
    let empty = Map::new();
    let positions = our_positions(root.get("hooks").and_then(Value::as_object).unwrap_or(&empty));
    if positions.is_empty() {
        return ComponentStatus::new(ComponentState::NotInstalled, "no ContextForge hooks to review", Some(&cpath));
    }
    let Ok((_, doc)) = read_toml(&cpath) else {
        return ComponentStatus::new(ComponentState::Error, "config.toml unreadable", Some(&cpath));
    };
    let source = hpath.display().to_string();
    let state = doc.get("hooks").and_then(|h| h.get("state")).and_then(Item::as_table_like);
    let trusted = positions
        .iter()
        .filter(|(label, gi, hi, _)| {
            let key = format!("{source}:{label}:{gi}:{hi}");
            state
                .and_then(|s| s.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key)).map(|(_, v)| v))
                .and_then(|v| v.as_table_like())
                .and_then(|t| t.get("trusted_hash"))
                .is_some()
        })
        .count();
    if trusted == positions.len() {
        ComponentStatus::new(
            ComponentState::Installed,
            format!("trust recorded for all {trusted} ContextForge hooks (Codex re-checks the definition hash at startup)"),
            Some(&cpath),
        )
    } else {
        ComponentStatus::new(
            ComponentState::PendingUserAction,
            format!(
                "{trusted} of {} hooks trusted — open /hooks in Codex (or Settings → Hooks in the Codex app) to review and trust them",
                positions.len()
            ),
            Some(&cpath),
        )
    }
}

pub fn mcp_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    let path = config_path(env, scope);
    match read_toml(&path) {
        Err(e) => ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&path)),
        Ok((_, doc)) => match mcp_entry_command(&doc) {
            None => ComponentStatus::new(ComponentState::NotInstalled, "MCP server not configured", Some(&path)),
            Some(cmd) if crate::claude::same_path(&cmd, exe) => {
                ComponentStatus::new(ComponentState::Installed, "MCP server configured", Some(&path))
            }
            Some(cmd) if is_contextforge_command(&cmd) => ComponentStatus::new(
                ComponentState::Outdated,
                "MCP server points to a different ContextForge executable; run Repair",
                Some(&path),
            ),
            Some(_) => {
                ComponentStatus::new(ComponentState::Error, format!("an unrelated MCP server named '{MCP_NAME}' exists"), Some(&path))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Users\Dev User\AppData\Local\ContextForge\contextforge.exe")
    }

    const USER_CONFIG: &str = r#"# my codex config
model = "gpt-5"

[features]
hooks = true

[mcp_servers.node_repl]
command = "node_repl.exe"

[projects.'E:\Work']
trust_level = "trusted"
"#;

    #[test]
    fn install_preserves_config_formatting_and_existing_servers() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        std::fs::create_dir_all(&env.codex_home).unwrap();
        std::fs::write(config_path(&env, &Scope::User), USER_CONFIG).unwrap();
        let plan = plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        assert_eq!(plan.changes.len(), 2);
        assert!(plan.notes.iter().any(|n| n.contains("/hooks")), "trust review is explained");
        crate::plan::apply(&plan, &env).unwrap();

        let cfg = std::fs::read_to_string(config_path(&env, &Scope::User)).unwrap();
        assert!(cfg.starts_with("# my codex config\nmodel = \"gpt-5\""), "comments and order preserved:\n{cfg}");
        assert!(cfg.contains("[mcp_servers.node_repl]"));
        assert!(cfg.contains("[mcp_servers.contextforge]"));
        let doc: DocumentMut = cfg.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["contextforge"]["args"].as_array().unwrap().get(0).unwrap().as_str(), Some("mcp"));

        let hooks: Value = serde_json::from_str(&std::fs::read_to_string(hooks_path(&env, &Scope::User)).unwrap()).unwrap();
        let start = &hooks["hooks"]["SessionStart"][0]["hooks"][0];
        assert_eq!(start["commandWindows"], format!("\"{}\" hook codex session-start", exe().display()));
        assert_eq!(hooks["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
        assert_eq!(hooks_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);
        assert_eq!(mcp_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);
        assert_eq!(trust_status(&env, &Scope::User).state, ComponentState::PendingUserAction);

        // Uninstall restores the config text exactly and removes the hooks file we created.
        let un = super::plan(Action::Uninstall, &Scope::User, &exe(), &env).unwrap();
        crate::plan::apply(&un, &env).unwrap();
        assert_eq!(std::fs::read_to_string(config_path(&env, &Scope::User)).unwrap(), USER_CONFIG);
        assert!(!hooks_path(&env, &Scope::User).exists());
    }

    #[test]
    fn detects_recorded_trust_without_writing_it() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let plan = plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        crate::plan::apply(&plan, &env).unwrap();
        let source = hooks_path(&env, &Scope::User).display().to_string();
        let mut cfg = std::fs::read_to_string(config_path(&env, &Scope::User)).unwrap();
        for label in ["session_start", "user_prompt_submit", "post_tool_use", "stop", "session_end"] {
            cfg.push_str(&format!("\n[hooks.state.'{source}:{label}:0:0']\ntrusted_hash = \"sha256:abc\"\n"));
        }
        std::fs::write(config_path(&env, &Scope::User), cfg).unwrap();
        assert_eq!(trust_status(&env, &Scope::User).state, ComponentState::Installed);
        // Re-planning an install with unchanged definitions does not touch the trust state.
        let again = super::plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes.iter().map(|c| &c.path).collect::<Vec<_>>());
    }

    #[test]
    fn keeps_user_hooks_in_shared_hooks_json() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        std::fs::create_dir_all(&env.codex_home).unwrap();
        let user = json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "python3 ~/.codex/hooks/stop.py"}]}]}});
        std::fs::write(hooks_path(&env, &Scope::User), user.to_string()).unwrap();
        crate::plan::apply(&plan(Action::Install, &Scope::User, &exe(), &env).unwrap(), &env).unwrap();
        crate::plan::apply(&plan(Action::Uninstall, &Scope::User, &exe(), &env).unwrap(), &env).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(hooks_path(&env, &Scope::User)).unwrap()).unwrap();
        assert_eq!(after, user);
    }
}
