//! Normalised hook-event processing, including the cross-agent flow.

mod common;

use common::*;
use contextforge_core::capture::{FileAction, FileTouch, HookEvent, HookEventKind, ToolUse};
use contextforge_core::memories::NewMemory;
use contextforge_core::model::*;
use contextforge_core::search::SearchQuery;

fn ev(agent: AgentKind, session: &str, cwd: &std::path::Path, kind: HookEventKind) -> HookEvent {
    HookEvent {
        agent,
        external_session_id: session.into(),
        cwd: Some(cwd.to_path_buf()),
        workspace_paths: vec![],
        model: Some("test-model".into()),
        raw_event: "test".into(),
        kind,
    }
}

#[test]
fn unregistered_projects_are_a_no_op() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let out =
        store.process_hook_event(&ev(AgentKind::ClaudeCode, "s", &repo.path(), HookEventKind::SessionStart { source: None })).unwrap();
    assert!(out.skipped.is_some());
    assert!(out.context.is_none());
    assert_eq!(store.list_sessions(&Default::default()).unwrap().len(), 0);
}

#[test]
fn auto_registration_is_opt_in() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let mut s = store.settings().unwrap();
    s.capture.auto_register_projects = true;
    store.save_settings(&s).unwrap();
    let out = store
        .process_hook_event(&ev(AgentKind::Codex, "s", &repo.path().join("src"), HookEventKind::SessionStart { source: None }))
        .unwrap();
    assert!(out.project_id.is_some());
    assert_eq!(store.list_projects().unwrap().len(), 1);
}

#[test]
fn disabled_projects_are_skipped() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let mut settings = p.settings.clone();
    settings.capture_enabled = false;
    store.update_project(&p.id, None, Some(&settings)).unwrap();
    let out =
        store.process_hook_event(&ev(AgentKind::ClaudeCode, "s", &repo.path(), HookEventKind::SessionStart { source: None })).unwrap();
    assert!(out.skipped.unwrap().contains("disabled"));
}

#[test]
fn tool_events_record_files_commands_and_verified_command_memories() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    repo.write(".env", "TOKEN=1");
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let root = repo.path();
    let edit = ToolUse {
        tool: "Edit".into(),
        files: vec![
            FileTouch { path: root.join("src/db.rs").to_string_lossy().into_owned(), action: FileAction::Edit },
            FileTouch { path: ".env".into(), action: FileAction::Edit },
            FileTouch { path: "../../outside.rs".into(), action: FileAction::Edit },
        ],
        ..Default::default()
    };
    let out = store.process_hook_event(&ev(AgentKind::ClaudeCode, "s1", &root, HookEventKind::ToolUsed(edit))).unwrap();
    let sid = out.session_id.unwrap();
    let files = store.session_files(&sid).unwrap();
    assert_eq!(files.len(), 2, "outside path is ignored");
    assert!(files.iter().any(|f| f.path == ".env" && f.sensitive));

    for _ in 0..2 {
        let run = ToolUse {
            tool: "Bash".into(),
            command: Some("cargo test --workspace".into()),
            success: Some(true),
            exit_code: Some(0),
            ..Default::default()
        };
        store.process_hook_event(&ev(AgentKind::ClaudeCode, "s1", &root, HookEventKind::ToolUsed(run))).unwrap();
    }
    let cmds = store
        .search_memories(&SearchQuery { project_id: Some(p.id.clone()), kinds: vec![MemoryKind::Command], ..Default::default() })
        .unwrap();
    assert_eq!(cmds.total, 1, "repeated command is reinforced, not duplicated");
    let m = &cmds.hits[0].memory;
    assert_eq!(m.certainty, Certainty::Verified);
    assert_eq!(m.title, "`cargo test --workspace`");
    assert_eq!(m.reinforced_count, 1);

    // Failed commands and commands containing secrets never become memories.
    let failed = ToolUse {
        tool: "Bash".into(),
        command: Some("cargo build".into()),
        success: Some(false),
        exit_code: Some(101),
        ..Default::default()
    };
    store.process_hook_event(&ev(AgentKind::ClaudeCode, "s1", &root, HookEventKind::ToolUsed(failed))).unwrap();
    let secret = ToolUse {
        tool: "Bash".into(),
        command: Some("API_TOKEN=abcdef123456789 npm test".into()),
        success: Some(true),
        ..Default::default()
    };
    store.process_hook_event(&ev(AgentKind::ClaudeCode, "s1", &root, HookEventKind::ToolUsed(secret))).unwrap();
    let cmds = store
        .search_memories(&SearchQuery { project_id: Some(p.id.clone()), kinds: vec![MemoryKind::Command], ..Default::default() })
        .unwrap();
    assert_eq!(cmds.total, 1);
    let detail = store.session_detail(&sid).unwrap();
    let joined: String = detail.events.iter().map(|e| e.summary.clone()).collect::<Vec<_>>().join("\n");
    assert!(!joined.contains("abcdef123456789"), "secrets are redacted in the activity log");
    assert!(joined.contains("exit 101"));
    assert_eq!(detail.session.commands_run, 4);
}

#[test]
fn prompts_are_not_stored_by_default() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    store
        .save_memory(NewMemory::new(&p.id, MemoryKind::Bug, "Login redirect loop", "Session cookie dropped on redirect"), &codex())
        .unwrap();
    let out = store
        .process_hook_event(&ev(
            AgentKind::ClaudeCode,
            "s1",
            &repo.path(),
            HookEventKind::PromptSubmitted { prompt: "fix the login redirect loop, my password is hunter2".into() },
        ))
        .unwrap();
    assert!(out.context.unwrap().contains("Login redirect loop"));
    let detail = store.session_detail(&out.session_id.unwrap()).unwrap();
    let all = serde_json::to_string(&detail.events).unwrap();
    assert!(!all.contains("hunter2"));
    assert!(all.contains("text not stored"));
    assert_eq!(detail.session.turns, 1);
}

#[test]
fn antigravity_pre_invocation_injects_once_per_prompt() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    store
        .save_memory(
            NewMemory::new(&p.id, MemoryKind::Decision, "Cache invalidation uses version stamps", "Each entry stores a version"),
            &claude(),
        )
        .unwrap();
    let mk = |input: &str| HookEvent {
        agent: AgentKind::AntigravityCli,
        external_session_id: "conv-1".into(),
        cwd: None,
        workspace_paths: vec![repo.path()],
        model: None,
        raw_event: "PreInvocation".into(),
        kind: HookEventKind::BeforeModel { last_user_input: Some(input.into()), invocation: Some(0) },
    };
    let first = store.process_hook_event(&mk("how does cache invalidation work with version stamps?")).unwrap();
    assert!(first.context.unwrap().contains("Cache invalidation"));
    let repeat = store.process_hook_event(&mk("how does cache invalidation work with version stamps?")).unwrap();
    assert!(repeat.context.is_none(), "same prompt, later model call: nothing new to inject");
    let detail = store.session_detail(&first.session_id.unwrap()).unwrap();
    assert_eq!(detail.session.turns, 1);
    assert_eq!(detail.session.agent, AgentKind::AntigravityCli);
}

#[test]
fn turn_end_and_session_end_capture_summary_and_commits() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    store.register_project(&repo.path(), None).unwrap();
    let root = repo.path();
    let s = store
        .process_hook_event(&ev(AgentKind::Codex, "c1", &root, HookEventKind::SessionStart { source: Some("startup".into()) }))
        .unwrap();
    let sid = s.session_id.unwrap();
    repo.write("src/new.rs", "fn new() {}\n");
    repo.commit_all("add new module");
    store
        .process_hook_event(&ev(
            AgentKind::Codex,
            "c1",
            &root,
            HookEventKind::TurnEnded {
                last_message: Some("Added src/new.rs. Token sk-ant-api03-aaaaaaaaaaaaaaaaaaaaaaaaaaaa used.".into()),
                fully_idle: None,
                reason: None,
            },
        ))
        .unwrap();
    store.process_hook_event(&ev(AgentKind::Codex, "c1", &root, HookEventKind::SessionEnded { reason: Some("other".into()) })).unwrap();
    let d = store.session_detail(&sid).unwrap();
    let summary = d.session.summary.unwrap();
    assert!(summary.starts_with("Added src/new.rs"));
    assert!(!summary.contains("aaaaaaaa"));
    assert!(d.session.ended_at.is_some());
    assert_ne!(d.session.start_commit, d.session.end_commit);
    assert!(d.events.iter().any(|e| e.kind == EventKind::Commits && e.summary.contains("add new module")));
    let activity = store.agent_activity().unwrap();
    assert_eq!(activity[0].agent, AgentKind::Codex);
}

#[test]
fn session_memories_are_settled_at_turn_end() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let root = repo.path();
    let s = store.process_hook_event(&ev(AgentKind::ClaudeCode, "k1", &root, HookEventKind::SessionStart { source: None })).unwrap();
    let sid = s.session_id.unwrap();
    // A memory recorded mid-session about a file the same session keeps editing.
    let mut m = fact(&p.id, "db.rs owns connection setup", "db.rs", &["src/db.rs"]);
    m.base_dir = Some(root.clone());
    let actor = Actor::agent(AgentKind::ClaudeCode, Origin::Mcp, Some(sid.clone()));
    let id = store.save_memory(m, &actor).unwrap().memory_id;
    repo.write("src/db.rs", "// edited later in the same session\n");
    store
        .process_hook_event(&ev(
            AgentKind::ClaudeCode,
            "k1",
            &root,
            HookEventKind::TurnEnded { last_message: None, fully_idle: None, reason: None },
        ))
        .unwrap();
    store.check_freshness(&p.id, None, &Actor::system()).unwrap();
    assert_eq!(store.get_memory(&id).unwrap().freshness, Freshness::Current);
}

/// Claude Code records a decision → Codex sees it at session start → Codex records progress →
/// Antigravity sees both. (Adapter-level parsing is covered in crates/integrations.)
#[test]
fn cross_agent_memory_flow() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), Some("Shared")).unwrap();
    let root = repo.path();

    let c = store
        .process_hook_event(&ev(AgentKind::ClaudeCode, "claude-1", &root, HookEventKind::SessionStart { source: Some("startup".into()) }))
        .unwrap();
    let claude_actor = Actor::agent(AgentKind::ClaudeCode, Origin::Mcp, c.session_id.clone());
    store
        .save_memory(
            NewMemory::new(&p.id, MemoryKind::Decision, "Adopt event sourcing for orders", "Orders are rebuilt from an append-only log"),
            &claude_actor,
        )
        .unwrap();

    let x = store
        .process_hook_event(&ev(AgentKind::Codex, "codex-1", &root, HookEventKind::SessionStart { source: Some("startup".into()) }))
        .unwrap();
    let codex_ctx = x.context.unwrap();
    assert!(codex_ctx.contains("Adopt event sourcing for orders"));
    assert!(codex_ctx.contains("Last activity: Claude Code"));
    let codex_actor = Actor::agent(AgentKind::Codex, Origin::Mcp, x.session_id.clone());
    let mut task = NewMemory::new(&p.id, MemoryKind::Task, "Implement order event log", "Append-only table plus replay");
    task.status = Some(MemoryStatus::InProgress);
    store.save_memory(task, &codex_actor).unwrap();

    let a = HookEvent {
        agent: AgentKind::AntigravityIde,
        external_session_id: "ag-1".into(),
        cwd: None,
        workspace_paths: vec![root.clone()],
        model: None,
        raw_event: "SessionStart".into(),
        kind: HookEventKind::SessionStart { source: None },
    };
    let ag_ctx = store.process_hook_event(&a).unwrap().context.unwrap();
    assert!(ag_ctx.contains("Adopt event sourcing for orders"));
    assert!(ag_ctx.contains("Implement order event log"));
    assert!(ag_ctx.contains("call_mcp_tool"), "Antigravity gets its own MCP access hint");
    let sessions = store.list_sessions(&Default::default()).unwrap();
    let agents: Vec<AgentKind> = sessions.iter().map(|s| s.agent).collect();
    assert!(agents.contains(&AgentKind::ClaudeCode) && agents.contains(&AgentKind::Codex) && agents.contains(&AgentKind::AntigravityIde));
}
