//! `contextforge` — command-line interface, lifecycle hook runner and MCP server entry point.

mod commands;
mod hook;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "contextforge",
    version,
    about = "One memory. Every coding agent.\nLocal-first shared project memory for Claude Code, Codex and Antigravity."
)]
pub struct Cli {
    /// Data directory (overrides CONTEXTFORGE_HOME and the default per-user location).
    #[arg(long, global = true, env = "CONTEXTFORGE_HOME", value_name = "DIR", hide_env_values = true)]
    pub data_dir: Option<PathBuf>,
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show where data lives and what it contains.
    Status,
    /// Register and manage projects.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Create, search, inspect and curate memories.
    #[command(subcommand)]
    Memory(MemoryCmd),
    /// Print the budgeted context brief an agent would receive.
    Context(ContextArgs),
    /// Generate a handoff brief for another agent.
    Handoff(HandoffArgs),
    /// Re-check memory freshness against the working tree.
    Freshness {
        #[arg(long)]
        project: Option<String>,
    },
    /// List recent agent sessions.
    Sessions {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Install, inspect, repair or remove agent integrations.
    #[command(subcommand)]
    Integrations(IntegrationsCmd),
    /// Validate the MCP server, hook commands and integration configuration.
    Doctor,
    /// Export memories (with provenance) to a JSON file.
    Export {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, short)]
        out: PathBuf,
    },
    /// Import memories from a ContextForge JSON export.
    Import { file: PathBuf },
    /// Write a consistent snapshot of the whole database.
    Backup {
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Replace the database with a backup (the current state is backed up first).
    Restore {
        file: PathBuf,
        #[arg(long)]
        yes: bool,
    },
    /// Show or change settings.
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Lifecycle hook entry point used by agents (reads the event JSON on stdin).
    Hook {
        /// claude, codex or antigravity
        provider: String,
        /// Provider event, e.g. session-start, user-prompt-submit, post-tool-use, pre-invocation, stop, session-end
        event: String,
    },
    /// Run the MCP server over stdio (used by agents).
    Mcp {
        /// Fallback project path when the client does not report its workspace.
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Register a local folder (Git repositories are detected automatically).
    Add {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    List,
    Show {
        project: String,
    },
    /// Remove a project from ContextForge. Project files are never touched.
    Remove {
        project: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KindArg {
    Decision,
    Convention,
    Fact,
    Task,
    Bug,
    Note,
    Command,
    Research,
}

#[derive(Subcommand)]
pub enum MemoryCmd {
    Add(MemoryAddArgs),
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, value_enum)]
        kind: Vec<KindArg>,
        #[arg(long)]
        status: Vec<String>,
        #[arg(long)]
        freshness: Vec<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    Search {
        query: String,
        #[arg(long)]
        project: Option<String>,
        /// Match any term (ranked) instead of all terms.
        #[arg(long)]
        any: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        id: String,
    },
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        certainty: Option<String>,
    },
    Pin {
        id: String,
    },
    Unpin {
        id: String,
    },
    Supersede {
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        content: String,
        #[arg(long, default_value = "superseded by the user")]
        reason: String,
    },
    Revalidate {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
    Delete {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args)]
pub struct MemoryAddArgs {
    #[arg(long, value_enum)]
    pub kind: KindArg,
    #[arg(long)]
    pub title: String,
    #[arg(long, default_value = "")]
    pub content: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub file: Vec<String>,
    #[arg(long)]
    pub commit: Vec<String>,
    #[arg(long)]
    pub certainty: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub tag: Vec<String>,
    #[arg(long)]
    pub pin: bool,
}

#[derive(Args)]
pub struct ContextArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub budget: Option<usize>,
    #[arg(long)]
    pub task: Option<String>,
}

#[derive(Args)]
pub struct HandoffArgs {
    #[arg(long)]
    pub project: Option<String>,
    /// Target agent: claude_code, codex, antigravity_cli, antigravity_ide, antigravity_app
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long)]
    pub budget: Option<usize>,
}

#[derive(Subcommand)]
pub enum IntegrationsCmd {
    /// Show detected agents and integration state.
    Status {
        #[arg(long)]
        project: Option<PathBuf>,
    },
    Install(IntegrationArgs),
    Uninstall(IntegrationArgs),
    Repair(IntegrationArgs),
    /// List configuration backups made by ContextForge.
    Backups,
    /// Restore configuration files from a backup.
    Restore {
        backup_id: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Args)]
pub struct IntegrationArgs {
    /// claude, codex, antigravity or all
    pub provider: String,
    /// Install into this project's configuration instead of the user-level configuration.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Show the exact changes without applying them.
    #[arg(long)]
    pub dry_run: bool,
    /// Apply without asking for confirmation.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Subcommand)]
pub enum SettingsCmd {
    Show,
    /// Set a value by dotted key, e.g. `context.startup_budget_tokens 1500`.
    Set {
        key: String,
        value: String,
    },
}

fn init_logging(cli: &Cli) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    let data_dir = cli.data_dir.clone().unwrap_or_else(contextforge_core::data_dir);
    let level = std::env::var("CONTEXTFORGE_LOG").ok().unwrap_or_else(|| {
        contextforge_core::Store::open_in(&data_dir, contextforge_core::OpenOptions::hook())
            .ok()
            .and_then(|s| s.settings().ok())
            .map(|s| s.logging.level)
            .unwrap_or_else(|| "info".into())
    });
    let filter = EnvFilter::try_new(format!("{level},rmcp=warn")).unwrap_or_else(|_| EnvFilter::new("info"));
    let logs = data_dir.join("logs");
    std::fs::create_dir_all(&logs).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("contextforge")
        .filename_suffix("log")
        .max_log_files(14)
        .build(&logs)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    // Never log to stdout: it carries the MCP protocol and hook output.
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(writer).with_ansi(false).with_target(false).try_init().ok()?;
    Some(guard)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(dir) = &cli.data_dir {
        // Make the location visible to every component (core, MCP server) of this process.
        // SAFETY: set once at startup before any other thread is spawned.
        unsafe { std::env::set_var(contextforge_core::db::HOME_ENV, dir) };
    }
    let _guard = init_logging(&cli);
    match &cli.command {
        Command::Hook { provider, event } => match provider.parse() {
            Ok(p) => hook::run(p, event),
            Err(e) => {
                tracing::warn!(error = %e, "unknown hook provider");
                ExitCode::SUCCESS
            }
        },
        Command::Mcp { project } => {
            let opts = contextforge_mcp::ServeOptions { data_dir: cli.data_dir.clone(), project_hint: project.clone() };
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("contextforge: cannot start runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match rt.block_on(contextforge_mcp::serve_stdio(opts)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = %e, "MCP server stopped with an error");
                    eprintln!("contextforge mcp: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => commands::run(&cli),
    }
}
