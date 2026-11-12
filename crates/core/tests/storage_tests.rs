//! Concurrency, persistence, export/import and backup/restore.

mod common;

use common::*;
use contextforge_core::memories::NewMemory;
use contextforge_core::model::*;
use contextforge_core::search::SearchQuery;
use contextforge_core::{OpenOptions, Store};

#[test]
fn concurrent_writers_from_separate_connections() {
    let (dir, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let path = dir.path().to_path_buf();
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let path = path.clone();
            let pid = p.id.clone();
            std::thread::spawn(move || {
                let mut s = Store::open_in(&path, OpenOptions::interactive()).unwrap();
                let agent = [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::AntigravityCli][t % 3];
                for i in 0..25 {
                    let m = NewMemory::new(
                        &pid,
                        MemoryKind::Note,
                        format!("thread{t} note{i} uniq{t}x{i}"),
                        format!("body t{t} i{i} payload{t}{i}"),
                    );
                    s.save_memory(m, &Actor::agent(agent, Origin::Mcp, None)).unwrap();
                    // interleave reads
                    let _ = s
                        .search_memories(&SearchQuery {
                            project_id: Some(pid.clone()),
                            text: Some(format!("thread{t}")),
                            ..Default::default()
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("writer thread panicked");
    }
    let total = store.search_memories(&SearchQuery { project_id: Some(p.id.clone()), limit: 1, ..Default::default() }).unwrap().total;
    assert_eq!(total, 200);
    let integrity: String = store.conn().query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(integrity, "ok");
    let fts: i64 =
        store.conn().query_row("SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'uniq0x0'", [], |r| r.get(0)).unwrap();
    assert_eq!(fts, 1, "FTS index stays consistent under concurrency");
}

#[test]
fn export_import_round_trip_preserves_provenance() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), Some("Exported")).unwrap();
    let a = store.save_memory(fact(&p.id, "Uses Postgres 16", "Prod database is Postgres 16", &["src/db.rs"]), &claude()).unwrap();
    let b = store
        .supersede_memory(&a.memory_id, fact(&p.id, "Uses Postgres 17", "Upgraded to 17", &["src/db.rs"]), "upgrade", &codex())
        .unwrap();
    let file = tempfile::tempdir().unwrap();
    let out = file.path().join("export.json");
    assert_eq!(store.export_to_file(None, &out).unwrap(), 2);

    let (_d2, mut other) = common::store();
    let report = other.import_from_file(&out).unwrap();
    assert_eq!(report.memories_imported, 2);
    assert_eq!(report.projects_created, 1);
    let old = other.get_memory_detail(&a.memory_id).unwrap();
    assert_eq!(old.memory.freshness, Freshness::Superseded);
    assert_eq!(old.memory.superseded_by.as_deref(), Some(b.memory_id.as_str()));
    assert_eq!(old.memory.source_agent, AgentKind::ClaudeCode);
    assert!(old.links.iter().any(|l| l.relation == LinkRelation::Supersedes));
    assert_eq!(old.files[0].path, "src/db.rs");
    // Importing again skips existing records.
    let again = other.import_from_file(&out).unwrap();
    assert_eq!(again.memories_skipped, 2);
    assert_eq!(again.memories_imported, 0);
    // Search works on imported data.
    let r = other.search_memories(&SearchQuery { text: Some("postgres".into()), ..Default::default() }).unwrap();
    assert_eq!(r.total, 1);
}

#[test]
fn backup_and_restore() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let keep = store.save_memory(fact(&p.id, "Before backup", "kept", &[]), &claude()).unwrap().memory_id;
    let backup = store.backup_now().unwrap();
    assert!(backup.exists());
    let lost = store.save_memory(fact(&p.id, "After backup", "will be rolled back", &[]), &claude()).unwrap().memory_id;
    let safety = store.restore_from(&backup).unwrap();
    assert!(safety.exists(), "current state is saved before restoring");
    assert!(store.get_memory(&keep).is_ok());
    assert!(store.get_memory(&lost).is_err());
    let r = store.search_memories(&SearchQuery { text: Some("backup".into()), ..Default::default() }).unwrap();
    assert_eq!(r.total, 1);
    assert!(store.list_backups().unwrap().len() >= 2);
    // A non-ContextForge file is rejected.
    let junk = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(junk.path(), b"not a database").unwrap();
    assert!(store.restore_from(junk.path()).is_err());
    assert!(store.get_memory(&keep).is_ok());
}

#[test]
fn event_retention_prunes_only_events() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    let id = store.save_memory(fact(&p.id, "Retained", "memories are not pruned", &[]), &claude()).unwrap().memory_id;
    store.conn().execute("UPDATE events SET occurred_at = '2000-01-01T00:00:00.000Z'", []).unwrap();
    let pruned = store.prune_events(30).unwrap();
    assert!(pruned >= 1);
    assert!(store.get_memory(&id).is_ok());
}

#[test]
fn overview_counts_real_data() {
    let (_d, mut store) = store();
    let repo = TestRepo::new();
    let (p, _) = store.register_project(&repo.path(), None).unwrap();
    store.save_memory(NewMemory::new(&p.id, MemoryKind::Task, "Open task", ""), &claude()).unwrap();
    store.save_memory(NewMemory::new(&p.id, MemoryKind::Bug, "Open bug", ""), &codex()).unwrap();
    let o = store.overview(None).unwrap();
    assert_eq!(o.projects, 1);
    assert_eq!(o.memories, 2);
    assert_eq!(o.open_tasks, 1);
    assert_eq!(o.open_bugs, 1);
    assert!(o.database_bytes > 0);
    assert!(o.recent_events.len() >= 2);
}
