//! Git-aware freshness scenarios against real temporary repositories.

mod common;

use common::*;
use contextforge_core::memories::NewMemory;
use contextforge_core::model::*;

fn setup() -> (tempfile::TempDir, contextforge_core::Store, TestRepo, String) {
    let (d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    (d, store, repo, p.id)
}

fn freshness(store: &contextforge_core::Store, id: &str) -> Freshness {
    store.get_memory(id).unwrap().freshness
}

#[test]
fn uncommitted_modification_flags_and_revert_clears() {
    let (_d, mut store, repo, pid) = setup();
    let id =
        store.save_memory(fact(&pid, "db.rs opens WAL", "Connection setup lives in db.rs", &["src/db.rs"]), &claude()).unwrap().memory_id;
    let unrelated = store.save_memory(fact(&pid, "api.rs routes", "Routes live in api.rs", &["src/api.rs"]), &claude()).unwrap().memory_id;
    let original = std::fs::read_to_string(repo.path().join("src/db.rs")).unwrap();

    repo.write("src/db.rs", &format!("{original}// new line\n"));
    let report = store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::PotentiallyStale);
    assert_eq!(freshness(&store, &unrelated), Freshness::Current, "unrelated memories are not invalidated");
    assert_eq!(report.changes.len(), 1);
    let m = store.get_memory(&id).unwrap();
    assert!(m.stale_reason.as_deref().unwrap().contains("uncommitted"), "{:?}", m.stale_reason);

    // Restoring the content clears the automatic flag.
    repo.write("src/db.rs", &original);
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current);
    let d = store.get_memory_detail(&id).unwrap();
    assert!(d.findings.iter().any(|f| f.resolution.as_deref() == Some("auto_cleared")));
    assert_eq!(d.memory.content, "Connection setup lives in db.rs", "history and content are untouched");
}

#[test]
fn line_ending_only_changes_are_not_modifications() {
    let (_d, mut store, repo, pid) = setup();
    let id = store.save_memory(fact(&pid, "API file", "api", &["src/api.rs"]), &claude()).unwrap().memory_id;
    let crlf = std::fs::read_to_string(repo.path().join("src/api.rs")).unwrap().replace('\n', "\r\n");
    repo.write("src/api.rs", &crlf);
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current);
}

#[test]
fn committed_change_then_revalidate() {
    let (_d, mut store, repo, pid) = setup();
    let id = store.save_memory(fact(&pid, "API returns JSON", "api.rs returns JSON", &["src/api.rs"]), &codex()).unwrap().memory_id;
    repo.write("src/api.rs", &numbered_lines("api v2", 40));
    repo.commit_all("rewrite api");
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::PotentiallyStale);

    store.revalidate_memory(&id, Some("Checked api.rs: still returns JSON"), vec![], &codex()).unwrap();
    let d = store.get_memory_detail(&id).unwrap();
    assert_eq!(d.memory.freshness, Freshness::Current);
    assert_eq!(d.memory.validated_commit.as_deref(), Some(repo.head().as_str()));
    assert!(d.evidence.iter().any(|e| e.kind == EvidenceKind::AgentCheck));
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current, "snapshot was refreshed by revalidation");
}

#[test]
fn deleted_file_needs_review() {
    let (_d, mut store, repo, pid) = setup();
    let id = store.save_memory(fact(&pid, "API module", "api.rs exists", &["src/api.rs"]), &claude()).unwrap().memory_id;
    repo.remove("src/api.rs");
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    let d = store.get_memory_detail(&id).unwrap();
    assert_eq!(d.memory.freshness, Freshness::NeedsReview);
    assert!(d.findings.iter().any(|f| f.kind == FindingKind::FileDeleted && f.path.as_deref() == Some("src/api.rs")));
}

#[test]
fn renames_are_followed() {
    let (_d, mut store, repo, pid) = setup();
    let id = store.save_memory(fact(&pid, "Storage layer", "storage code", &["src/db.rs"]), &claude()).unwrap().memory_id;
    repo.git(&["mv", "src/db.rs", "src/storage.rs"]);
    repo.commit_all("rename db to storage");
    let report = store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(report.renames_followed, 1);
    let d = store.get_memory_detail(&id).unwrap();
    assert_eq!(d.memory.freshness, Freshness::Current, "pure rename keeps the memory current");
    assert_eq!(d.files[0].path, "src/storage.rs");
    assert!(d.findings.iter().any(|f| f.kind == FindingKind::FileRenamed && f.resolution.as_deref() == Some("auto_followed")));
}

#[test]
fn branch_switching_is_reversible() {
    let (_d, mut store, repo, pid) = setup();
    let id = store.save_memory(fact(&pid, "db.rs v1 behaviour", "v1", &["src/db.rs"]), &claude()).unwrap().memory_id;
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/db.rs", &numbered_lines("feature db", 40));
    repo.commit_all("feature change");
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::PotentiallyStale);
    let finding = store.get_memory_detail(&id).unwrap().findings.into_iter().find(|f| f.resolved_at.is_none()).unwrap();
    assert_eq!(finding.branch.as_deref(), Some("feature"));

    repo.git(&["checkout", "-q", "main"]);
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current, "back on main the snapshot matches again");
}

#[test]
fn branch_scoped_memories_are_only_checked_on_their_branch() {
    let (_d, mut store, repo, pid) = setup();
    let mut m = fact(&pid, "Feature flag wiring", "only on feature", &["src/api.rs"]);
    m.branch = Some("feature".into());
    let id = store.save_memory(m, &claude()).unwrap().memory_id;
    repo.write("src/api.rs", "changed\n");
    let report = store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(report.checked, 0);
    assert_eq!(freshness(&store, &id), Freshness::Current);
}

#[test]
fn rewritten_history_flags_missing_commits() {
    let (_d, mut store, repo, pid) = setup();
    repo.write("src/fix.rs", "fn fix() {}\n");
    let sha = repo.commit_all("fix bug 42");
    let mut bug = NewMemory::new(&pid, MemoryKind::Bug, "Bug 42 crash on startup", "Fixed by guarding the config load");
    bug.status = Some(MemoryStatus::Resolved);
    bug.commits = vec![sha.clone()];
    let id = store.save_memory(bug, &codex()).unwrap().memory_id;
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current);

    // Simulate a rebase that drops the commit, then prune it from the object database.
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.git(&["update-ref", "-d", "ORIG_HEAD"]);
    repo.git(&["reflog", "expire", "--expire=now", "--all"]);
    repo.git(&["gc", "-q", "--prune=now"]);
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    let d = store.get_memory_detail(&id).unwrap();
    assert_eq!(d.memory.freshness, Freshness::NeedsReview);
    assert!(d.findings.iter().any(|f| f.kind == FindingKind::CommitMissing));
    assert_eq!(d.commits, vec![sha], "the original reference is preserved for inspection");
}

#[test]
fn decisions_tolerate_small_edits() {
    let (_d, mut store, repo, pid) = setup();
    let id = store
        .save_memory(
            {
                let mut d = NewMemory::new(
                    &pid,
                    MemoryKind::Decision,
                    "Use one connection per process",
                    "Each process opens its own SQLite connection.",
                );
                d.files = vec![contextforge_core::memories::FileRefInput::path("src/db.rs")];
                d
            },
            &claude(),
        )
        .unwrap()
        .memory_id;
    let small = std::fs::read_to_string(repo.path().join("src/db.rs")).unwrap().replacen("line 3", "line three", 1);
    repo.write("src/db.rs", &small);
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::Current, "a one-line edit does not invalidate a decision");
    repo.write("src/db.rs", &numbered_lines("completely different", 60));
    store.check_freshness(&pid, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::PotentiallyStale);
}

#[test]
fn worktrees_map_to_the_same_project() {
    let (_d, mut store, repo, pid) = setup();
    let wt_parent = tempfile::tempdir().unwrap();
    let wt = wt_parent.path().join("feature-wt");
    repo.git(&["worktree", "add", "-q", "-b", "wt-branch", &wt.to_string_lossy()]);
    let resolved = store.resolve_project(&wt.join("src"), false).unwrap().expect("worktree resolves");
    assert_eq!(resolved.project.id, pid);
    assert_eq!(
        contextforge_core::paths::path_key(&resolved.checkout_root),
        contextforge_core::paths::path_key(&contextforge_core::paths::canonical(&wt))
    );
    let p = store.get_project(&pid).unwrap();
    assert!(p.roots.iter().any(|r| r.kind == "worktree" && r.branch.as_deref() == Some("wt-branch")));
    // Registering the worktree path directly attaches it instead of creating a second project.
    let (again, created) = store.register_project(&wt, None).unwrap();
    assert!(!created);
    assert_eq!(again.id, pid);
    repo.git(&["worktree", "remove", "--force", &wt.to_string_lossy()]);
}

#[test]
fn non_git_projects_still_detect_changes() {
    let (_d, mut store) = store();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.md"), "v1").unwrap();
    let (p, _) = store.register_project(dir.path(), None).unwrap();
    assert!(!p.is_git);
    let id = store.save_memory(fact(&p.id, "Notes", "notes file", &["notes.md"]), &claude()).unwrap().memory_id;
    std::fs::write(dir.path().join("notes.md"), "v2").unwrap();
    store.check_freshness(&p.id, None, &Actor::system()).unwrap();
    assert_eq!(freshness(&store, &id), Freshness::PotentiallyStale);
}

#[test]
fn missing_checkout_is_reported_not_mass_invalidated() {
    let (_d, mut store) = store();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let root = dir.path().to_path_buf();
    let (p, _) = store.register_project(&root, None).unwrap();
    let id = store.save_memory(fact(&p.id, "A", "a", &["a.txt"]), &claude()).unwrap().memory_id;
    drop(dir);
    assert!(store.check_freshness(&p.id, None, &Actor::system()).is_err());
    assert_eq!(freshness(&store, &id), Freshness::Current);
}
