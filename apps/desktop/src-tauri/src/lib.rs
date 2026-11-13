//! ContextForge desktop backend: Tauri commands over the shared core.
//!
//! The app opens the same SQLite database as the CLI, hooks and MCP servers (WAL mode makes this safe) and
//! does all blocking work on a worker thread so the UI never stalls. The UI polls `data_version` to refresh
//! when another process (an agent hook or MCP server) writes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use contextforge_core::context::{BriefOptions, ContextBrief};
use contextforge_core::freshness::FreshnessReport;
use contextforge_core::handoff::{Handoff, HandoffSummary};
use contextforge_core::memories::{EvidenceInput, FileRefInput, MemoryPatch, NewMemory, SaveOutcome};
use contextforge_core::model::*;
use contextforge_core::projects::{Project, ProjectSettings, ProjectSummary};
use contextforge_core::search::{SearchQuery, SearchResults};
use contextforge_core::sessions::{SessionDetail, SessionQuery};
use contextforge_core::stats::Overview;
use contextforge_core::{OpenOptions, Settings, Store};
use contextforge_integrations::detect::AgentInstall;
use contextforge_integrations::doctor::CheckResult;
use contextforge_integrations::plan::{ApplyReport, BackupRecord, RestoreReport};
use contextforge_integrations::{self as integ, Action, Env, InstallPlan, Provider, ProviderStatus, Scope};
use serde::{Deserialize, Serialize};
use tauri::State;

struct AppState {
    store: Mutex<Store>,
    data_dir: PathBuf,
    env: Env,
    installs: Mutex<Option<Vec<AgentInstall>>>,
}

type Shared = Arc<AppState>;
type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(
    state: &State<'_, Shared>,
    f: impl FnOnce(&AppState, &mut Store) -> Result<T, String> + Send + 'static,
) -> CmdResult<T> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = st.store.lock().unwrap_or_else(|p| p.into_inner());
        f(&st, &mut guard)
    })
    .await
    .map_err(|e| format!("worker failed: {e}"))?
}

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// The bundled `contextforge` CLI (installed next to the app), or a development build.
fn cli_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CONTEXTFORGE_CLI").map(PathBuf::from).filter(|p| p.is_file()) {
        return p;
    }
    let exe_name = if cfg!(windows) { "contextforge.exe" } else { "contextforge" };
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let side = dir.join(exe_name);
        if side.is_file() {
            return dunce::canonicalize(&side).unwrap_or(side);
        }
        // Development: target/<profile>/contextforge(.exe) next to the desktop binary.
        for up in dir.ancestors().take(4) {
            for profile in ["debug", "release"] {
                let c = up.join(profile).join(exe_name);
                if c.is_file() {
                    return dunce::canonicalize(&c).unwrap_or(c);
                }
            }
        }
    }
    PathBuf::from(exe_name)
}

fn user() -> Actor {
    Actor::user_ui()
}

// ---------------------------------------------------------------- overview & projects

#[tauri::command]
async fn overview(state: State<'_, Shared>, project_id: Option<String>) -> CmdResult<Overview> {
    blocking(&state, move |_, s| s.overview(project_id.as_deref()).map_err(e)).await
}

#[tauri::command]
async fn data_version(state: State<'_, Shared>) -> CmdResult<i64> {
    blocking(&state, |_, s| s.data_version().map_err(e)).await
}

#[tauri::command]
async fn list_projects(state: State<'_, Shared>) -> CmdResult<Vec<ProjectSummary>> {
    blocking(&state, |_, s| s.list_projects().map_err(e)).await
}

#[tauri::command]
async fn add_project(state: State<'_, Shared>, path: String, name: Option<String>) -> CmdResult<Project> {
    blocking(&state, move |_, s| s.register_project(Path::new(&path), name.as_deref()).map(|(p, _)| p).map_err(e)).await
}

#[tauri::command]
async fn update_project(
    state: State<'_, Shared>,
    id: String,
    name: Option<String>,
    settings: Option<ProjectSettings>,
) -> CmdResult<Project> {
    blocking(&state, move |_, s| s.update_project(&id, name.as_deref(), settings.as_ref()).map_err(e)).await
}

#[tauri::command]
async fn remove_project(state: State<'_, Shared>, id: String) -> CmdResult<()> {
    blocking(&state, move |_, s| s.remove_project(&id).map_err(e)).await
}

#[tauri::command]
async fn check_freshness(state: State<'_, Shared>, project_id: String) -> CmdResult<FreshnessReport> {
    blocking(&state, move |_, s| s.check_freshness(&project_id, None, &user()).map_err(e)).await
}

#[tauri::command]
async fn preview_context(
    state: State<'_, Shared>,
    project_id: String,
    budget: Option<usize>,
    task: Option<String>,
    target: Option<AgentKind>,
) -> CmdResult<ContextBrief> {
    blocking(&state, move |_, s| {
        let b = budget.unwrap_or(s.settings().map_err(e)?.context.startup_budget_tokens);
        let mut opts = BriefOptions::new(b);
        opts.task = task;
        opts.target = target;
        s.build_brief(&project_id, &opts).map_err(e)
    })
    .await
}

// ---------------------------------------------------------------- memories

#[tauri::command]
async fn search_memories(state: State<'_, Shared>, query: SearchQuery) -> CmdResult<SearchResults> {
    blocking(&state, move |_, s| s.search_memories(&query).map_err(e)).await
}

#[tauri::command]
async fn get_memory(state: State<'_, Shared>, id: String) -> CmdResult<MemoryDetail> {
    blocking(&state, move |_, s| s.get_memory_detail(&id).map_err(e)).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryInput {
    project_id: String,
    kind: MemoryKind,
    title: String,
    #[serde(default)]
    content: String,
    status: Option<MemoryStatus>,
    certainty: Option<Certainty>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    pinned: bool,
    branch: Option<String>,
}

#[tauri::command]
async fn create_memory(state: State<'_, Shared>, input: MemoryInput) -> CmdResult<SaveOutcome> {
    blocking(&state, move |_, s| {
        let mut m = NewMemory::new(input.project_id, input.kind, input.title, input.content);
        m.status = input.status;
        m.certainty = input.certainty;
        m.tags = input.tags;
        m.files = input.files.into_iter().filter(|f| !f.trim().is_empty()).map(FileRefInput::path).collect();
        m.pinned = input.pinned;
        m.branch = input.branch;
        s.save_memory(m, &user()).map_err(e)
    })
    .await
}

#[tauri::command]
async fn update_memory(state: State<'_, Shared>, id: String, patch: MemoryPatch) -> CmdResult<SaveOutcome> {
    blocking(&state, move |_, s| s.update_memory(&id, patch, &user()).map_err(e)).await
}

#[tauri::command]
async fn set_pinned(state: State<'_, Shared>, id: String, pinned: bool) -> CmdResult<()> {
    blocking(&state, move |_, s| s.set_pinned(&id, pinned, &user()).map_err(e)).await
}

#[tauri::command]
async fn supersede_memory(state: State<'_, Shared>, id: String, title: String, content: String, reason: String) -> CmdResult<SaveOutcome> {
    blocking(&state, move |_, s| {
        let old = s.get_memory(&id).map_err(e)?;
        let new = NewMemory::new(old.project_id.clone(), old.kind, title, content);
        s.supersede_memory(&id, new, &reason, &user()).map_err(e)
    })
    .await
}

#[tauri::command]
async fn revalidate_memory(state: State<'_, Shared>, id: String, note: Option<String>) -> CmdResult<SaveOutcome> {
    blocking(&state, move |_, s| s.revalidate_memory(&id, note.as_deref(), Vec::<EvidenceInput>::new(), &user()).map_err(e)).await
}

#[tauri::command]
async fn delete_memory(state: State<'_, Shared>, id: String) -> CmdResult<()> {
    blocking(&state, move |_, s| s.delete_memory(&id, &user()).map_err(e)).await
}

// ---------------------------------------------------------------- sessions & handoffs

#[tauri::command]
async fn list_sessions(state: State<'_, Shared>, query: SessionQuery) -> CmdResult<Vec<SessionInfo>> {
    blocking(&state, move |_, s| s.list_sessions(&query).map_err(e)).await
}

#[tauri::command]
async fn session_detail(state: State<'_, Shared>, id: String) -> CmdResult<SessionDetail> {
    blocking(&state, move |_, s| s.session_detail(&id).map_err(e)).await
}

#[tauri::command]
async fn recent_events(state: State<'_, Shared>, project_id: Option<String>, limit: Option<usize>) -> CmdResult<Vec<EventRecord>> {
    blocking(&state, move |_, s| s.recent_events(project_id.as_deref(), limit.unwrap_or(100)).map_err(e)).await
}

#[tauri::command]
async fn generate_handoff(
    state: State<'_, Shared>,
    project_id: String,
    target: Option<AgentKind>,
    budget: Option<usize>,
) -> CmdResult<Handoff> {
    blocking(&state, move |_, s| {
        let b = budget.unwrap_or(s.settings().map_err(e)?.context.handoff_budget_tokens);
        s.generate_handoff(&project_id, target, b, &user(), chrono::Utc::now()).map_err(e)
    })
    .await
}

#[tauri::command]
async fn list_handoffs(state: State<'_, Shared>, project_id: Option<String>) -> CmdResult<Vec<HandoffSummary>> {
    blocking(&state, move |_, s| s.list_handoffs(project_id.as_deref(), 100).map_err(e)).await
}

#[tauri::command]
async fn get_handoff(state: State<'_, Shared>, id: String) -> CmdResult<Handoff> {
    blocking(&state, move |_, s| s.get_handoff(&id).map_err(e)).await
}

// ---------------------------------------------------------------- integrations

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationsView {
    cli_path: String,
    cli_exists: bool,
    statuses: Vec<ProviderStatus>,
    overall: Vec<(Provider, integ::ComponentState)>,
}

fn scope_for(project: Option<String>) -> Scope {
    match project {
        Some(p) if !p.trim().is_empty() => Scope::Project(PathBuf::from(p)),
        _ => Scope::User,
    }
}

#[tauri::command]
async fn integrations_status(state: State<'_, Shared>, project_path: Option<String>, refresh: Option<bool>) -> CmdResult<IntegrationsView> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let installs = {
            let mut cache = st.installs.lock().unwrap_or_else(|p| p.into_inner());
            if cache.is_none() || refresh.unwrap_or(false) {
                *cache = Some(integ::detect::detect_all(&st.env));
            }
            cache.clone().unwrap_or_default()
        };
        let exe = cli_path();
        let scope = scope_for(project_path);
        let statuses: Vec<ProviderStatus> = Provider::ALL.iter().map(|p| integ::status(*p, &scope, &exe, &st.env, &installs)).collect();
        let overall = statuses.iter().map(|s| (s.provider, s.overall())).collect();
        Ok(IntegrationsView { cli_exists: exe.is_file(), cli_path: exe.to_string_lossy().into_owned(), statuses, overall })
    })
    .await
    .map_err(e)?
}

#[derive(Serialize)]
struct PlanView {
    plan: InstallPlan,
    preview: String,
}

#[tauri::command]
async fn integration_plan(
    state: State<'_, Shared>,
    provider: Provider,
    action: Action,
    project_path: Option<String>,
) -> CmdResult<PlanView> {
    let env = state.env.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plan = integ::plan(provider, action, &scope_for(project_path), &cli_path(), &env).map_err(e)?;
        Ok(PlanView { preview: plan.preview(), plan })
    })
    .await
    .map_err(e)?
}

#[tauri::command]
async fn integration_apply(state: State<'_, Shared>, plan: InstallPlan) -> CmdResult<ApplyReport> {
    let env = state.env.clone();
    // `apply` re-verifies every file still matches the reviewed plan before writing anything.
    tauri::async_runtime::spawn_blocking(move || integ::plan::apply(&plan, &env).map_err(e)).await.map_err(e)?
}

#[tauri::command]
async fn integration_validate(state: State<'_, Shared>) -> CmdResult<Vec<CheckResult>> {
    let data = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let exe = cli_path();
        let mut out = vec![integ::doctor::check_mcp(&exe, Some(&data))];
        for p in Provider::ALL {
            out.push(integ::doctor::check_hook(&exe, p, Some(&data)));
        }
        Ok(out)
    })
    .await
    .map_err(e)?
}

#[tauri::command]
async fn config_backups(state: State<'_, Shared>) -> CmdResult<Vec<BackupRecord>> {
    Ok(integ::plan::list_backups(&state.env))
}

#[tauri::command]
async fn restore_config_backup(state: State<'_, Shared>, id: String, force: bool) -> CmdResult<RestoreReport> {
    let env = state.env.clone();
    tauri::async_runtime::spawn_blocking(move || integ::plan::restore_backup(&env, &id, force).map_err(e)).await.map_err(e)?
}

// ---------------------------------------------------------------- settings, data, diagnostics

#[tauri::command]
async fn get_settings(state: State<'_, Shared>) -> CmdResult<Settings> {
    blocking(&state, |_, s| s.settings().map_err(e)).await
}

#[tauri::command]
async fn save_settings(state: State<'_, Shared>, settings: Settings) -> CmdResult<()> {
    blocking(&state, move |_, s| s.save_settings(&settings).map_err(e)).await
}

#[tauri::command]
async fn export_data(state: State<'_, Shared>, path: String, project_id: Option<String>) -> CmdResult<usize> {
    blocking(&state, move |_, s| s.export_to_file(project_id.as_deref(), Path::new(&path)).map_err(e)).await
}

#[tauri::command]
async fn import_data(state: State<'_, Shared>, path: String) -> CmdResult<contextforge_core::backup::ImportReport> {
    blocking(&state, move |_, s| s.import_from_file(Path::new(&path)).map_err(e)).await
}

#[tauri::command]
async fn backup_database(state: State<'_, Shared>, path: Option<String>) -> CmdResult<String> {
    blocking(&state, move |_, s| match path {
        Some(p) => s.backup_to(Path::new(&p)).map(|_| p).map_err(e),
        None => s.backup_now().map(|p| p.to_string_lossy().into_owned()).map_err(e),
    })
    .await
}

#[tauri::command]
async fn list_database_backups(state: State<'_, Shared>) -> CmdResult<Vec<contextforge_core::backup::BackupFile>> {
    blocking(&state, |_, s| s.list_backups().map_err(e)).await
}

#[tauri::command]
async fn restore_database(state: State<'_, Shared>, path: String) -> CmdResult<String> {
    blocking(&state, move |_, s| s.restore_from(Path::new(&path)).map(|p| p.to_string_lossy().into_owned()).map_err(e)).await
}

#[tauri::command]
async fn prune_events(state: State<'_, Shared>) -> CmdResult<usize> {
    blocking(&state, |_, s| {
        let days = s.settings().map_err(e)?.retention.events_days;
        s.prune_events(days).map_err(e)
    })
    .await
}

/// Move the database to another folder: consistent copy, pointer update, reopen.
#[tauri::command]
async fn relocate_database(state: State<'_, Shared>, folder: String) -> CmdResult<String> {
    blocking(&state, move |st, s| {
        let dir = PathBuf::from(&folder);
        if !dir.is_dir() {
            return Err(format!("'{folder}' is not an existing folder"));
        }
        let target = dir.join("contextforge.db");
        if target.exists() {
            return Err(format!("{} already exists; choose an empty folder", target.display()));
        }
        s.backup_to(&target).map_err(e)?;
        contextforge_core::db::set_database_location(&st.data_dir, Some(&target)).map_err(e)?;
        *s = Store::open_at(&st.data_dir, &target, OpenOptions::interactive()).map_err(e)?;
        Ok(target.to_string_lossy().into_owned())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    version: String,
    home_dir: String,
    data_dir: String,
    database_path: String,
    logs_dir: String,
    backups_dir: String,
    config_backups_dir: String,
    cli_path: String,
    cli_exists: bool,
    git_available: bool,
    integrity: String,
    schema_version: i64,
}

#[tauri::command]
async fn diagnostics(state: State<'_, Shared>) -> CmdResult<Diagnostics> {
    blocking(&state, |st, s| {
        let integrity: String = s.conn().query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(e)?;
        let cli = cli_path();
        Ok(Diagnostics {
            version: contextforge_core::VERSION.into(),
            home_dir: st.env.home.to_string_lossy().into_owned(),
            data_dir: st.data_dir.to_string_lossy().into_owned(),
            database_path: s.db_path().to_string_lossy().into_owned(),
            logs_dir: st.data_dir.join("logs").to_string_lossy().into_owned(),
            backups_dir: s.backups_dir().to_string_lossy().into_owned(),
            config_backups_dir: st.env.backups_dir.to_string_lossy().into_owned(),
            cli_exists: cli.is_file(),
            cli_path: cli.to_string_lossy().into_owned(),
            git_available: contextforge_core::git::available(),
            integrity,
            schema_version: s.schema_version().map_err(e)?,
        })
    })
    .await
}

#[tauri::command]
async fn read_recent_log(state: State<'_, Shared>, lines: Option<usize>) -> CmdResult<String> {
    let dir = state.data_dir.join("logs");
    tauri::async_runtime::spawn_blocking(move || {
        let mut files: Vec<_> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        let Some(last) = files.last() else { return Ok(String::new()) };
        let text = std::fs::read_to_string(last).map_err(e)?;
        let n = lines.unwrap_or(200);
        let all: Vec<&str> = text.lines().collect();
        Ok(all[all.len().saturating_sub(n)..].join("\n"))
    })
    .await
    .map_err(e)?
}

pub fn run() {
    let data_dir = contextforge_core::data_dir();
    let store = match Store::open_default(OpenOptions::interactive()) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("ContextForge: cannot open database: {err}");
            std::process::exit(1);
        }
    };
    let env = Env::detect(&data_dir);
    let state: Shared = Arc::new(AppState { store: Mutex::new(store), data_dir, env, installs: Mutex::new(None) });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            overview,
            data_version,
            list_projects,
            add_project,
            update_project,
            remove_project,
            check_freshness,
            preview_context,
            search_memories,
            get_memory,
            create_memory,
            update_memory,
            set_pinned,
            supersede_memory,
            revalidate_memory,
            delete_memory,
            list_sessions,
            session_detail,
            recent_events,
            generate_handoff,
            list_handoffs,
            get_handoff,
            integrations_status,
            integration_plan,
            integration_apply,
            integration_validate,
            config_backups,
            restore_config_backup,
            get_settings,
            save_settings,
            export_data,
            import_data,
            backup_database,
            list_database_backups,
            restore_database,
            prune_events,
            relocate_database,
            diagnostics,
            read_recent_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ContextForge");
}
