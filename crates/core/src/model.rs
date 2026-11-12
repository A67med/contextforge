//! Shared data model. All enums serialise as snake_case strings, matching the database.

use serde::{Deserialize, Serialize};

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $s)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(&self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::Error;
            fn from_str(s: &str) -> crate::Result<Self> {
                let norm = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
                match norm.as_str() {
                    $($s => Ok($name::$variant),)+
                    _ => Err(crate::Error::Invalid(format!(
                        "invalid {} '{}'; expected one of: {}",
                        stringify!($name), s, [$($s),+].join(", ")
                    ))),
                }
            }
        }

        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::Borrowed(rusqlite::types::ValueRef::Text(self.as_str().as_bytes())))
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let s = value.as_str()?;
                s.parse().map_err(|e: crate::Error| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}

string_enum! {
    /// What a memory describes.
    MemoryKind {
        /// An explicit architectural or design decision.
        Decision => "decision",
        /// A coding/technical convention the project follows.
        Convention => "convention",
        /// A project fact (verified or observed).
        Fact => "fact",
        /// A unit of work (pending, in progress, completed, cancelled).
        Task => "task",
        /// A bug (open, resolved, won't fix).
        Bug => "bug",
        /// An implementation note.
        Note => "note",
        /// An important command or environment detail.
        Command => "command",
        /// A research finding or unresolved hypothesis.
        Research => "research",
    }
}

string_enum! {
    /// Lifecycle status. Which statuses are valid depends on the kind.
    MemoryStatus {
        Active => "active",
        Pending => "pending",
        InProgress => "in_progress",
        Completed => "completed",
        Cancelled => "cancelled",
        Open => "open",
        Resolved => "resolved",
        WontFix => "wont_fix",
    }
}

string_enum! {
    /// How much a memory has been established. Never a numeric score.
    Certainty {
        /// Backed by recorded evidence (command run, commit, file, user confirmation).
        Verified => "verified",
        /// An explicit decision made by the user or an agent.
        Decided => "decided",
        /// An observation that has not been verified.
        Unverified => "unverified",
        /// A hypothesis or open question.
        Hypothesis => "hypothesis",
    }
}

string_enum! {
    /// Whether the memory still matches the code it references.
    Freshness {
        Current => "current",
        PotentiallyStale => "potentially_stale",
        NeedsReview => "needs_review",
        Superseded => "superseded",
    }
}

string_enum! {
    /// The agent (or human surface) responsible for an action.
    AgentKind {
        ClaudeCode => "claude_code",
        Codex => "codex",
        AntigravityCli => "antigravity_cli",
        AntigravityIde => "antigravity_ide",
        AntigravityApp => "antigravity_app",
        Antigravity => "antigravity",
        McpClient => "mcp_client",
        User => "user",
        Cli => "cli",
        System => "system",
    }
}

string_enum! {
    /// Through which channel a record was created.
    Origin {
        Mcp => "mcp",
        Hook => "hook",
        Cli => "cli",
        Ui => "ui",
        Import => "import",
        Auto => "auto",
    }
}

string_enum! {
    EvidenceKind {
        File => "file",
        Commit => "commit",
        Command => "command",
        Url => "url",
        SessionEvent => "session_event",
        AgentCheck => "agent_check",
        UserNote => "user_note",
    }
}

string_enum! {
    LinkRelation {
        Supersedes => "supersedes",
        RelatesTo => "relates_to",
        Resolves => "resolves",
        DependsOn => "depends_on",
        DuplicateOf => "duplicate_of",
    }
}

string_enum! {
    EventKind {
        SessionStart => "session_start",
        Prompt => "prompt",
        FileEdit => "file_edit",
        Command => "command",
        Tool => "tool",
        TurnEnd => "turn_end",
        SessionEnd => "session_end",
        MemorySaved => "memory_saved",
        MemoryUpdated => "memory_updated",
        Handoff => "handoff",
        ContextInjected => "context_injected",
        FreshnessCheck => "freshness_check",
        Commits => "commits",
    }
}

string_enum! {
    FindingKind {
        FileModified => "file_modified",
        FileDeleted => "file_deleted",
        FileRenamed => "file_renamed",
        CommitMissing => "commit_missing",
        /// An agent proposed replacing a user-owned memory; only the user can resolve this.
        ReplacementProposed => "replacement_proposed",
    }
}

impl FindingKind {
    /// Freshness implied by an unresolved finding of this kind.
    pub fn implied_freshness(self) -> Freshness {
        match self {
            FindingKind::FileModified => Freshness::PotentiallyStale,
            FindingKind::FileDeleted | FindingKind::CommitMissing | FindingKind::ReplacementProposed => Freshness::NeedsReview,
            FindingKind::FileRenamed => Freshness::Current,
        }
    }

    /// Findings produced (and cleared) automatically by freshness checks.
    pub fn is_automatic(self) -> bool {
        !matches!(self, FindingKind::ReplacementProposed)
    }
}

impl MemoryKind {
    pub fn default_status(self) -> MemoryStatus {
        match self {
            MemoryKind::Task => MemoryStatus::Pending,
            MemoryKind::Bug | MemoryKind::Research => MemoryStatus::Open,
            _ => MemoryStatus::Active,
        }
    }

    pub fn allowed_statuses(self) -> &'static [MemoryStatus] {
        use MemoryStatus::*;
        match self {
            MemoryKind::Task => &[Pending, InProgress, Completed, Cancelled],
            MemoryKind::Bug => &[Open, Resolved, WontFix],
            MemoryKind::Research => &[Open, Resolved],
            _ => &[Active],
        }
    }

    pub fn default_certainty(self) -> Certainty {
        match self {
            MemoryKind::Decision | MemoryKind::Convention => Certainty::Decided,
            MemoryKind::Research => Certainty::Hypothesis,
            _ => Certainty::Unverified,
        }
    }

    /// Decisions and conventions describe intent, so small code edits do not make them stale.
    pub fn tolerant_to_edits(self) -> bool {
        matches!(self, MemoryKind::Decision | MemoryKind::Convention)
    }

    pub fn label(self) -> &'static str {
        match self {
            MemoryKind::Decision => "Decision",
            MemoryKind::Convention => "Convention",
            MemoryKind::Fact => "Fact",
            MemoryKind::Task => "Task",
            MemoryKind::Bug => "Bug",
            MemoryKind::Note => "Note",
            MemoryKind::Command => "Command",
            MemoryKind::Research => "Research",
        }
    }
}

impl MemoryStatus {
    /// Is this status "still open" (work or problem outstanding)?
    pub fn is_open(self) -> bool {
        matches!(self, MemoryStatus::Pending | MemoryStatus::InProgress | MemoryStatus::Open)
    }
}

impl Freshness {
    pub fn severity(self) -> u8 {
        match self {
            Freshness::Current => 0,
            Freshness::PotentiallyStale => 1,
            Freshness::NeedsReview => 2,
            Freshness::Superseded => 3,
        }
    }
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::AntigravityCli => "Antigravity CLI",
            AgentKind::AntigravityIde => "Antigravity IDE",
            AgentKind::AntigravityApp => "Antigravity App",
            AgentKind::Antigravity => "Antigravity",
            AgentKind::McpClient => "MCP client",
            AgentKind::User => "User",
            AgentKind::Cli => "CLI",
            AgentKind::System => "ContextForge",
        }
    }

    pub fn is_agent(self) -> bool {
        !matches!(self, AgentKind::User | AgentKind::Cli | AgentKind::System)
    }

    pub fn is_antigravity(self) -> bool {
        matches!(self, AgentKind::AntigravityCli | AgentKind::AntigravityIde | AgentKind::AntigravityApp | AgentKind::Antigravity)
    }
}

/// Who performs an action — used for provenance and permission checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Actor {
    pub agent: AgentKind,
    pub origin: Origin,
    pub session_id: Option<String>,
}

impl Actor {
    pub fn user_ui() -> Self {
        Actor { agent: AgentKind::User, origin: Origin::Ui, session_id: None }
    }
    pub fn user_cli() -> Self {
        Actor { agent: AgentKind::User, origin: Origin::Cli, session_id: None }
    }
    pub fn system() -> Self {
        Actor { agent: AgentKind::System, origin: Origin::Auto, session_id: None }
    }
    pub fn agent(agent: AgentKind, origin: Origin, session_id: Option<String>) -> Self {
        Actor { agent, origin, session_id }
    }
    /// True for the human user acting through the desktop app or the CLI.
    pub fn is_user(&self) -> bool {
        self.agent == AgentKind::User
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub id: String,
    pub project_id: String,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub certainty: Certainty,
    pub freshness: Freshness,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub user_locked: bool,
    pub branch: Option<String>,
    pub origin_branch: Option<String>,
    pub source_agent: AgentKind,
    pub source_session_id: Option<String>,
    pub source_origin: Origin,
    pub created_at: String,
    pub updated_at: String,
    pub last_validated_at: Option<String>,
    pub validated_commit: Option<String>,
    pub superseded_by: Option<String>,
    pub stale_reason: Option<String>,
    pub injection_flag: bool,
    pub revision: i64,
    pub reinforced_count: i64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryFile {
    pub path: String,
    pub content_hash: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    pub id: i64,
    pub kind: EvidenceKind,
    pub reference: String,
    pub excerpt: Option<String>,
    pub agent: AgentKind,
    pub session_id: Option<String>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryLink {
    pub from_id: String,
    pub to_id: String,
    pub relation: LinkRelation,
    pub created_at: String,
    /// Title of the memory on the other end of the link, for display.
    pub other_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Revision {
    pub revision: i64,
    pub title: String,
    pub content: String,
    pub status: MemoryStatus,
    pub certainty: Certainty,
    pub changed_by: String,
    pub changed_at: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    pub id: i64,
    pub memory_id: String,
    pub detected_at: String,
    pub kind: FindingKind,
    pub path: Option<String>,
    pub detail: String,
    pub head_commit: Option<String>,
    pub branch: Option<String>,
    pub resolved_at: Option<String>,
    pub resolution: Option<String>,
}

/// A memory plus everything needed to inspect its provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryDetail {
    pub memory: Memory,
    pub files: Vec<MemoryFile>,
    pub commits: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub links: Vec<MemoryLink>,
    pub revisions: Vec<Revision>,
    pub findings: Vec<Finding>,
    pub source_session: Option<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    pub project_id: String,
    pub agent: AgentKind,
    pub external_id: Option<String>,
    pub cwd: Option<String>,
    pub root_path: Option<String>,
    pub model: Option<String>,
    pub branch: Option<String>,
    pub start_commit: Option<String>,
    pub end_commit: Option<String>,
    pub started_at: String,
    pub last_activity_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    pub summary: Option<String>,
    pub files_touched: i64,
    pub commands_run: i64,
    pub turns: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventRecord {
    pub id: i64,
    pub project_id: String,
    pub session_id: Option<String>,
    pub agent: AgentKind,
    pub kind: EventKind,
    pub occurred_at: String,
    pub summary: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionFile {
    pub path: String,
    pub action: String,
    pub sensitive: bool,
    pub touches: i64,
    pub last_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn enums_round_trip_and_reject_unknown() {
        for k in MemoryKind::ALL {
            assert_eq!(MemoryKind::from_str(k.as_str()).unwrap(), *k);
        }
        assert_eq!(MemoryStatus::from_str("in-progress").unwrap(), MemoryStatus::InProgress);
        assert!(MemoryKind::from_str("poem").is_err());
        assert_eq!(serde_json::to_string(&Freshness::PotentiallyStale).unwrap(), "\"potentially_stale\"");
    }

    #[test]
    fn statuses_are_kind_specific() {
        assert!(MemoryKind::Task.allowed_statuses().contains(&MemoryStatus::Completed));
        assert!(!MemoryKind::Decision.allowed_statuses().contains(&MemoryStatus::Completed));
        assert_eq!(MemoryKind::Bug.default_status(), MemoryStatus::Open);
    }
}
