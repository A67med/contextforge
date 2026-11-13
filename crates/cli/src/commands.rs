//! Interactive CLI commands.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use contextforge_core::context::BriefOptions;
use contextforge_core::memories::{FileRefInput, MemoryPatch, NewMemory};
use contextforge_core::model::*;
use contextforge_core::projects::Project;
use contextforge_core::search::{MatchMode, SearchQuery, SortOrder};
use contextforge_core::sessions::SessionQuery;
use contextforge_core::{OpenOptions, Store, util};
use contextforge_integrations::{self as integ, Action, Env, Provider, Scope};
use serde::Serialize;

use crate::{Cli, Command, IntegrationArgs, IntegrationsCmd, KindArg, MemoryCmd, ProjectCmd, SettingsCmd};

type CmdResult = Result<(), String>;

fn open(cli: &Cli) -> Result<Store, String> {
    let r = match &cli.data_dir {
        Some(d) => Store::open_in(d, OpenOptions::interactive()),
        None => Store::open_default(OpenOptions::interactive()),
    };
    r.map_err(|e| format!("cannot open the ContextForge database: {e}"))
}

fn print_json<T: Serialize>(v: &T) -> CmdResult {
    println!("{}", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?);
    Ok(())
}

fn kind(k: KindArg) -> MemoryKind {
    match k {
        KindArg::Decision => MemoryKind::Decision,
        KindArg::Convention => MemoryKind::Convention,
        KindArg::Fact => MemoryKind::Fact,
        KindArg::Task => MemoryKind::Task,
        KindArg::Bug => MemoryKind::Bug,
        KindArg::Note => MemoryKind::Note,
        KindArg::Command => MemoryKind::Command,
        KindArg::Research => MemoryKind::Research,
    }
}

fn parse<T: std::str::FromStr<Err = contextforge_core::Error>>(s: &str) -> Result<T, String> {
    s.parse::<T>().map_err(|e| e.to_string())
}

/// Resolve `--project` (id or path), defaulting to the current directory.
fn project(store: &mut Store, arg: Option<&str>) -> Result<Project, String> {
    let target = match arg {
        Some(a) => a.to_string(),
        None => std::env::current_dir().map_err(|e| e.to_string())?.to_string_lossy().into_owned(),
    };
    if let Some(p) = store.find_project(&target).map_err(|e| e.to_string())? {
        return Ok(p);
    }
    if let Ok(Some(r)) = store.resolve_project(Path::new(&target), false) {
        return Ok(r.project);
    }
    Err(format!("'{target}' is not a registered project. Register it with: contextforge project add <path>"))
}

fn exe_path() -> PathBuf {
    std::env::current_exe().ok().map(|p| dunce::canonicalize(&p).unwrap_or(p)).unwrap_or_else(|| PathBuf::from("contextforge"))
}

fn data_dir(cli: &Cli) -> PathBuf {
    cli.data_dir.clone().unwrap_or_else(contextforge_core::data_dir)
}

fn confirm_or_explain(yes: bool, what: &str) -> CmdResult {
    if yes { Ok(()) } else { Err(format!("{what}\nRe-run with --yes to confirm.")) }
}

pub fn run(cli: &Cli) -> ExitCode {
    let result = dispatch(cli);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("contextforge: {msg}");
            ExitCode::from(2)
        }
    }
}

fn dispatch(cli: &Cli) -> CmdResult {
    match &cli.command {
        Command::Status => status(cli),
        Command::Project(cmd) => project_cmd(cli, cmd),
        Command::Memory(cmd) => memory_cmd(cli, cmd),
        Command::Context(a) => {
            let mut store = open(cli)?;
            let p = project(&mut store, a.project.as_deref())?;
            let mut opts =
                BriefOptions::new(a.budget.unwrap_or(store.settings().map_err(|e| e.to_string())?.context.startup_budget_tokens));
            opts.task = a.task.clone();
            let brief = store.build_brief(&p.id, &opts).map_err(|e| e.to_string())?;
            if cli.json {
                print_json(&brief)
            } else {
                println!("{}", brief.text);
                eprintln!("(~{} tokens, {} memories, {} omitted)", brief.token_estimate, brief.memory_ids.len(), brief.omitted);
                Ok(())
            }
        }
        Command::Handoff(a) => {
            let mut store = open(cli)?;
            let p = project(&mut store, a.project.as_deref())?;
            let target = a.to.as_deref().map(parse::<AgentKind>).transpose()?;
            let budget = a.budget.unwrap_or(store.settings().map_err(|e| e.to_string())?.context.handoff_budget_tokens);
            let h = store.generate_handoff(&p.id, target, budget, &Actor::user_cli(), chrono::Utc::now()).map_err(|e| e.to_string())?;
            if cli.json {
                print_json(&h)
            } else {
                println!("{}", h.markdown);
                Ok(())
            }
        }
        Command::Freshness { project: pa } => {
            let mut store = open(cli)?;
            let p = project(&mut store, pa.as_deref())?;
            let r = store.check_freshness(&p.id, None, &Actor::user_cli()).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&r);
            }
            println!(
                "Checked {} memories on {} in {} ms; {} changed, {} renames followed; {} flagged in total.",
                r.checked,
                r.branch.as_deref().unwrap_or("(no branch)"),
                r.duration_ms,
                r.changes.len(),
                r.renames_followed,
                r.flagged_total
            );
            for c in &r.changes {
                println!("  {} {} → {}  {}", c.memory_id, c.from, c.to, c.title);
                for reason in &c.reasons {
                    println!("      {reason}");
                }
            }
            Ok(())
        }
        Command::Sessions { project: pa, agent, limit } => {
            let mut store = open(cli)?;
            let project_id = match pa {
                Some(p) => Some(project(&mut store, Some(p))?.id),
                None => None,
            };
            let agent = agent.as_deref().map(parse::<AgentKind>).transpose()?;
            let list = store.list_sessions(&SessionQuery { project_id, agent, limit: *limit, offset: 0 }).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&list);
            }
            let now = chrono::Utc::now();
            for s in &list {
                println!(
                    "{}  {:<16} {:<10} files:{:<3} cmds:{:<3} {}{}",
                    s.id,
                    s.agent.label(),
                    util::relative_time(&s.last_activity_at, now),
                    s.files_touched,
                    s.commands_run,
                    if s.ended_at.is_some() { "[ended] " } else { "" },
                    s.summary.as_deref().map(|x| util::truncate_chars(&util::collapse_ws(x), 80)).unwrap_or_default()
                );
            }
            if list.is_empty() {
                println!("No sessions recorded yet.");
            }
            Ok(())
        }
        Command::Integrations(cmd) => integrations_cmd(cli, cmd),
        Command::Doctor => doctor(cli),
        Command::Export { project: pa, out } => {
            let mut store = open(cli)?;
            let pid = match pa {
                Some(p) => Some(project(&mut store, Some(p))?.id),
                None => None,
            };
            let n = store.export_to_file(pid.as_deref(), out).map_err(|e| e.to_string())?;
            println!("Exported {n} memories to {}", out.display());
            Ok(())
        }
        Command::Import { file } => {
            let mut store = open(cli)?;
            let r = store.import_from_file(file).map_err(|e| e.to_string())?;
            if cli.json {
                print_json(&r)
            } else {
                println!(
                    "Imported {} memories ({} skipped as already present), {} links; {} projects created, {} matched.",
                    r.memories_imported, r.memories_skipped, r.links_imported, r.projects_created, r.projects_matched
                );
                Ok(())
            }
        }
        Command::Backup { out } => {
            let store = open(cli)?;
            let path = match out {
                Some(p) => {
                    store.backup_to(p).map_err(|e| e.to_string())?;
                    p.clone()
                }
                None => store.backup_now().map_err(|e| e.to_string())?,
            };
            println!("Backup written to {}", path.display());
            Ok(())
        }
        Command::Restore { file, yes } => {
            confirm_or_explain(
                *yes,
                &format!("This replaces the current database with {} (a safety backup is made first).", file.display()),
            )?;
            let mut store = open(cli)?;
            let safety = store.restore_from(file).map_err(|e| e.to_string())?;
            println!("Restored. The previous state was saved to {}", safety.display());
            Ok(())
        }
        Command::Settings(cmd) => settings_cmd(cli, cmd),
        Command::Hook { .. } | Command::Mcp { .. } => unreachable!("handled in main"),
    }
}

fn status(cli: &Cli) -> CmdResult {
    let store = open(cli)?;
    let o = store.overview(None).map_err(|e| e.to_string())?;
    if cli.json {
        return print_json(&o);
    }
    println!("ContextForge {}", contextforge_core::VERSION);
    println!("  database: {} ({} KiB, schema v{})", o.database_path, o.database_bytes / 1024, o.schema_version);
    println!(
        "  projects: {}   memories: {}   open tasks: {}   open bugs: {}   need review: {}",
        o.projects, o.memories, o.open_tasks, o.open_bugs, o.needs_review
    );
    println!("  sessions: {}   handoffs: {}", o.sessions_total, o.handoffs);
    let now = chrono::Utc::now();
    for a in &o.agent_activity {
        println!(
            "  last {} event: {} ({}, {} total)",
            a.agent.label(),
            util::relative_time(&a.last_event_at, now),
            a.last_event_kind,
            a.event_count
        );
    }
    Ok(())
}

fn project_cmd(cli: &Cli, cmd: &ProjectCmd) -> CmdResult {
    let mut store = open(cli)?;
    match cmd {
        ProjectCmd::Add { path, name } => {
            let (p, created) = store.register_project(path, name.as_deref()).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&p);
            }
            println!("{} project {} — {} ({})", if created { "Registered" } else { "Already registered:" }, p.name, p.root_path, p.id);
            if p.is_git {
                println!(
                    "  Git repository detected{}",
                    p.default_branch.as_deref().map(|b| format!(" (default branch {b})")).unwrap_or_default()
                );
            }
            Ok(())
        }
        ProjectCmd::List => {
            let list = store.list_projects().map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&list);
            }
            if list.is_empty() {
                println!("No projects registered. Add one with: contextforge project add <path>");
            }
            for s in list {
                println!(
                    "{}  {:<24} memories:{:<4} tasks:{:<3} bugs:{:<3} review:{:<3} {}",
                    s.project.id, s.project.name, s.memory_count, s.open_tasks, s.open_bugs, s.needs_review, s.project.root_path
                );
            }
            Ok(())
        }
        ProjectCmd::Show { project: p } => {
            let p = project(&mut store, Some(p))?;
            let s = store.project_summary(&p.id).map_err(|e| e.to_string())?;
            print_json(&s)
        }
        ProjectCmd::Remove { project: p, yes } => {
            let p = project(&mut store, Some(p))?;
            confirm_or_explain(
                *yes,
                &format!("This deletes all ContextForge data for '{}' ({}). Project files are not touched.", p.name, p.root_path),
            )?;
            store.remove_project(&p.id).map_err(|e| e.to_string())?;
            println!("Removed {} from ContextForge.", p.name);
            Ok(())
        }
    }
}

fn print_memory_line(m: &Memory) {
    let flags = match m.freshness {
        Freshness::PotentiallyStale => " ⚠ stale?",
        Freshness::NeedsReview => " ⚠ review",
        Freshness::Superseded => " (superseded)",
        Freshness::Current => "",
    };
    println!("{}  {:<10} {:<11} {:<10} {}{}{}", m.id, m.kind, m.status, m.certainty, if m.pinned { "📌 " } else { "" }, m.title, flags);
}

fn memory_cmd(cli: &Cli, cmd: &MemoryCmd) -> CmdResult {
    let mut store = open(cli)?;
    let user = Actor::user_cli();
    match cmd {
        MemoryCmd::Add(a) => {
            let p = project(&mut store, a.project.as_deref())?;
            let mut m = NewMemory::new(p.id.clone(), kind(a.kind), a.title.clone(), a.content.clone());
            m.files = a.file.iter().map(FileRefInput::path).collect();
            m.commits = a.commit.clone();
            m.tags = a.tag.clone();
            m.certainty = a.certainty.as_deref().map(parse::<Certainty>).transpose()?;
            m.status = a.status.as_deref().map(parse::<MemoryStatus>).transpose()?;
            m.pinned = a.pin;
            m.base_dir = std::env::current_dir().ok();
            let o = store.save_memory(m, &user).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&o);
            }
            println!(
                "{} {}",
                if o.action == contextforge_core::memories::SaveAction::Created { "Created" } else { "Reinforced existing" },
                o.memory_id
            );
            for n in &o.notes {
                println!("  note: {n}");
            }
            Ok(())
        }
        MemoryCmd::List { project: pa, kind: kinds, status, freshness, limit } => {
            let p = project(&mut store, pa.as_deref())?;
            let q = SearchQuery {
                project_id: Some(p.id),
                kinds: kinds.iter().map(|k| kind(*k)).collect(),
                statuses: status.iter().map(|s| parse::<MemoryStatus>(s)).collect::<Result<_, _>>()?,
                freshness: freshness.iter().map(|s| parse::<Freshness>(s)).collect::<Result<_, _>>()?,
                limit: *limit,
                sort: SortOrder::Updated,
                ..Default::default()
            };
            let r = store.search_memories(&q).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&r);
            }
            for h in &r.hits {
                print_memory_line(&h.memory);
            }
            println!("({} of {} shown)", r.hits.len(), r.total);
            Ok(())
        }
        MemoryCmd::Search { query, project: pa, any, limit } => {
            let p = project(&mut store, pa.as_deref())?;
            let q = SearchQuery {
                project_id: Some(p.id),
                text: Some(query.clone()),
                mode: if *any { MatchMode::Any } else { MatchMode::All },
                limit: *limit,
                ..Default::default()
            };
            let r = store.search_memories(&q).map_err(|e| e.to_string())?;
            if cli.json {
                return print_json(&r);
            }
            for h in &r.hits {
                print_memory_line(&h.memory);
                if let Some(s) = &h.snippet {
                    println!("      {}", util::collapse_ws(s));
                }
            }
            println!("({} of {} results)", r.hits.len(), r.total);
            Ok(())
        }
        MemoryCmd::Show { id } => {
            let d = store.get_memory_detail(id).map_err(|e| e.to_string())?;
            print_json(&d)
        }
        MemoryCmd::Edit { id, title, content, status, certainty } => {
            let patch = MemoryPatch {
                title: title.clone(),
                content: content.clone(),
                status: status.as_deref().map(parse::<MemoryStatus>).transpose()?,
                certainty: certainty.as_deref().map(parse::<Certainty>).transpose()?,
                reason: Some("edited from the CLI".into()),
                ..Default::default()
            };
            let o = store.update_memory(id, patch, &user).map_err(|e| e.to_string())?;
            println!("Updated {} ({})", o.memory_id, o.notes.join("; "));
            Ok(())
        }
        MemoryCmd::Pin { id } => store.set_pinned(id, true, &user).map_err(|e| e.to_string()).map(|_| println!("Pinned {id}")),
        MemoryCmd::Unpin { id } => store.set_pinned(id, false, &user).map_err(|e| e.to_string()).map(|_| println!("Unpinned {id}")),
        MemoryCmd::Supersede { id, title, content, reason } => {
            let old = store.get_memory(id).map_err(|e| e.to_string())?;
            let new = NewMemory::new(old.project_id.clone(), old.kind, title.clone(), content.clone());
            let o = store.supersede_memory(id, new, reason, &user).map_err(|e| e.to_string())?;
            println!("{} supersedes {id}", o.memory_id);
            Ok(())
        }
        MemoryCmd::Revalidate { id, note } => {
            store.revalidate_memory(id, note.as_deref(), vec![], &user).map_err(|e| e.to_string())?;
            println!("Revalidated {id}: snapshots refreshed and marked current");
            Ok(())
        }
        MemoryCmd::Delete { id, yes } => {
            let m = store.get_memory(id).map_err(|e| e.to_string())?;
            confirm_or_explain(*yes, &format!("This permanently deletes '{}' and its provenance.", m.title))?;
            store.delete_memory(id, &user).map_err(|e| e.to_string())?;
            println!("Deleted {id}");
            Ok(())
        }
    }
}

fn providers(arg: &str) -> Result<Vec<Provider>, String> {
    if arg.eq_ignore_ascii_case("all") {
        return Ok(Provider::ALL.to_vec());
    }
    arg.split(',').map(|p| p.parse::<Provider>().map_err(|e| e.to_string())).collect()
}

fn scope_of(project: &Option<PathBuf>) -> Result<Scope, String> {
    match project {
        Some(p) => {
            let c = dunce::canonicalize(p).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok(Scope::Project(c))
        }
        None => Ok(Scope::User),
    }
}

fn integrations_cmd(cli: &Cli, cmd: &IntegrationsCmd) -> CmdResult {
    let env = Env::detect(&data_dir(cli));
    let exe = exe_path();
    match cmd {
        IntegrationsCmd::Status { project } => {
            let scope = scope_of(project)?;
            let installs = integ::detect::detect_all(&env);
            let statuses: Vec<_> = Provider::ALL.iter().map(|p| integ::status(*p, &scope, &exe, &env, &installs)).collect();
            if cli.json {
                return print_json(&statuses);
            }
            println!("ContextForge executable: {}", exe.display());
            for s in &statuses {
                println!("\n{} — {:?}", s.label, s.overall());
                for i in &s.installations {
                    println!(
                        "  {:<34} {}",
                        i.label,
                        if i.installed {
                            format!("installed {}", i.version.as_deref().unwrap_or("(version unknown)"))
                        } else {
                            "not found".into()
                        }
                    );
                }
                println!("  MCP:   {:?} — {}", s.mcp.state, s.mcp.detail);
                println!("  Hooks: {:?} — {}", s.hooks.state, s.hooks.detail);
                if let Some(t) = &s.trust {
                    println!("  Trust: {:?} — {}", t.state, t.detail);
                }
            }
            Ok(())
        }
        IntegrationsCmd::Install(a) => apply_integration(cli, &env, &exe, Action::Install, a),
        IntegrationsCmd::Uninstall(a) => apply_integration(cli, &env, &exe, Action::Uninstall, a),
        IntegrationsCmd::Repair(a) => apply_integration(cli, &env, &exe, Action::Repair, a),
        IntegrationsCmd::Backups => {
            let list = integ::plan::list_backups(&env);
            if cli.json {
                return print_json(&list);
            }
            if list.is_empty() {
                println!("No configuration backups yet.");
            }
            for b in list {
                println!("{}  {:<11} {:<9} {} file(s)", b.id, b.provider.label(), b.action.as_str(), b.files.len());
            }
            Ok(())
        }
        IntegrationsCmd::Restore { backup_id, force } => {
            let r = integ::plan::restore_backup(&env, backup_id, *force).map_err(|e| e.to_string())?;
            for p in &r.restored {
                println!("restored {}", p.display());
            }
            for n in &r.notes {
                println!("note: {n}");
            }
            Ok(())
        }
    }
}

fn apply_integration(cli: &Cli, env: &Env, exe: &Path, action: Action, a: &IntegrationArgs) -> CmdResult {
    let scope = scope_of(&a.project)?;
    for provider in providers(&a.provider)? {
        let plan = integ::plan(provider, action, &scope, exe, env).map_err(|e| e.to_string())?;
        println!(
            "== {} {} ({})",
            action.as_str(),
            provider.label(),
            match &scope {
                Scope::User => "user".to_string(),
                Scope::Project(p) => p.display().to_string(),
            }
        );
        for w in &plan.warnings {
            println!("warning: {w}");
        }
        if plan.is_noop() {
            println!("Nothing to change.");
            continue;
        }
        print!("{}", plan.preview());
        if a.dry_run {
            continue;
        }
        if !a.yes {
            return Err("review the changes above; re-run with --yes to apply them (or --dry-run to only preview)".into());
        }
        let report = integ::plan::apply(&plan, env).map_err(|e| e.to_string())?;
        if cli.json {
            print_json(&report)?;
        }
        println!("Applied. Backup id: {}", report.backup_id.as_deref().unwrap_or("-"));
        for n in &report.notes {
            println!("note: {n}");
        }
    }
    Ok(())
}

fn doctor(cli: &Cli) -> CmdResult {
    let exe = exe_path();
    let dd = data_dir(cli);
    let env = Env::detect(&dd);
    let mut checks = vec![integ::doctor::check_mcp(&exe, Some(&dd))];
    for p in Provider::ALL {
        checks.push(integ::doctor::check_hook(&exe, p, Some(&dd)));
    }
    let store = open(cli)?;
    let integrity: String = store.conn().query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    checks.push(integ::doctor::CheckResult { name: "Database integrity".into(), ok: integrity == "ok", detail: integrity, duration_ms: 0 });
    checks.push(integ::doctor::CheckResult {
        name: "Git available".into(),
        ok: contextforge_core::git::available(),
        detail: "required for branch, commit and rename awareness".into(),
        duration_ms: 0,
    });
    let installs = integ::detect::detect_all(&env);
    if cli.json {
        let statuses: Vec<_> = Provider::ALL.iter().map(|p| integ::status(*p, &Scope::User, &exe, &env, &installs)).collect();
        return print_json(&serde_json::json!({ "checks": checks, "integrations": statuses }));
    }
    for c in &checks {
        println!("[{}] {} — {}", if c.ok { " ok " } else { "FAIL" }, c.name, c.detail);
    }
    if checks.iter().all(|c| c.ok) { Ok(()) } else { Err("some checks failed".into()) }
}

fn settings_cmd(cli: &Cli, cmd: &SettingsCmd) -> CmdResult {
    let mut store = open(cli)?;
    match cmd {
        SettingsCmd::Show => print_json(&store.settings().map_err(|e| e.to_string())?),
        SettingsCmd::Set { key, value } => {
            let current = store.settings().map_err(|e| e.to_string())?;
            let mut json = serde_json::to_value(&current).map_err(|e| e.to_string())?;
            let parsed: serde_json::Value = serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            let mut slot = &mut json;
            for part in key.split('.') {
                slot = slot.get_mut(part).ok_or_else(|| format!("unknown setting '{key}'"))?;
            }
            *slot = parsed;
            let new: contextforge_core::Settings = serde_json::from_value(json).map_err(|e| format!("invalid value for {key}: {e}"))?;
            store.save_settings(&new).map_err(|e| e.to_string())?;
            println!("{key} updated");
            Ok(())
        }
    }
}
