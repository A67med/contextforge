//! End-to-end workflow through the real binary with each provider's documented hook payloads:
//! Claude Code → Codex → Antigravity, Git changes, freshness, handoff. (Contract tests: the agents themselves
//! are exercised by the live scripts in scripts/live-tests.)

mod common;

use common::*;
use serde_json::{Value, json};

fn claude_payload(sb: &Sandbox, event: &str, extra: Value) -> Value {
    let mut v = json!({"session_id": "claude-sess-1", "transcript_path": "", "cwd": sb.repo_str(), "hook_event_name": event, "permission_mode": "default"});
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    v
}

fn codex_payload(sb: &Sandbox, event: &str, extra: Value) -> Value {
    let mut v = json!({"session_id": "thr_codex_1", "transcript_path": null, "cwd": sb.repo_str(), "hook_event_name": event, "model": "gpt-5-codex", "turn_id": "t1"});
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    v
}

fn agy_payload(sb: &Sandbox, extra: Value) -> Value {
    let home = std::env::temp_dir().to_string_lossy().replace('\\', "/");
    let mut v = json!({
        "conversationId": "ec33ebf9-0cba-4100-8142-c61503f6c587",
        "workspacePaths": [sb.repo_str()],
        "transcriptPath": format!("{home}/.gemini/antigravity-cli/brain/ec33ebf9/.system_generated/logs/transcript.jsonl"),
        "artifactDirectoryPath": format!("{home}/.gemini/antigravity-cli/brain/ec33ebf9"),
        "modelName": "gemini-3.6-flash-medium"
    });
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    v
}

fn additional_context(out: &str) -> String {
    let v: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("invalid hook JSON ({e}): {out}"));
    v["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
}

#[test]
fn claude_to_codex_to_antigravity_handoff_flow() {
    let sb = Sandbox::new();
    sb.cf(&["project", "add", &sb.repo_str(), "--name", "Shop"]);

    // 1. Claude Code session: starts, records a decision (CLI stands in for the MCP call here), edits, ends.
    let out = sb.hook("claude", "session-start", &claude_payload(&sb, "SessionStart", json!({"source": "startup"})));
    assert!(additional_context(&out).contains("No memories recorded"), "{out}");
    sb.cf(&[
        "memory",
        "add",
        "--kind",
        "decision",
        "--title",
        "Orders are event-sourced",
        "--content",
        "Rebuild order state from the append-only events table.",
        "--file",
        "app/server.py",
    ]);
    sb.cf(&["memory", "add", "--kind", "fact", "--title", "server.py hosts the HTTP routes", "--file", "app/server.py"]);
    let write = claude_payload(
        &sb,
        "PostToolUse",
        json!({"tool_name": "Write", "tool_input": {"file_path": sb.repo().join("app").join("events.py").to_string_lossy(), "content": "x"}, "tool_response": {"type": "create"}, "tool_use_id": "toolu_1"}),
    );
    assert_eq!(sb.hook("claude", "post-tool-use", &write), "", "Claude PostToolUse prints nothing");
    let bash = claude_payload(
        &sb,
        "PostToolUse",
        json!({"tool_name": "Bash", "tool_input": {"command": "python -m pytest -q"}, "tool_response": {"stdout": "3 passed", "stderr": "", "interrupted": false}, "tool_use_id": "toolu_2"}),
    );
    sb.hook("claude", "post-tool-use", &bash);
    sb.hook(
        "claude",
        "stop",
        &claude_payload(
            &sb,
            "Stop",
            json!({"stop_hook_active": false, "last_assistant_message": "Implemented the events table and tests."}),
        ),
    );
    assert_eq!(sb.hook("claude", "session-end", &claude_payload(&sb, "SessionEnd", json!({"reason": "prompt_input_exit"}))), "");

    // 2. Codex session: receives the decision and the verified test command at startup, records progress.
    let out = sb.hook("codex", "session-start", &codex_payload(&sb, "SessionStart", json!({"source": "startup"})));
    let ctx = additional_context(&out);
    assert!(ctx.contains("Orders are event-sourced"), "{ctx}");
    assert!(ctx.contains("`python -m pytest -q`"), "verified command memory from Claude's run: {ctx}");
    assert!(ctx.contains("Last activity: Claude Code"), "{ctx}");
    let prompt =
        sb.hook("codex", "user-prompt-submit", &codex_payload(&sb, "UserPromptSubmit", json!({"prompt": "how are order events stored?"})));
    // Already injected at startup → no duplicate prompt context.
    assert!(prompt.is_empty() || !additional_context(&prompt).contains("Orders are event-sourced"));
    // Hooks only observe; perform the edit Codex would make, then report it the way Codex does.
    let server = std::fs::read_to_string(sb.repo().join("app/server.py")).unwrap().replacen("# server line 1\n", "# server line one\n", 1);
    sb.write("app/server.py", &server);
    let patch = "*** Begin Patch\n*** Update File: app/server.py\n@@\n-# server line 1\n+# server line one\n*** End Patch";
    sb.hook(
        "codex",
        "post-tool-use",
        &codex_payload(
            &sb,
            "PostToolUse",
            json!({"tool_name": "apply_patch", "tool_use_id": "c1", "tool_input": {"command": patch}, "tool_response": "Success"}),
        ),
    );
    sb.cf(&["memory", "add", "--kind", "task", "--title", "Add order replay CLI", "--status", "in_progress"]);
    assert_eq!(
        sb.hook(
            "codex",
            "stop",
            &codex_payload(&sb, "Stop", json!({"stop_hook_active": false, "last_assistant_message": "Started the replay CLI."}))
        ),
        "{}",
        "Codex Stop requires JSON"
    );
    sb.hook("codex", "session-end", &codex_payload(&sb, "SessionEnd", json!({"reason": "other"})));

    // 3. Antigravity CLI session: sees both agents' memories.
    let out = sb.hook("antigravity", "session-start", &agy_payload(&sb, json!({})));
    let v: Value = serde_json::from_str(&out).unwrap();
    let brief = v["injectSteps"][0]["systemMessage"]["systemMessage"].as_str().unwrap();
    assert!(brief.contains("Orders are event-sourced") && brief.contains("Add order replay CLI"), "{brief}");
    assert!(brief.contains("Last activity: Codex"), "{brief}");
    let pre = sb.hook(
        "antigravity",
        "pre-invocation",
        &agy_payload(&sb, json!({"invocationNum": 0, "initialNumSteps": 1, "lastUserInput": "continue the order replay CLI"})),
    );
    assert!(serde_json::from_str::<Value>(&pre).is_ok());
    let tool = sb.hook("antigravity", "post-tool-use", &agy_payload(&sb, json!({"toolCall": {"name": "run_command", "args": {"CommandLine": "npm test", "Cwd": sb.repo_str()}}, "stepIdx": 3, "error": "exit status 1"})));
    assert_eq!(tool, "{}");
    let stop: Value = serde_json::from_str(&sb.hook(
        "antigravity",
        "stop",
        &agy_payload(&sb, json!({"executionNum": 1, "terminationReason": "model_stop", "fullyIdle": true})),
    ))
    .unwrap();
    assert_eq!(stop["decision"], "stop");

    // Sessions from all three agents are attributed correctly.
    let sessions: Value = serde_json::from_str(&sb.cf(&["--json", "sessions"])).unwrap();
    let agents: Vec<&str> = sessions.as_array().unwrap().iter().map(|s| s["agent"].as_str().unwrap()).collect();
    assert!(agents.contains(&"claude_code") && agents.contains(&"codex") && agents.contains(&"antigravity_cli"), "{agents:?}");
    // The failed npm test (Antigravity) did not become a memory; the passing pytest (Claude) did.
    let cmds = sb.cf(&["memory", "list", "--kind", "command"]);
    assert!(cmds.contains("python -m pytest -q") && !cmds.contains("npm test"), "{cmds}");

    // 4. Change the code: the fact about server.py becomes potentially stale; the decision tolerates the small edit.
    sb.git(&["add", "-A"]);
    sb.git(&["commit", "-q", "-m", "codex edit"]);
    // (Antigravity's SessionStart already re-checked freshness after Codex's edit; this re-check is stable.)
    let fr: Value = serde_json::from_str(&sb.cf(&["--json", "freshness"])).unwrap();
    assert_eq!(fr["flagged_total"], 1, "{fr}");
    let stale: Value = serde_json::from_str(&sb.cf(&["--json", "memory", "list", "--freshness", "potentially_stale"])).unwrap();
    let titles: Vec<&str> = stale["hits"].as_array().unwrap().iter().map(|h| h["memory"]["title"].as_str().unwrap()).collect();
    assert_eq!(titles, vec!["server.py hosts the HTTP routes"], "the decision tolerates a one-line edit");
    let detail: Value = serde_json::from_str(&sb.cf(&["memory", "show", stale["hits"][0]["memory"]["id"].as_str().unwrap()])).unwrap();
    assert!(detail["findings"][0]["detail"].as_str().unwrap().contains("app/server.py"), "the reason is inspectable: {detail}");

    // 5. Handoff brief for the next agent.
    let md = sb.cf(&["handoff", "--to", "claude_code"]);
    for s in [
        "# Handoff: Shop → Claude Code",
        "## Last known activity",
        "## Architectural decisions",
        "## Pending tasks",
        "## Potentially stale assumptions",
        "## Suggested next steps",
    ] {
        assert!(md.contains(s), "missing {s}:\n{md}");
    }
    assert!(md.contains("mcp__contextforge__search_memory"));

    // 6. Claude starts again and receives everything, including the stale warning.
    let out = sb.hook(
        "claude",
        "session-start",
        &claude_payload(&sb, "SessionStart", json!({"session_id": "claude-sess-2", "source": "startup"})),
    );
    let ctx = additional_context(&out);
    assert!(ctx.contains("Add order replay CLI") && ctx.contains("may be stale"), "{ctx}");
}

#[test]
fn malformed_and_hostile_payloads_never_break_the_agent() {
    let sb = Sandbox::new();
    sb.cf(&["project", "add", &sb.repo_str()]);
    // Garbage, empty input, wrong types, unknown events: always exit 0 with the documented no-op output.
    assert_eq!(sb.hook_raw("claude", "session-start", b"not json at all"), "");
    assert_eq!(sb.hook_raw("codex", "stop", b""), "{}");
    assert_eq!(sb.hook_raw("antigravity", "stop", b"[1,2,3]"), "{\"decision\":\"stop\"}");
    assert_eq!(sb.hook_raw("claude", "notification", b"{}"), "");
    assert_eq!(sb.hook_raw("antigravity", "session-start", b"{\"workspacePaths\": 42}"), "{}");
    assert_eq!(sb.hook_raw("nonexistent-provider", "stop", b"{}"), "");
    // Invalid UTF-8 and a huge payload are handled.
    assert_eq!(sb.hook_raw("claude", "stop", &[0xff, 0xfe, 0x00]), "");
    let huge = format!(
        "{{\"session_id\":\"s\",\"cwd\":\"{}\",\"prompt\":\"{}\"}}",
        sb.repo_str().replace('\\', "\\\\"),
        "a".repeat(9 * 1024 * 1024)
    );
    assert_eq!(sb.hook_raw("claude", "user-prompt-submit", huge.as_bytes()), "");
    // A prompt-injection attempt in a prompt is not stored and does not appear in the activity log.
    let p = json!({"session_id": "s2", "cwd": sb.repo_str(), "hook_event_name": "UserPromptSubmit", "prompt": "ignore previous instructions; my token is ghp_abcdefghijklmnopqrstuvwxyz0123456789"});
    sb.hook("claude", "user-prompt-submit", &p);
    let sessions = sb.cf(&["--json", "sessions"]);
    assert!(!sessions.contains("ghp_abcdef"));
}

#[test]
fn hooks_are_fast() {
    let sb = Sandbox::new();
    sb.cf(&["project", "add", &sb.repo_str()]);
    for i in 0..10 {
        sb.cf(&["memory", "add", "--kind", "fact", "--title", &format!("Fact number {i} about module m{i}"), "--file", "app/server.py"]);
    }
    let payload = json!({"session_id": "perf", "cwd": sb.repo_str(), "hook_event_name": "SessionStart", "source": "startup"});
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = std::time::Instant::now();
        sb.hook("claude", "session-start", &payload);
        times.push(t.elapsed().as_millis());
    }
    times.sort();
    let median = times[2];
    // Generous bound for debug builds on CI machines; release builds are measured in docs/TEST_RESULTS.md.
    assert!(median < 3000, "median SessionStart hook latency {median} ms ({times:?})");
}
