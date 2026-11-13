//! Claude Code integration.
//!
//! * Hooks: `~/.claude/settings.json` (user) or `<project>/.claude/settings.local.json` (project, not committed).
//!   Exec form (`command` = executable, `args` = argv) so no shell quoting is involved on Windows.
//! * MCP: registered with the official `claude mcp add --scope user|local` CLI, because `~/.claude.json` is
//!   rewritten by Claude Code itself and must not be edited concurrently.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::jsoncfg::{array_entry, change_if_different, object_entry, read_object};
use crate::plan::{Action, ExternalCommand, InstallPlan, Scope};
use crate::status::{ComponentState, ComponentStatus};
use crate::{Env, Provider, Result, exe_display, is_contextforge_command};

pub const MCP_NAME: &str = "contextforge";

pub struct HookSpec {
    pub event: &'static str,
    pub matcher: Option<&'static str>,
    pub timeout: u32,
    pub arg: &'static str,
}

pub const HOOKS: &[HookSpec] = &[
    HookSpec { event: "SessionStart", matcher: Some("startup|resume|clear|compact"), timeout: 30, arg: "session-start" },
    HookSpec { event: "UserPromptSubmit", matcher: None, timeout: 15, arg: "user-prompt-submit" },
    HookSpec {
        event: "PostToolUse",
        matcher: Some("Write|Edit|MultiEdit|NotebookEdit|Bash|PowerShell"),
        timeout: 15,
        arg: "post-tool-use",
    },
    HookSpec { event: "PostToolUseFailure", matcher: Some("Bash|PowerShell"), timeout: 15, arg: "post-tool-use-failure" },
    HookSpec { event: "Stop", matcher: None, timeout: 15, arg: "stop" },
    HookSpec { event: "SessionEnd", matcher: None, timeout: 5, arg: "session-end" },
];

pub fn settings_path(env: &Env, scope: &Scope) -> PathBuf {
    match scope {
        Scope::User => env.claude_config_dir.join("settings.json"),
        Scope::Project(p) => p.join(".claude").join("settings.local.json"),
    }
}

pub fn handler(exe: &Path, spec: &HookSpec) -> Value {
    json!({ "type": "command", "command": exe_display(exe), "args": ["hook", "claude", spec.arg], "timeout": spec.timeout })
}

fn is_ours(h: &Value) -> bool {
    let cmd = h.get("command").and_then(Value::as_str).unwrap_or("");
    if !is_contextforge_command(cmd) {
        return false;
    }
    let exec_form = h.get("args").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str) == Some("hook");
    exec_form || cmd.contains(" hook claude ")
}

/// Remove every ContextForge handler; drop groups/events that become empty. Returns removed count.
fn strip_ours(hooks: &mut Map<String, Value>) -> usize {
    let mut removed = 0;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for ev in events {
        let Some(Value::Array(groups)) = hooks.get_mut(&ev) else { continue };
        for g in groups.iter_mut() {
            if let Some(Value::Array(handlers)) = g.get_mut("hooks") {
                let before = handlers.len();
                handlers.retain(|h| !is_ours(h));
                removed += before - handlers.len();
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|a| !a.is_empty()));
        if groups.is_empty() {
            hooks.remove(&ev);
        }
    }
    removed
}

/// Our handlers currently configured: (event, command path).
fn ours(hooks: &Map<String, Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (ev, groups) in hooks {
        for g in groups.as_array().into_iter().flatten() {
            for h in g.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                if is_ours(h) {
                    out.push((ev.clone(), h.get("command").and_then(Value::as_str).unwrap_or("").to_string()));
                }
            }
        }
    }
    out
}

/// The MCP entry Claude Code currently has for ContextForge at this scope (read-only).
pub fn current_mcp(env: &Env, scope: &Scope) -> Result<Option<Value>> {
    let (_, root) = read_object(&env.claude_json)?;
    let servers = match scope {
        Scope::User => root.get("mcpServers").cloned(),
        Scope::Project(p) => {
            let want = p.to_string_lossy().replace('\\', "/").to_lowercase();
            root.get("projects")
                .and_then(Value::as_object)
                .and_then(|projects| projects.iter().find(|(k, _)| k.replace('\\', "/").to_lowercase() == want).map(|(_, v)| v.clone()))
                .and_then(|p| p.get("mcpServers").cloned())
        }
    };
    Ok(servers.and_then(|s| s.get(MCP_NAME).cloned()))
}

fn mcp_points_to(entry: &Value, exe: &Path) -> bool {
    let cmd = entry.get("command").and_then(Value::as_str).unwrap_or("");
    same_path(cmd, exe) && entry.get("args").and_then(Value::as_array).is_some_and(|a| a.first().and_then(Value::as_str) == Some("mcp"))
}

pub(crate) fn same_path(a: &str, b: &Path) -> bool {
    a.trim_matches('"').replace('\\', "/").eq_ignore_ascii_case(&b.to_string_lossy().replace('\\', "/"))
}

pub fn plan(action: Action, scope: &Scope, exe: &Path, env: &Env) -> Result<InstallPlan> {
    let mut plan = InstallPlan::new(Provider::Claude, action, scope.clone());
    let path = settings_path(env, scope);
    let (before, mut root) = read_object(&path)?;
    {
        let hooks = object_entry(&mut root, "hooks", &path)?;
        strip_ours(hooks);
        if action != Action::Uninstall {
            for spec in HOOKS {
                let mut group = Map::new();
                if let Some(m) = spec.matcher {
                    group.insert("matcher".into(), Value::String(m.into()));
                }
                group.insert("hooks".into(), Value::Array(vec![handler(exe, spec)]));
                array_entry(hooks, spec.event, &path)?.push(Value::Object(group));
            }
        }
    }
    if root.get("hooks").and_then(Value::as_object).is_some_and(|h| h.is_empty()) {
        root.remove("hooks");
    }
    let summary = match action {
        Action::Uninstall => "remove ContextForge hooks",
        _ => "add ContextForge lifecycle hooks (SessionStart, UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop, SessionEnd)",
    };
    if let Some(change) = change_if_different(&path, before, &root, summary) {
        plan.changes.push(change);
    }

    // --- MCP server registration through the official CLI ---
    let current = match current_mcp(env, scope) {
        Ok(c) => c,
        Err(e) => {
            plan.warnings.push(format!("could not read {}: {e}", env.claude_json.display()));
            None
        }
    };
    let (scope_flag, cwd) = match scope {
        Scope::User => ("user", None),
        Scope::Project(p) => ("local", Some(p.clone())),
    };
    let add_args = vec![
        "mcp".to_string(),
        "add".into(),
        "--scope".into(),
        scope_flag.into(),
        MCP_NAME.into(),
        "--".into(),
        exe_display(exe),
        "mcp".into(),
    ];
    let remove_args = vec!["mcp".to_string(), "remove".into(), "--scope".into(), scope_flag.into(), MCP_NAME.into()];
    let foreign = current.as_ref().is_some_and(|c| !is_contextforge_command(c.get("command").and_then(Value::as_str).unwrap_or("")));
    let mut cmds = Vec::new();
    match action {
        Action::Install | Action::Repair => {
            if foreign {
                plan.warnings.push(format!(
                    "an MCP server named '{MCP_NAME}' that is not ContextForge already exists in Claude Code; it was left untouched"
                ));
            } else if current.as_ref().is_some_and(|c| mcp_points_to(c, exe)) {
                // already correct
            } else {
                if current.is_some() {
                    cmds.push(ExternalCommand {
                        program: PathBuf::new(),
                        args: remove_args.clone(),
                        cwd: cwd.clone(),
                        summary: "remove the outdated ContextForge MCP registration".into(),
                        undo_args: None,
                    });
                }
                cmds.push(ExternalCommand {
                    program: PathBuf::new(),
                    args: add_args.clone(),
                    cwd: cwd.clone(),
                    summary: format!("register the ContextForge MCP server ({scope_flag} scope)"),
                    undo_args: Some(remove_args.clone()),
                });
            }
        }
        Action::Uninstall => {
            if current.is_some() && !foreign {
                cmds.push(ExternalCommand {
                    program: PathBuf::new(),
                    args: remove_args.clone(),
                    cwd: cwd.clone(),
                    summary: "unregister the ContextForge MCP server".into(),
                    undo_args: None,
                });
            }
        }
    }
    if !cmds.is_empty() {
        match &env.claude_cli {
            Some(cli) => {
                for mut c in cmds {
                    c.program = cli.clone();
                    plan.commands.push(c);
                }
            }
            None => {
                let manual = cmds.iter().map(|c| format!("claude {}", c.args.join(" "))).collect::<Vec<_>>().join("  &&  ");
                plan.warnings.push(format!(
                    "Claude Code CLI not found on PATH, so the MCP server cannot be registered automatically. Run: {manual}"
                ));
            }
        }
    }
    if action != Action::Uninstall {
        plan.notes.push(
            "New Claude Code sessions load the hooks and MCP server automatically; restart sessions that are already running.".into(),
        );
    }
    Ok(plan)
}

pub fn hooks_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    let path = settings_path(env, scope);
    let (_, root) = match read_object(&path) {
        Ok(r) => r,
        Err(e) => return ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&path)),
    };
    let empty = Map::new();
    let hooks = root.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
    let found = ours(hooks);
    let events: std::collections::HashSet<&str> = found.iter().map(|(e, _)| e.as_str()).collect();
    if found.is_empty() {
        return ComponentStatus::new(ComponentState::NotInstalled, "no ContextForge hooks configured", Some(&path));
    }
    if found.iter().any(|(_, cmd)| !same_path(cmd, exe)) {
        return ComponentStatus::new(
            ComponentState::Outdated,
            "hooks point to a different ContextForge executable; run Repair",
            Some(&path),
        );
    }
    if HOOKS.iter().all(|s| events.contains(s.event)) {
        ComponentStatus::new(ComponentState::Installed, format!("{} lifecycle hooks configured", HOOKS.len()), Some(&path))
    } else {
        ComponentStatus::new(ComponentState::Partial, format!("{} of {} hook events configured", events.len(), HOOKS.len()), Some(&path))
    }
}

pub fn mcp_status(env: &Env, scope: &Scope, exe: &Path) -> ComponentStatus {
    match current_mcp(env, scope) {
        Err(e) => ComponentStatus::new(ComponentState::Error, e.to_string(), Some(&env.claude_json)),
        Ok(None) => ComponentStatus::new(ComponentState::NotInstalled, "MCP server not registered", Some(&env.claude_json)),
        Ok(Some(entry)) if mcp_points_to(&entry, exe) => {
            ComponentStatus::new(ComponentState::Installed, "MCP server registered", Some(&env.claude_json))
        }
        Ok(Some(entry)) if is_contextforge_command(entry.get("command").and_then(Value::as_str).unwrap_or("")) => ComponentStatus::new(
            ComponentState::Outdated,
            "MCP server points to a different ContextForge executable; run Repair",
            Some(&env.claude_json),
        ),
        Ok(Some(_)) => ComponentStatus::new(
            ComponentState::Error,
            format!("an unrelated MCP server named '{MCP_NAME}' exists"),
            Some(&env.claude_json),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\ContextForge\contextforge.exe")
    }

    #[test]
    fn install_preserves_user_settings_and_hooks() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let path = settings_path(&env, &Scope::User);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let user = json!({
            "theme": "dark",
            "permissions": {"allow": ["Bash(npm test)"]},
            "hooks": {"PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "npx prettier --write"}]}]}
        });
        std::fs::write(&path, serde_json::to_string_pretty(&user).unwrap()).unwrap();
        let plan = plan(Action::Install, &Scope::User, &exe(), &env).unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert!(!plan.warnings.is_empty(), "no claude CLI in the test env → manual MCP instructions");
        crate::plan::apply(&plan, &env).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["permissions"]["allow"][0], "Bash(npm test)");
        let post = v["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 2, "user hook kept, ours appended");
        assert_eq!(post[0]["hooks"][0]["command"], "npx prettier --write");
        let start = &v["hooks"]["SessionStart"][0];
        assert_eq!(start["matcher"], "startup|resume|clear|compact");
        assert_eq!(start["hooks"][0]["args"], json!(["hook", "claude", "session-start"]));
        assert_eq!(hooks_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);

        // Installing again is a no-op (idempotent).
        assert!(super::plan(Action::Install, &Scope::User, &exe(), &env).unwrap().changes.is_empty());
        // Moving the executable makes the hooks outdated; repair rewrites only our entries.
        let moved = PathBuf::from(r"D:\Apps\contextforge.exe");
        assert_eq!(hooks_status(&env, &Scope::User, &moved).state, ComponentState::Outdated);
        let repair = super::plan(Action::Repair, &Scope::User, &moved, &env).unwrap();
        crate::plan::apply(&repair, &env).unwrap();
        assert_eq!(hooks_status(&env, &Scope::User, &moved).state, ComponentState::Installed);

        // Uninstall removes only ContextForge entries.
        let un = super::plan(Action::Uninstall, &Scope::User, &moved, &env).unwrap();
        crate::plan::apply(&un, &env).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
        assert!(v["hooks"].get("SessionStart").is_none());
    }

    #[test]
    fn project_scope_uses_local_settings_and_local_mcp() {
        let root = tempfile::tempdir().unwrap();
        let mut env = Env::for_test(root.path());
        env.claude_cli = Some(PathBuf::from("claude"));
        let project = root.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let plan = plan(Action::Install, &Scope::Project(project.clone()), &exe(), &env).unwrap();
        assert_eq!(plan.changes[0].path, project.join(".claude").join("settings.local.json"));
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].args[..4], ["mcp", "add", "--scope", "local"]);
        assert_eq!(plan.commands[0].cwd.as_deref(), Some(project.as_path()));
    }

    #[test]
    fn mcp_detection_reads_claude_json() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let cfg = json!({"mcpServers": {"contextforge": {"type": "stdio", "command": exe().to_string_lossy(), "args": ["mcp"]}},
                         "projects": {"C:/Work/App": {"mcpServers": {}}}});
        std::fs::write(&env.claude_json, cfg.to_string()).unwrap();
        assert_eq!(mcp_status(&env, &Scope::User, &exe()).state, ComponentState::Installed);
        assert_eq!(mcp_status(&env, &Scope::Project(PathBuf::from(r"C:\Work\App")), &exe()).state, ComponentState::NotInstalled);
    }

    #[test]
    fn refuses_invalid_settings() {
        let root = tempfile::tempdir().unwrap();
        let env = Env::for_test(root.path());
        let path = settings_path(&env, &Scope::User);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ invalid").unwrap();
        assert!(plan(Action::Install, &Scope::User, &exe(), &env).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ invalid");
    }
}
