//! Tool input schemas. Doc comments become JSON-schema descriptions shown to the model.

use contextforge_core::model::{AgentKind, Certainty, EvidenceKind, Freshness, MemoryKind, MemoryStatus};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ProjectParam {
    /// Project id (prj_…) or repository path. Optional when the client's workspace is a registered project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IdParam {
    /// Memory id (mem_…).
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FileRef {
    /// File path, relative to the repository root (absolute paths inside the project are accepted).
    pub path: String,
    /// Optional first line of the relevant range (1-based).
    #[serde(default)]
    pub line_start: Option<i64>,
    /// Optional last line of the relevant range.
    #[serde(default)]
    pub line_end: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EvidenceParam {
    /// Evidence type: command (that ran), commit, file, url, session_event, agent_check, user_note.
    pub kind: EvidenceKind,
    /// The command line, commit hash, file path, URL, or a short label.
    pub reference: String,
    /// Short supporting excerpt, e.g. "exit 0, 214 tests passed" or how the check was done.
    #[serde(default)]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct SearchParams {
    /// Free-text query. Leave empty to list by filters only.
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    /// Require every query term (default false: any term, ranked by relevance).
    #[serde(default)]
    pub match_all: Option<bool>,
    #[serde(default)]
    pub kinds: Vec<MemoryKind>,
    #[serde(default)]
    pub statuses: Vec<MemoryStatus>,
    #[serde(default)]
    pub freshness: Vec<Freshness>,
    /// Only memories recorded by these agents.
    #[serde(default)]
    pub agents: Vec<AgentKind>,
    /// Only memories referencing this repository-relative file path.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub include_superseded: Option<bool>,
    /// Maximum results (1–50, default 10).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ContextParams {
    #[serde(default)]
    pub project: Option<String>,
    /// What you are about to work on; memories relevant to it are listed first.
    #[serde(default)]
    pub task: Option<String>,
    /// Token budget for the brief (200–8000, default from settings).
    #[serde(default)]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SaveParams {
    /// decision, convention, fact, task, bug, note, command or research.
    pub kind: MemoryKind,
    /// Short, specific title (max 200 characters).
    pub title: String,
    /// The memory itself: concise, self-contained, no secrets.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub project: Option<String>,
    /// verified (needs evidence), decided, unverified (default for observations) or hypothesis.
    #[serde(default)]
    pub certainty: Option<Certainty>,
    /// Lifecycle status valid for the kind (tasks: pending/in_progress/completed/cancelled; bugs: open/resolved/wont_fix; research: open/resolved).
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Files this memory is about; ContextForge watches them to detect when the memory may become stale.
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    /// Related commit hashes.
    #[serde(default)]
    pub commits: Option<Vec<String>>,
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceParam>>,
    /// Restrict the memory to one branch (optional).
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateParams {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub certainty: Option<Certainty>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub add_files: Option<Vec<FileRef>>,
    #[serde(default)]
    pub add_commits: Option<Vec<String>>,
    #[serde(default)]
    pub add_evidence: Option<Vec<EvidenceParam>>,
    /// Why the change was made (kept in the revision history).
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SupersedeParams {
    /// Id of the outdated memory.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
    /// Why the old memory is outdated.
    pub reason: String,
    /// Kind of the replacement (defaults to the old memory's kind).
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    #[serde(default)]
    pub certainty: Option<Certainty>,
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceParam>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DecisionParams {
    /// The decision in a few words, e.g. "Use SQLite WAL mode for the memory store".
    pub title: String,
    /// What was decided.
    pub decision: String,
    #[serde(default)]
    pub rationale: Option<String>,
    /// Alternatives that were considered and rejected.
    #[serde(default)]
    pub alternatives: Option<Vec<String>>,
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Id of an earlier decision this one replaces.
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct TaskParams {
    /// Existing task id to update; omit to create a new task.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// pending, in_progress, completed or cancelled.
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    /// Description or progress notes (appended when updating).
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    /// Evidence of completion, e.g. the test command that passed or the commit.
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceParam>>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BugParams {
    /// Existing bug id to update; omit to record a new bug.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// open, resolved or wont_fix.
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    /// Symptoms, root cause, or fix description.
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    /// Commit that fixed the bug (recorded as evidence).
    #[serde(default)]
    pub fix_commit: Option<String>,
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceParam>>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct HandoffParams {
    /// Agent that will receive the handoff: claude_code, codex, antigravity_cli, antigravity_ide, antigravity_app.
    #[serde(default)]
    pub target_agent: Option<AgentKind>,
    #[serde(default)]
    pub project: Option<String>,
    /// Token budget (300–12000, default from settings).
    #[serde(default)]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct HealthParams {
    #[serde(default)]
    pub project: Option<String>,
    /// Re-evaluate freshness against the current working tree before reporting.
    #[serde(default)]
    pub recheck: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RevalidateParams {
    pub id: String,
    /// How you confirmed the memory is still accurate (at least 10 characters).
    pub note: String,
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceParam>>,
}
