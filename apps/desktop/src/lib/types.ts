// Mirrors the serde output of contextforge-core / contextforge-integrations (snake_case fields).

export type MemoryKind = "decision" | "convention" | "fact" | "task" | "bug" | "note" | "command" | "research";
export type MemoryStatus = "active" | "pending" | "in_progress" | "completed" | "cancelled" | "open" | "resolved" | "wont_fix";
export type Certainty = "verified" | "decided" | "unverified" | "hypothesis";
export type Freshness = "current" | "potentially_stale" | "needs_review" | "superseded";
export type AgentKind =
  | "claude_code" | "codex" | "antigravity_cli" | "antigravity_ide" | "antigravity_app" | "antigravity"
  | "mcp_client" | "user" | "cli" | "system";

export interface Memory {
  id: string;
  project_id: string;
  kind: MemoryKind;
  status: MemoryStatus;
  certainty: Certainty;
  freshness: Freshness;
  title: string;
  content: string;
  tags: string[];
  pinned: boolean;
  user_locked: boolean;
  branch: string | null;
  origin_branch: string | null;
  source_agent: AgentKind;
  source_session_id: string | null;
  source_origin: string;
  created_at: string;
  updated_at: string;
  last_validated_at: string | null;
  validated_commit: string | null;
  superseded_by: string | null;
  stale_reason: string | null;
  injection_flag: boolean;
  revision: number;
  reinforced_count: number;
  metadata: Record<string, unknown>;
}

export interface MemoryFile { path: string; content_hash: string | null; line_start: number | null; line_end: number | null; sensitive: boolean }
export interface Evidence { id: number; kind: string; reference: string; excerpt: string | null; agent: AgentKind; session_id: string | null; captured_at: string }
export interface MemoryLink { from_id: string; to_id: string; relation: string; created_at: string; other_title: string | null }
export interface Revision { revision: number; title: string; content: string; status: MemoryStatus; certainty: Certainty; changed_by: string; changed_at: string; reason: string | null }
export interface Finding { id: number; memory_id: string; detected_at: string; kind: string; path: string | null; detail: string; head_commit: string | null; branch: string | null; resolved_at: string | null; resolution: string | null }

export interface SessionInfo {
  id: string; project_id: string; agent: AgentKind; external_id: string | null; cwd: string | null; root_path: string | null;
  model: string | null; branch: string | null; start_commit: string | null; end_commit: string | null; started_at: string;
  last_activity_at: string; ended_at: string | null; end_reason: string | null; summary: string | null;
  files_touched: number; commands_run: number; turns: number;
}

export interface MemoryDetail {
  memory: Memory; files: MemoryFile[]; commits: string[]; evidence: Evidence[]; links: MemoryLink[];
  revisions: Revision[]; findings: Finding[]; source_session: SessionInfo | null;
}

export interface ProjectSettings { capture_enabled: boolean; excluded_globs: string[]; startup_budget_tokens: number | null }
export interface ProjectRoot { path: string; kind: string; branch: string | null; added_at: string }
export interface Project {
  id: string; name: string; root_path: string; is_git: boolean; repo_fingerprint: string | null; remote_url: string | null;
  default_branch: string | null; settings: ProjectSettings; created_at: string; updated_at: string; last_activity_at: string | null; roots: ProjectRoot[];
}
export interface ProjectSummary { project: Project; memory_count: number; open_tasks: number; open_bugs: number; needs_review: number; session_count: number; agents: AgentKind[] }

export interface EventRecord { id: number; project_id: string; session_id: string | null; agent: AgentKind; kind: string; occurred_at: string; summary: string; detail: Record<string, unknown> }
export interface SessionFile { path: string; action: string; sensitive: boolean; touches: number; last_at: string }
export interface SessionDetail { session: SessionInfo; events: EventRecord[]; files: SessionFile[]; memories: [string, string, string][] }
export interface AgentActivity { agent: AgentKind; last_event_at: string; last_event_kind: string; event_count: number }
export interface Count<T> { key: T; count: number }

export interface Overview {
  projects: number; memories: number; by_kind: Count<MemoryKind>[]; by_freshness: Count<Freshness>[]; open_tasks: number; open_bugs: number;
  needs_review: number; sessions_total: number; sessions_last_7d: Count<AgentKind>[]; handoffs: number; agent_activity: AgentActivity[];
  recent_sessions: SessionInfo[]; recent_events: EventRecord[]; database_path: string; database_bytes: number; schema_version: number;
}

export interface SearchQuery {
  project_id?: string | null; text?: string | null; mode?: "all" | "any"; kinds?: MemoryKind[]; statuses?: MemoryStatus[];
  freshness?: Freshness[]; certainty?: Certainty[]; agents?: AgentKind[]; pinned_only?: boolean; include_superseded?: boolean;
  file?: string | null; limit?: number; offset?: number; sort?: "relevance" | "updated" | "created";
}
export interface SearchHit { memory: Memory; snippet: string | null; files: string[] }
export interface SearchResults { hits: SearchHit[]; total: number }

export interface SaveOutcome { memory_id: string; action: "created" | "reinforced"; certainty: Certainty; status: MemoryStatus; notes: string[]; similar: { id: string; title: string; similarity: number }[] }
export interface MemoryPatch {
  title?: string; content?: string; status?: MemoryStatus; certainty?: Certainty; kind?: MemoryKind; tags?: string[];
  pinned?: boolean; metadata?: Record<string, unknown>; reason?: string;
}

export interface FreshnessChange { memory_id: string; title: string; from: Freshness; to: Freshness; reasons: string[] }
export interface FreshnessReport { project_id: string; root: string; head: string | null; branch: string | null; checked: number; changes: FreshnessChange[]; renames_followed: number; flagged_total: number; duration_ms: number }
export interface ContextBrief { text: string; token_estimate: number; memory_ids: string[]; omitted: number }

export interface HandoffSummary { id: string; project_id: string; target_agent: AgentKind | null; requested_by: AgentKind; created_at: string; token_estimate: number; branch: string | null; head_commit: string | null }
export interface Handoff { id: string; project_id: string; target_agent: AgentKind | null; requested_by: AgentKind; created_at: string; token_estimate: number; markdown: string; doc: Record<string, unknown> }

export type Provider = "claude" | "codex" | "antigravity";
export type Action = "install" | "uninstall" | "repair";
export type ComponentState = "not_installed" | "installed" | "partial" | "outdated" | "pending_user_action" | "error";
export interface ComponentStatus { state: ComponentState; detail: string; path: string | null }
export interface AgentInstall { id: string; label: string; provider: Provider; installed: boolean; executable: string | null; version: string | null; detail: string | null }
export interface ProviderStatus { provider: Provider; label: string; installations: AgentInstall[]; mcp: ComponentStatus; hooks: ComponentStatus; trust: ComponentStatus | null; notes: string[] }
export interface IntegrationsView { cliPath: string; cliExists: boolean; statuses: ProviderStatus[]; overall: [Provider, ComponentState][] }
export interface FileChange { path: string; before: string | null; after: string | null; summary: string }
export interface ExternalCommand { program: string; args: string[]; cwd: string | null; summary: string; undo_args: string[] | null }
export interface InstallPlan { provider: Provider; action: Action; scope: unknown; changes: FileChange[]; commands: ExternalCommand[]; notes: string[]; warnings: string[] }
export interface PlanView { plan: InstallPlan; preview: string }
export interface ApplyReport { backup_id: string | null; files_written: string[]; commands_run: string[]; notes: string[] }
export interface CheckResult { name: string; ok: boolean; detail: string; duration_ms: number }
export interface BackupRecord { id: string; provider: Provider; action: Action; scope: unknown; created_at: string; files: { original: string; stored: string | null }[]; commands: string[] }

export interface Settings {
  context: { startup_budget_tokens: number; prompt_budget_tokens: number; prompt_injection: boolean; handoff_budget_tokens: number };
  capture: { commands: boolean; prompts: boolean; assistant_summaries: boolean; auto_command_memories: boolean; auto_register_projects: boolean };
  retention: { events_days: number; handoffs_per_project: number };
  privacy: { extra_sensitive_globs: string[] };
  freshness: { check_on_session_start: boolean };
  logging: { level: string };
  ui: { theme: "system" | "dark" | "light" };
}

export interface Diagnostics {
  version: string; homeDir: string; dataDir: string; databasePath: string; logsDir: string; backupsDir: string; configBackupsDir: string;
  cliPath: string; cliExists: boolean; gitAvailable: boolean; integrity: string; schemaVersion: number;
}
export interface BackupFile { path: string; size_bytes: number; modified: string }
export interface ImportReport { projects_created: number; projects_matched: number; memories_imported: number; memories_skipped: number; links_imported: number }
