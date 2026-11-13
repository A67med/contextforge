//! Google Antigravity adapter (https://antigravity.google/docs/hooks) — App, IDE and CLI share it.
//!
//! Input: camelCase JSON on stdin (`conversationId`, `workspacePaths`, `transcriptPath`, `modelName`,
//! `lastUserInput`, …). There is no `hook_event_name`, so the event comes from the command line.
//! Output: `{"injectSteps":[{"systemMessage":{"systemMessage":"…"}}]}` to inject context; PostToolUse returns
//! `{}`; Stop must return a `decision` ("continue" would keep the agent running, so ContextForge returns
//! "stop").

use std::path::PathBuf;

use contextforge_core::capture::{FileAction, FileTouch, HookEvent, HookEventKind, ToolUse};
use contextforge_core::model::AgentKind;
use serde_json::{Value, json};

use super::{AdapterError, clamp, command_text, str_field, string_field};

pub const EVENTS: &[&str] = &["SessionStart", "PreInvocation", "PostToolUse", "PostInvocation", "PreToolUse", "Stop"];

const MAX_CONTEXT_CHARS: usize = 30_000;

/// Which Antigravity surface produced the event, from the documented `<app_data_dir>` in `transcriptPath`:
/// `~/.gemini/antigravity` (App), `~/.gemini/antigravity-ide` (IDE), `~/.gemini/antigravity-cli` (CLI).
pub fn surface(v: &Value) -> AgentKind {
    for key in ["transcriptPath", "artifactDirectoryPath"] {
        if let Some(p) = str_field(v, key) {
            let p = p.replace('\\', "/").to_lowercase();
            if p.contains("/.gemini/antigravity-cli/") {
                return AgentKind::AntigravityCli;
            }
            if p.contains("/.gemini/antigravity-ide/") {
                return AgentKind::AntigravityIde;
            }
            if p.contains("/.gemini/antigravity/") {
                return AgentKind::AntigravityApp;
            }
        }
    }
    AgentKind::Antigravity
}

pub fn parse(event: &str, v: &Value) -> Result<HookEvent, AdapterError> {
    if !EVENTS.contains(&event) {
        return Err(AdapterError::UnsupportedEvent { provider: "Antigravity", event: event.to_string() });
    }
    let workspace_paths: Vec<PathBuf> = v
        .get("workspacePaths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|s| !s.trim().is_empty()).map(PathBuf::from).collect())
        .unwrap_or_default();
    let kind = match event {
        "SessionStart" => HookEventKind::SessionStart { source: None },
        "PreInvocation" => HookEventKind::BeforeModel {
            last_user_input: string_field(v, "lastUserInput"),
            invocation: v.get("invocationNum").and_then(Value::as_i64),
        },
        "PostToolUse" => HookEventKind::ToolUsed(tool_use(v)),
        // Not installed by ContextForge; accepted so a stray configuration never errors.
        "PostInvocation" | "PreToolUse" => HookEventKind::BeforeModel { last_user_input: None, invocation: None },
        "Stop" => HookEventKind::TurnEnded {
            last_message: None,
            fully_idle: v.get("fullyIdle").and_then(Value::as_bool),
            reason: string_field(v, "terminationReason"),
        },
        _ => unreachable!("checked above"),
    };
    Ok(HookEvent {
        agent: surface(v),
        external_session_id: string_field(v, "conversationId").unwrap_or_default(),
        cwd: None,
        workspace_paths,
        model: string_field(v, "modelName"),
        raw_event: event.into(),
        kind,
    })
}

fn tool_use(v: &Value) -> ToolUse {
    let call = v.get("toolCall").cloned().unwrap_or(Value::Null);
    let name = str_field(&call, "name").unwrap_or("").to_string();
    let args = call.get("args").cloned().unwrap_or(Value::Null);
    let error = string_field(v, "error");
    let mut tu = ToolUse { tool: name.clone(), error: error.clone(), ..Default::default() };
    match name.as_str() {
        "write_to_file" => {
            if let Some(p) = str_field(&args, "TargetFile") {
                let overwrite = args.get("Overwrite").and_then(Value::as_bool).unwrap_or(false);
                tu.files.push(FileTouch { path: p.into(), action: if overwrite { FileAction::Write } else { FileAction::Create } });
            }
        }
        "replace_file_content" | "multi_replace_file_content" => {
            if let Some(p) = str_field(&args, "TargetFile") {
                tu.files.push(FileTouch { path: p.into(), action: FileAction::Edit });
            }
        }
        "run_command" => {
            tu.command = command_text(args.get("CommandLine"));
            // Documented: `error` is empty when the tool call succeeded (e.g. "exit status 1" on failure).
            tu.success = Some(error.is_none());
            if let Some(e) = &error
                && let Some(code) = e.strip_prefix("exit status ").and_then(|c| c.trim().parse().ok())
            {
                tu.exit_code = Some(code);
            }
        }
        _ => {}
    }
    tu
}

pub fn render(event: &str, context: Option<&str>) -> String {
    match event {
        "SessionStart" | "PreInvocation" => match context.filter(|c| !c.trim().is_empty()) {
            Some(ctx) => json!({ "injectSteps": [ { "systemMessage": { "systemMessage": clamp(ctx, MAX_CONTEXT_CHARS) } } ] }).to_string(),
            None => "{}".to_string(),
        },
        "Stop" => json!({ "decision": "stop" }).to_string(),
        "PreToolUse" => json!({ "decision": "allow" }).to_string(),
        _ => "{}".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common() -> Value {
        json!({
            "conversationId": "ec33ebf9-0cba-4100-8142-c61503f6c587",
            "workspacePaths": ["/workspace/project"],
            "transcriptPath": "C:\\Users\\dev\\.gemini\\antigravity-cli\\brain\\ec33\\.system_generated\\logs\\transcript.jsonl",
            "artifactDirectoryPath": "~/.gemini/antigravity-cli/brain/ec33",
            "modelName": "gemini-3.6-flash-medium"
        })
    }

    #[test]
    fn detects_surface_from_transcript_path() {
        assert_eq!(surface(&common()), AgentKind::AntigravityCli);
        assert_eq!(surface(&json!({"transcriptPath":"~/.gemini/antigravity-ide/brain/x/t.jsonl"})), AgentKind::AntigravityIde);
        assert_eq!(surface(&json!({"transcriptPath":"/home/u/.gemini/antigravity/brain/x/t.jsonl"})), AgentKind::AntigravityApp);
        assert_eq!(surface(&json!({})), AgentKind::Antigravity);
    }

    #[test]
    fn parses_session_start_and_pre_invocation() {
        let e = parse("SessionStart", &common()).unwrap();
        assert_eq!(e.external_session_id, "ec33ebf9-0cba-4100-8142-c61503f6c587");
        assert_eq!(e.workspace_paths, vec![PathBuf::from("/workspace/project")]);
        let mut p = common();
        p["invocationNum"] = json!(0);
        p["initialNumSteps"] = json!(2);
        p["lastUserInput"] = json!("Fix the failing unit test");
        let e = parse("PreInvocation", &p).unwrap();
        assert_eq!(e.kind, HookEventKind::BeforeModel { last_user_input: Some("Fix the failing unit test".into()), invocation: Some(0) });
    }

    #[test]
    fn parses_documented_post_tool_use() {
        let mut v = common();
        v["toolCall"] = json!({"name":"run_command","args":{"CommandLine":"npm test","Cwd":"/workspace/project","WaitMsBeforeAsync":5000}});
        v["stepIdx"] = json!(5);
        v["error"] = json!("exit status 1");
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &v).unwrap().kind else { panic!() };
        assert_eq!(tu.command.as_deref(), Some("npm test"));
        assert_eq!(tu.success, Some(false));
        assert_eq!(tu.exit_code, Some(1));
        v["toolCall"] = json!({"name":"replace_file_content","args":{"TargetFile":"/workspace/project/src/a.ts"}});
        v["error"] = json!("");
        let HookEventKind::ToolUsed(tu) = parse("PostToolUse", &v).unwrap().kind else { panic!() };
        assert_eq!(tu.files[0].path, "/workspace/project/src/a.ts");
    }

    #[test]
    fn renders_documented_output_shapes() {
        let out: Value = serde_json::from_str(&render("SessionStart", Some("brief"))).unwrap();
        assert_eq!(out["injectSteps"][0]["systemMessage"]["systemMessage"], "brief");
        assert_eq!(render("SessionStart", None), "{}");
        assert_eq!(render("PostToolUse", None), "{}");
        let stop: Value = serde_json::from_str(&render("Stop", None)).unwrap();
        assert_eq!(stop["decision"], "stop");
        assert_ne!(stop["decision"], "continue", "must never keep the agent running");
        let stop_parsed =
            parse("Stop", &json!({"conversationId":"c","executionNum":1,"terminationReason":"model_stop","fullyIdle":true})).unwrap();
        assert_eq!(
            stop_parsed.kind,
            HookEventKind::TurnEnded { last_message: None, fully_idle: Some(true), reason: Some("model_stop".into()) }
        );
    }
}
