//! Claude Code adapter (https://code.claude.com/docs/en/hooks).
//!
//! Input: snake_case JSON on stdin (`session_id`, `cwd`, `hook_event_name`, …).
//! Context injection: `hookSpecificOutput.additionalContext` for `SessionStart` and `UserPromptSubmit`
//! (capped by Claude Code at 10,000 characters). No output for other events.

use std::path::PathBuf;

use contextforge_core::capture::{FileAction, FileTouch, HookEvent, HookEventKind, ToolUse};
use contextforge_core::model::AgentKind;
use serde_json::{Value, json};

use super::{AdapterError, clamp, command_text, str_field, string_field};

pub const EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "PostToolUse", "PostToolUseFailure", "Stop", "SessionEnd"];

/// Claude Code stores longer context in a file and shows a preview; stay safely under its 10k cap.
const MAX_CONTEXT_CHARS: usize = 9_500;

pub fn parse(event: &str, v: &Value) -> Result<HookEvent, AdapterError> {
    if !EVENTS.contains(&event) {
        return Err(AdapterError::UnsupportedEvent { provider: "Claude Code", event: event.to_string() });
    }
    let session = string_field(v, "session_id").unwrap_or_default();
    let cwd = str_field(v, "cwd").map(PathBuf::from);
    let model = match v.get("model") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("id").or_else(|| o.get("display_name")).and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    let kind = match event {
        "SessionStart" => HookEventKind::SessionStart { source: string_field(v, "source") },
        "UserPromptSubmit" => HookEventKind::PromptSubmitted { prompt: v.get("prompt").and_then(Value::as_str).unwrap_or("").to_string() },
        "PostToolUse" => HookEventKind::ToolUsed(tool_use(v, false)),
        "PostToolUseFailure" => HookEventKind::ToolUsed(tool_use(v, true)),
        "Stop" => HookEventKind::TurnEnded { last_message: string_field(v, "last_assistant_message"), fully_idle: None, reason: None },
        "SessionEnd" => HookEventKind::SessionEnded { reason: string_field(v, "reason") },
        _ => unreachable!("checked above"),
    };
    Ok(HookEvent {
        agent: AgentKind::ClaudeCode,
        external_session_id: session,
        cwd,
        workspace_paths: Vec::new(),
        model,
        raw_event: event.into(),
        kind,
    })
}

fn tool_use(v: &Value, failed: bool) -> ToolUse {
    let tool = str_field(v, "tool_name").unwrap_or("").to_string();
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    let response = v.get("tool_response").cloned().unwrap_or(Value::Null);
    let mut tu = ToolUse { tool: tool.clone(), ..Default::default() };
    match tool.as_str() {
        "Write" => {
            if let Some(p) = str_field(&input, "file_path") {
                tu.files.push(FileTouch { path: p.into(), action: FileAction::Write });
            }
        }
        "Edit" | "MultiEdit" => {
            if let Some(p) = str_field(&input, "file_path") {
                tu.files.push(FileTouch { path: p.into(), action: FileAction::Edit });
            }
        }
        "NotebookEdit" => {
            if let Some(p) = str_field(&input, "notebook_path") {
                tu.files.push(FileTouch { path: p.into(), action: FileAction::Edit });
            }
        }
        "Bash" | "PowerShell" => {
            tu.command = command_text(input.get("command"));
            let exit = response.get("exitCode").or_else(|| response.get("exit_code")).and_then(Value::as_i64);
            let interrupted = response.get("interrupted").and_then(Value::as_bool).unwrap_or(false);
            tu.exit_code = exit;
            tu.success = Some(!failed && !interrupted && exit.is_none_or(|c| c == 0));
            if failed {
                tu.error = string_field(v, "error");
            }
        }
        _ => {}
    }
    tu
}

pub fn render(event: &str, context: Option<&str>) -> String {
    match (event, context) {
        ("SessionStart" | "UserPromptSubmit", Some(ctx)) if !ctx.trim().is_empty() => json!({
            "hookSpecificOutput": { "hookEventName": event, "additionalContext": clamp(ctx, MAX_CONTEXT_CHARS) }
        })
        .to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_session_start() {
        let v = json!({"session_id":"abc123","transcript_path":"C:\\t.jsonl","cwd":"C:\\Work\\Repo","hook_event_name":"SessionStart","source":"startup","model":"claude-sonnet"});
        let e = parse("SessionStart", &v).unwrap();
        assert_eq!(e.agent, AgentKind::ClaudeCode);
        assert_eq!(e.external_session_id, "abc123");
        assert_eq!(e.cwd.unwrap(), PathBuf::from("C:\\Work\\Repo"));
        assert_eq!(e.kind, HookEventKind::SessionStart { source: Some("startup".into()) });
        assert_eq!(e.model.as_deref(), Some("claude-sonnet"));
    }

    #[test]
    fn parses_file_and_command_tools() {
        let w = json!({"session_id":"s","cwd":"/r","tool_name":"Write","tool_input":{"file_path":"/r/src/a.rs","content":"x"},"tool_response":{"filePath":"/r/src/a.rs","type":"create"}});
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &w).unwrap().kind else { panic!() };
        assert_eq!(tu.files, vec![FileTouch { path: "/r/src/a.rs".into(), action: FileAction::Write }]);
        let b = json!({"session_id":"s","cwd":"/r","tool_name":"Bash","tool_input":{"command":"cargo test"},"tool_response":{"stdout":"ok","stderr":"","interrupted":false}});
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &b).unwrap().kind else { panic!() };
        assert_eq!(tu.command.as_deref(), Some("cargo test"));
        assert_eq!(tu.success, Some(true));
        let f = json!({"session_id":"s","cwd":"/r","tool_name":"Bash","tool_input":{"command":"cargo build"},"error":"exit code 101"});
        let HookEventKind::ToolUsed(tu) = parse("PostToolUseFailure", &f).unwrap().kind else { panic!() };
        assert_eq!(tu.success, Some(false));
        assert_eq!(tu.error.as_deref(), Some("exit code 101"));
    }

    #[test]
    fn renders_additional_context_only_when_present() {
        let out: Value = serde_json::from_str(&render("SessionStart", Some("<contextforge-memory>x</contextforge-memory>"))).unwrap();
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("contextforge-memory"));
        assert_eq!(render("SessionStart", None), "");
        assert_eq!(render("Stop", Some("ignored")), "");
        let big = "y".repeat(20_000);
        let out: Value = serde_json::from_str(&render("UserPromptSubmit", Some(&big))).unwrap();
        assert!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().chars().count() < 10_000);
    }

    #[test]
    fn rejects_unknown_events() {
        assert!(parse("Notification", &json!({})).is_err());
    }
}
