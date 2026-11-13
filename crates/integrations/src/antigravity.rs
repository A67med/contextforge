//! Google Antigravity integration — one configuration serves the App, the IDE and the CLI (`agy`).
//!
//! * Hooks: `~/.gemini/config/hooks.json` (global) or `<workspace>/.agents/hooks.json`. The file maps hook
//!   *names* to event configurations, so ContextForge owns exactly one key: `"contextforge"`.
//! * MCP: `~/.gemini/config/mcp_config.json` or `<workspace>/.agents/mcp_config.json` → `mcpServers.contextforge`.
//!   Antigravity lazy-loads MCP tools (invoked through `call_mcp_tool`), which keeps per-turn token use low.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::jsoncfg::{change_if_different, object_entry, read_object};
use crate::plan::{Action, FileChange, InstallPlan, Scope};
use crate::status::{ComponentState, ComponentStatus};
use crate::{Env, Provider, Result, exe_display, is_contextforge_command, shell_command};

pub const HOOK_NAME: &str = "contextforge";
pub const MCP_NAME: &str = "contextforge";
pub const TOOL_MATCHER: &str = "write_to_file|replace_file_content|multi_replace_file_content|run_command";

pub fn hooks_path(env: &Env, scope: &Scope) -> PathBuf {
    match scope {
        Scope::User => env.gemini_config_dir.join("hooks.json"),
        Scope::Project(p) => p.join(".agents").join("hooks.json"),
    }
}

pub fn mcp_path(env: &Env, scope: &Scope) -> PathBuf {
    match scope {
        Scope::User => env.gemini_config_dir.join("mcp_config.json"),
        Scope::Project(p) => p.join(".agents").join("mcp_config.json"),
    }
}

fn handler(exe: &Path, event: &str, timeout: u32) -> Value {
    json!({ "type": "command", "command": shell_command(exe, Provider::Antigravity, event), "timeout": timeout })
}

/// The `"contextforge"` hook definition. Tool events use matcher groups; the others list handlers directly
/// (as documented: "the matcher is ignored" for SessionStart, PreInvocation and Stop).
pub fn hook_definition(exe: &Path) -> Value {
    json!({
        "enabled": true,
        "SessionStart": [handler(exe, "session-start", 20)],
        "PreInvocation": [handler(exe, "pre-invocation", 10)],
        "PostToolUse": [{ "matcher": TOOL_MATCHER, "hooks": [handler(exe, "post-tool-use", 10)] }],
        "Stop": [handler(exe, "stop", 10)]
    })
}

pub fn mcp_definition(exe: &Path) -> Value {
    json!({ "command": exe_display(exe), "args": ["mcp"], "timeoutSeconds": 60 })
}

pub fn plan(action: Action, scope: &Scope, exe: &Path, env: &Env) -> Result<InstallPlan> {
    let mut plan = InstallPlan::new(Provider::Antigravity, action, scope.clone());

    let hpath = hooks_path(env, scope);
    let (before, mut root) = read_object(&hpath)?;
    if let Some(existing) = root.get(HOOK_NAME)
        && !definition_is_ours(existing)
    {
        plan.warnings.push(format!(
            "a hook named '{HOOK_NAME}' that does not run ContextForge exists in {}; it was left untouched",
            hpath.display()
        ));
    } else {
        match action {
            Action::Uninstall => {
                root.remove(HOOK_NAME);
            }
            _ => {
                // Keep the user's enabled/disabled choice when repairing.
                let enabled = root.get(HOOK_NAME).and_then(|h| h.get("enabled")).and_then(Value::as_bool).unwrap_or(true);
                let mut def = hook_definition(exe);
                def["enabled"] = Value::Bool(enabled);
                root.insert(HOOK_NAME.into(), def);
            }
        }
        if action == Action::Uninstall && root.is_empty() && before.is_some() {
            plan.changes.push(FileChange {
                path: hpath.clone(),
                before,
                after: None,
                summary: "remove hooks.json (it only contained the ContextForge hook)".into(),
            });
        } else if let Some(c) = change_if_different(
            &hpath,
            before,
            &root,
            if action == Action::Uninstall {
                "remove the 'contextforge' hook"
            } else {
                "add the 'contextforge' hook (SessionStart, PreInvocation, PostToolUse, Stop)"
            },
        ) {
            plan.changes.push(c);
        }
    }

    let mpath = mcp_path(env, scope);
    let (mbefore, mut mroot) = read_object(&mpath)?;
    let foreign = mroot
        .get("mcpServers")
        .and_then(|s| s.get(MCP_NAME))
        .is_some_and(|e| !is_contextforge_command(e.get("command").and_then(Value::as_str).unwrap_or("")));
    if foreign {
        plan.warnings.push(format!(
            "an MCP server named '{MCP_NAME}' that is not ContextForge exists in {}; it was left untouched",
            mpath.display()
        ));
    } else {
        match action {
            Action::Uninstall => {
                if let Some(Value::Object(servers)) = mroot.get_mut("mcpServers") {
                    servers.remove(MCP_NAME);
                }
            }
            _ => {
                let servers = object_entry(&mut mroot, "mcpServers", &mpath)?;
                let mut def = mcp_definition(exe);
                if let Some(Value::Object(old)) = servers.get(MCP_NAME) {
                    // Preserve user tweaks such as "disabled", "enabledTools" or per-tool "eager" settings.
                    for (k, v) in old {
                        if !["command", "args"].contains(&k.as_str()) {
                            def[k] = v.clone();
                        }
                    }
                }
                servers.insert(MCP_NAME.into(), def);
            }
        }
        if let Some(c) = change_if_different(
            &mpath,
            mbefore,
            &mroot,
            if action == Action::Uninstall { "remove mcpServers.contextforge" } else { "add mcpServers.contextforge" },
        ) {
            plan.changes.push(c);
        }
    }
    if action != Action::Uninstall {
        plan.notes
            .push("Applies to the Antigravity App, IDE and CLI (they share ~/.gemini/config). Start a new conversation to load it.".into());
        plan.notes.push("Antigravity asks before running MCP tools unless allowed; to skip prompts for ContextForge only, add \"mcp(contextforge/*)\" to your permission allow list.".into());
    }
    Ok(plan)
}

fn definition_is_ours(def: &Value) -> bool {
    let Some(obj) = def.as_object() else { return false };
    obj.values().filter_map(Value::as_array).flatten().any(|item| {
        let direct = item.get("command").and_then(Value::as_str);
        let nested =
            item.get("hooks").and_then(Value::as_array).and_then(|a| a.first()).and_then(|h| h.get("command")).and_then(Value::as_str);
        direct.or(nested).is_some_and(|c| is_contextforge_command(c) && c.contains(" hook antigravity "))
    })
}

fn commands_in(def: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for item in def.as_object().into_iter().flat_map(|o| o.values()).filter_map(Value::as_array).flatten() {
        if let Some(c) = item.get("command").and_then(Value::as_str) {
            out.push(c.to_string());
        }
        for h in item.get("hooks").and_then(Value::as_array).into_iter().flatten() {
            if let Some(c) = h.get("command").and_then(Value::as_str) {
                out.push(c.to_string());
            }
        }
    }
    out
}

pub fn hooks_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    let path = hooks_path(env, scope);
    let (_, root) = match read_object(&path) {
        Ok(r) => r,
        Err(e) => return ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&path)),
    };
    let Some(def) = root.get(HOOK_NAME) else {
        return ComponentStatus::new(ComponentState::NotInstalled, "the 'contextforge' hook is not configured", Some(&path));
    };
    if !definition_is_ours(def) {
        return ComponentStatus::new(ComponentState::Error, "a different hook named 'contextforge' exists", Some(&path));
    }
    if def.get("enabled").and_then(Value::as_bool) == Some(false) {
        return ComponentStatus::new(ComponentState::Partial, "the 'contextforge' hook is disabled in Antigravity", Some(&path));
    }
    let expected = hook_definition(exe);
    let want: Vec<String> = commands_in(&expected);
    let have = commands_in(def);
    if want.iter().all(|w| have.contains(w)) {
        ComponentStatus::new(ComponentState::Installed, "SessionStart, PreInvocation, PostToolUse and Stop hooks configured", Some(&path))
    } else if have.iter().any(|c| !c.to_lowercase().contains(&exe.to_string_lossy().to_lowercase())) {
        ComponentStatus::new(ComponentState::Outdated, "hooks point to a different ContextForge executable; run Repair", Some(&path))
    } else {
        ComponentStatus::new(ComponentState::Partial, "some ContextForge hook events are missing; run Repair", Some(&path))
    }
}

pub fn mcp_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    let path = mcp_path(env, scope);
    let (_, root) = match read_object(&path) {
        Ok(r) => r,
        Err(e) => return ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&path)),
    };
    let empty = Map::new();
    let servers = root.get("mcpServers").and_then(Value::as_object).unwrap_or(&empty);
    match servers.get(MCP_NAME) {
        None => ComponentStatus::new(ComponentState::NotInstalled, "MCP server not configured", Some(&path)),
        Some(e) => {
            let cmd = e.get("command").and_then(Value::as_str).unwrap_or("");
            if !is_contextforge_command(cmd) {
                ComponentStatus::new(ComponentState::Error, format!("an unrelated MCP server named '{MCP_NAME}' exists"), Some(&path))
            } else if !crate::claude::same_path(cmd, exe) {
                ComponentStatus::new(
                    ComponentState::Outdated,
                    "MCP server points to a different ContextForge executable; run Repair",
                    Some(&path),
                )
            } else if e.get("disabled").and_then(Value::as_bool) == Some(true) {
                ComponentStatus::new(ComponentState::Partial, "MCP server is disabled in Antigravity", Some(&path))
            } else {
                ComponentStatus::new(ComponentState::Installed, "MCP server configured", Some(&path))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Tools\ContextForge\contextforge.exe")
    }

    #[test]
    fn install_keeps_other_servers_and_hooks() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        std::fs::create_dir_all(&env.gemini_config_dir).unwrap();
        let servers = json!({"mcpServers": {"rea": {"command": "node.exe", "args": ["rea.mjs", "mcp"]}}});
        std::fs::write(mcp_path(&env, &Scope::User), serde_json::to_string_pretty(&servers).unwrap()).unwrap();
        let hooks = json!({"my-linter-hook": {"PostToolUse": [{"matcher": "run_command", "hooks": [{"type": "command", "command": "./lint.sh", "timeout": 10}]}]}});
        std::fs::write(hooks_path(&env, &Scope::User), hooks.to_string()).unwrap();

        let p = plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        crate::plan::apply(&p, &env).unwrap();
        let m: Value = serde_json::from_str(&std::fs::read_to_string(mcp_path(&env, &Scope::User)).unwrap()).unwrap();
        assert_eq!(m["mcpServers"]["rea"]["command"], "node.exe");
        assert_eq!(m["mcpServers"]["contextforge"]["args"], json!(["mcp"]));
        let h: Value = serde_json::from_str(&std::fs::read_to_string(hooks_path(&env, &Scope::User)).unwrap()).unwrap();
        assert_eq!(h["my-linter-hook"]["PostToolUse"][0]["hooks"][0]["command"], "./lint.sh");
        assert_eq!(
            h["contextforge"]["SessionStart"][0]["command"],
            r"C:\Tools\ContextForge\contextforge.exe hook antigravity session-start"
        );
        assert_eq!(h["contextforge"]["PostToolUse"][0]["matcher"], TOOL_MATCHER);
        assert!(h["contextforge"]["Stop"][0].get("matcher").is_none());
        assert_eq!(hooks_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);
        assert_eq!(mcp_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);

        // A user who disabled the hook keeps that choice through repair.
        let mut h2 = h.clone();
        h2["contextforge"]["enabled"] = json!(false);
        std::fs::write(hooks_path(&env, &Scope::User), serde_json::to_string_pretty(&h2).unwrap()).unwrap();
        let r = plan(Action::Repair, &Scope::User, &exe(), &env).unwrap();
        assert!(r.changes.iter().all(|c| !c.path.ends_with("hooks.json")), "nothing to repair");

        let un = plan(Action::Uninstall, &Scope::User, &exe(), &env).unwrap();
        crate::plan::apply(&un, &env).unwrap();
        let m: Value = serde_json::from_str(&std::fs::read_to_string(mcp_path(&env, &Scope::User)).unwrap()).unwrap();
        assert_eq!(m, servers);
        let h: Value = serde_json::from_str(&std::fs::read_to_string(hooks_path(&env, &Scope::User)).unwrap()).unwrap();
        assert_eq!(h, hooks);
    }

    #[test]
    fn workspace_scope_writes_into_dot_agents() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let ws = root.path().join("ws");
        let p = plan(Action::Install, &Scope::Project(ws.clone()), &exe(), &env).unwrap();
        let paths: Vec<PathBuf> = p.changes.iter().map(|c| c.path.clone()).collect();
        assert!(paths.contains(&ws.join(".agents").join("hooks.json")));
        assert!(paths.contains(&ws.join(".agents").join("mcp_config.json")));
    }

    #[test]
    fn foreign_entries_are_not_touched() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        std::fs::create_dir_all(&env.gemini_config_dir).unwrap();
        let foreign = json!({"contextforge": {"SessionStart": [{"type": "command", "command": "echo hi"}]}});
        std::fs::write(hooks_path(&env, &Scope::User), foreign.to_string()).unwrap();
        let p = plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        assert!(!p.warnings.is_empty());
        assert!(p.changes.iter().all(|c| !c.path.ends_with("hooks.json")));
    }
}
