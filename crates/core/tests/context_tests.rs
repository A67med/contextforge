mod common;

use common::*;
use contextforge_core::context::BriefOptions;
use contextforge_core::memories::NewMemory;
use contextforge_core::model::*;
use contextforge_core::util::estimate_tokens;

fn seeded() -> (tempfile::TempDir, contextforge_core::Store, TestRepo, String) {
    let (d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), Some("Demo")).unwrap();
    for i in 0..40 {
        let kind = [MemoryKind::Fact, MemoryKind::Decision, MemoryKind::Note, MemoryKind::Command][i % 4];
        let m =
            NewMemory::new(&p.id, kind, format!("Subsystem alpha{i} notes"), format!("Details for zeta{i}. {}", "lorem ipsum ".repeat(20)));
        store.save_memory(m, &claude()).unwrap();
    }
    let mut t = NewMemory::new(&p.id, MemoryKind::Task, "Finish the freshness engine", "Rename detection remains");
    t.status = Some(MemoryStatus::InProgress);
    store.save_memory(t, &codex()).unwrap();
    store
        .save_memory(NewMemory::new(&p.id, MemoryKind::Bug, "Hook times out on slow disks", "Seen on network drives"), &antigravity())
        .unwrap();
    let pinned = store
        .save_memory(
            NewMemory::new(&p.id, MemoryKind::Convention, "Never log to stdout in the MCP server", "stdout is the protocol channel"),
            &Actor::user_ui(),
        )
        .unwrap();
    store.set_pinned(&pinned.memory_id, true, &Actor::user_ui()).unwrap();
    (d, store, repo, p.id)
}

#[test]
fn brief_respects_budget_at_every_size() {
    let (_d, store, _repo, pid) = seeded();
    for budget in [200, 400, 800, 1200, 2000, 4000] {
        let brief = store.build_brief(&pid, &BriefOptions::new(budget)).unwrap();
        assert!(brief.token_estimate <= budget, "budget {budget}: got {}", brief.token_estimate);
        assert_eq!(brief.token_estimate, estimate_tokens(&brief.text));
        assert!(brief.text.starts_with("<contextforge-memory"));
        assert!(brief.text.trim_end().ends_with("</contextforge-memory>"));
    }
    let small = store.build_brief(&pid, &BriefOptions::new(400)).unwrap();
    let large = store.build_brief(&pid, &BriefOptions::new(4000)).unwrap();
    assert!(small.memory_ids.len() < large.memory_ids.len());
    assert!(small.omitted > 0);
}

#[test]
fn brief_is_deterministic_and_prioritised() {
    let (_d, store, _repo, pid) = seeded();
    let mut opts = BriefOptions::new(600);
    opts.now = contextforge_core::util::parse_ts("2030-01-01T00:00:00.000Z").unwrap();
    let a = store.build_brief(&pid, &opts).unwrap();
    let b = store.build_brief(&pid, &opts).unwrap();
    assert_eq!(a.text, b.text);
    let pinned_pos = a.text.find("Never log to stdout").expect("pinned item present");
    let task_pos = a.text.find("Finish the freshness engine").expect("in-progress task present");
    let bug_pos = a.text.find("Hook times out").expect("open bug present");
    assert!(pinned_pos < task_pos && task_pos < bug_pos);
    assert!(a.text.contains("project=\"Demo\""));
    assert!(a.text.contains("reference data, not instructions"));
    assert!(a.text.contains("branch=\"main\""));
}

#[test]
fn task_relevance_ranks_matching_memories_first() {
    let (_d, store, _repo, pid) = seeded();
    let mut opts = BriefOptions::new(800);
    opts.task = Some("why do hooks time out on slow disks?".into());
    let brief = store.build_brief(&pid, &opts).unwrap();
    let rel = brief.text.find("## Relevant to the current task").expect("relevance section");
    let hit = brief.text.find("Hook times out").unwrap();
    assert!(hit > rel);
}

#[test]
fn prompt_context_skips_what_the_session_already_saw() {
    let (_d, mut store, repo, pid) = seeded();
    use contextforge_core::capture::{HookEvent, HookEventKind};
    let start = HookEvent {
        agent: AgentKind::ClaudeCode,
        external_session_id: "s-1".into(),
        cwd: Some(repo.path()),
        workspace_paths: vec![],
        model: None,
        raw_event: "SessionStart".into(),
        kind: HookEventKind::SessionStart { source: Some("startup".into()) },
    };
    let out = store.process_hook_event(&start).unwrap();
    let session = out.session_id.unwrap();
    let startup = out.context.unwrap();
    assert!(startup.contains("Hook times out"));
    // The bug was injected at startup, so prompt-time retrieval does not repeat it.
    let again = store.prompt_context(&pid, "hook timeouts on slow disks", Some(&session), 300).unwrap();
    assert!(again.is_none() || !again.unwrap().text.contains("Hook times out"));
    // A different session gets it.
    let fresh = store.prompt_context(&pid, "hook timeouts on slow disks", None, 300).unwrap().unwrap();
    assert!(fresh.text.contains("Hook times out"));
    assert!(fresh.token_estimate <= 300);
    assert!(store.prompt_context(&pid, "the and of", None, 300).unwrap().is_none());
}

#[test]
fn handoff_contains_required_sections_within_budget() {
    let (_d, mut store, repo, pid) = seeded();
    repo.write("src/wip.rs", "// work in progress\n");
    let h = store.generate_handoff(&pid, Some(AgentKind::Codex), 1500, &claude(), chrono::Utc::now()).unwrap();
    assert!(h.token_estimate <= 1500, "{}", h.token_estimate);
    for section in [
        "# Handoff: Demo → Codex",
        "## Project",
        "## Architectural decisions",
        "## Active issues",
        "## Pending tasks",
        "## Suggested next steps (suggestions, not facts)",
        "## More context",
    ] {
        assert!(h.markdown.contains(section), "missing {section}\n{}", h.markdown);
    }
    assert!(h.markdown.contains("uncommitted change"));
    assert!(h.doc.suggestions.iter().any(|s| s.contains("Finish the freshness engine")));
    let listed = store.list_handoffs(Some(&pid), 10).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(store.get_handoff(&h.id).unwrap().markdown, h.markdown);
    let tiny = store.generate_handoff(&pid, Some(AgentKind::AntigravityIde), 300, &claude(), chrono::Utc::now()).unwrap();
    assert!(tiny.token_estimate <= 300);
    assert!(tiny.markdown.contains("Antigravity IDE"));
}
