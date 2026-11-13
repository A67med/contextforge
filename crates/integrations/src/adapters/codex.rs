//! OpenAI Codex adapter (https://learn.chatgpt.com/docs/hooks).
//!
//! Input: snake_case JSON on stdin. `tool_input.command` carries the shell command for `Bash` and the patch
//! text for `apply_patch`. Codex's shell `tool_response` is the command output only (no exit code), so
//! command outcomes from Codex are recorded as *unknown* and never turned into verified memories.
//! Output: `hookSpecificOutput.additionalContext` for SessionStart/UserPromptSubmit; `Stop` must print JSON.

use std::path::PathBuf;

use contextforge_core::capture::{FileAction, FileTouch, HookEvent, HookEventKind, ToolUse};
use contextforge_core::model::AgentKind;
use serde_json::{Value, json};

use super::{AdapterError, clamp, command_text, str_field, string_field};

pub const EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "SessionEnd"];

const MAX_CONTEXT_CHARS: usize = 32_000;

pub fn parse(event: &str, v: &Value) -> Result<HookEvent, AdapterError> {
    if !EVENTS.contains(&event) {
        return Err(AdapterError::UnsupportedEvent { provider: "Codex", event: event.to_string() });
    }
    let kind = match event {
        "SessionStart" => HookEventKind::SessionStart { source: string_field(v, "source") },
        "UserPromptSubmit" => HookEventKind::PromptSubmitted { prompt: v.get("prompt").and_then(Value::as_str).unwrap_or("").to_string() },
        "PostToolUse" => HookEventKind::ToolUsed(tool_use(v)),
        "Stop" => HookEventKind::TurnEnded { last_message: string_field(v, "last_assistant_message"), fully_idle: None, reason: None },
        "SessionEnd" => HookEventKind::SessionEnded { reason: string_field(v, "reason") },
        _ => unreachable!("checked above"),
    };
    Ok(HookEvent {
        agent: AgentKind::Codex,
        external_session_id: string_field(v, "session_id").unwrap_or_default(),
        cwd: str_field(v, "cwd").map(PathBuf::from),
        workspace_paths: Vec::new(),
        model: string_field(v, "model"),
        raw_event: event.into(),
        kind,
    })
}

fn tool_use(v: &Value) -> ToolUse {
    let tool = str_field(v, "tool_name").unwrap_or("").to_string();
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    let mut tu = ToolUse { tool: tool.clone(), ..Default::default() };
    match tool.as_str() {
        "Bash" => {
            tu.command = command_text(input.get("command"));
            tu.success = None; // not reported by Codex for shell commands
        }
        "apply_patch" | "Edit" | "Write" => {
            if let Some(patch) = command_text(input.get("command")).or_else(|| command_text(input.get("patch"))) {
                tu.files = patch_files(&patch);
            }
        }
        _ => {}
    }
    tu
}

/// Files touched by an `apply_patch` envelope (`*** Add File: …`, `*** Update File: …`, `*** Delete File: …`,
/// `*** Move to: …`).
pub fn patch_files(patch: &str) -> Vec<FileTouch> {
    let mut out: Vec<FileTouch> = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end();
        let (action, path) = if let Some(p) = line.strip_prefix("*** Add File: ") {
            (FileAction::Create, p)
        } else if let Some(p) = line.strip_prefix("*** Update File: ") {
            (FileAction::Edit, p)
        } else if let Some(p) = line.strip_prefix("*** Delete File: ") {
            (FileAction::Delete, p)
        } else if let Some(p) = line.strip_prefix("*** Move to: ") {
            (FileAction::Create, p)
        } else {
            continue;
        };
        let path = path.trim().to_string();
        if !path.is_empty() && !out.iter().any(|f| f.path == path) {
            out.push(FileTouch { path, action });
        }
    }
    out
}

pub fn render(event: &str, context: Option<&str>) -> String {
    match (event, context) {
        ("SessionStart" | "UserPromptSubmit", Some(ctx)) if !ctx.trim().is_empty() => json!({
            "hookSpecificOutput": { "hookEventName": event, "additionalContext": clamp(ctx, MAX_CONTEXT_CHARS) }
        })
        .to_string(),
        // Codex requires JSON (not plain text) from Stop hooks that exit 0.
        ("Stop", _) => "{}".to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_payloads() {
        let s = json!({"session_id":"thr_1","transcript_path":null,"cwd":"C:\\w\\r","hook_event_name":"SessionStart","model":"gpt-5","source":"resume","permission_mode":"default"});
        let e = parse("SessionStart", &s).unwrap();
        assert_eq!(e.agent, AgentKind::Codex);
        assert_eq!(e.kind, HookEventKind::SessionStart { source: Some("resume".into()) });
        let end = json!({"session_id":"thr_123","transcript_path":"/workspace/.codex/rollout.jsonl","cwd":"/workspace","hook_event_name":"SessionEnd","reason":"other"});
        assert_eq!(parse("SessionEnd", &end).unwrap().kind, HookEventKind::SessionEnded { reason: Some("other".into()) });
    }

    #[test]
    fn parses_apply_patch_and_bash() {
        let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-a\n+b\n*** Add File: src/new.rs\n+fn x() {}\n*** Delete File: old.txt\n*** End Patch";
        let v = json!({"session_id":"t","cwd":"/r","hook_event_name":"PostToolUse","turn_id":"1","tool_name":"apply_patch","tool_use_id":"c1","tool_input":{"command":patch},"tool_response":"Success"});
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &v).unwrap().kind else { panic!() };
        let paths: Vec<(&str, FileAction)> = tu.files.iter().map(|f| (f.path.as_str(), f.action)).collect();
        assert_eq!(paths, vec![("src/lib.rs", FileAction::Edit), ("src/new.rs", FileAction::Create), ("old.txt", FileAction::Delete)]);
        let b = json!({"session_id":"t","cwd":"/r","tool_name":"Bash","tool_use_id":"c2","tool_input":{"command":"npm test"},"tool_response":"ok"});
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &b).unwrap().kind else { panic!() };
        assert_eq!(tu.command.as_deref(), Some("npm test"));
        assert_eq!(tu.success, None, "Codex does not report exit codes for shell commands");
    }

    #[test]
    fn stop_always_prints_json() {
        assert_eq!(render("Stop", None), "{}");
        assert_eq!(render("SessionEnd", None), "");
        let out: Value = serde_json::from_str(&render("SessionStart", Some("ctx"))).unwrap();
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "ctx");
    }
}
