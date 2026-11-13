import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

/** Typed wrappers around the Tauri commands in src-tauri/src/lib.rs. */
export const api = {
  overview: (projectId?: string | null) => invoke<T.Overview>("overview", { projectId: projectId ?? null }),
  dataVersion: () => invoke<number>("data_version"),

  listProjects: () => invoke<T.ProjectSummary[]>("list_projects"),
  addProject: (path: string, name?: string) => invoke<T.Project>("add_project", { path, name: name || null }),
  updateProject: (id: string, name: string | null, settings: T.ProjectSettings | null) => invoke<T.Project>("update_project", { id, name, settings }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  checkFreshness: (projectId: string) => invoke<T.FreshnessReport>("check_freshness", { projectId }),
  previewContext: (projectId: string, budget?: number, task?: string, target?: T.AgentKind | null) =>
    invoke<T.ContextBrief>("preview_context", { projectId, budget: budget ?? null, task: task || null, target: target ?? null }),

  searchMemories: (query: T.SearchQuery) => invoke<T.SearchResults>("search_memories", { query }),
  getMemory: (id: string) => invoke<T.MemoryDetail>("get_memory", { id }),
  createMemory: (input: {
    projectId: string; kind: T.MemoryKind; title: string; content: string; status?: T.MemoryStatus | null; certainty?: T.Certainty | null;
    tags?: string[]; files?: string[]; pinned?: boolean;
  }) => invoke<T.SaveOutcome>("create_memory", { input }),
  updateMemory: (id: string, patch: T.MemoryPatch) => invoke<T.SaveOutcome>("update_memory", { id, patch }),
  setPinned: (id: string, pinned: boolean) => invoke<void>("set_pinned", { id, pinned }),
  supersedeMemory: (id: string, title: string, content: string, reason: string) => invoke<T.SaveOutcome>("supersede_memory", { id, title, content, reason }),
  revalidateMemory: (id: string, note?: string) => invoke<T.SaveOutcome>("revalidate_memory", { id, note: note || null }),
  deleteMemory: (id: string) => invoke<void>("delete_memory", { id }),

  listSessions: (query: { project_id?: string | null; agent?: T.AgentKind | null; limit?: number; offset?: number }) =>
    invoke<T.SessionInfo[]>("list_sessions", { query }),
  sessionDetail: (id: string) => invoke<T.SessionDetail>("session_detail", { id }),
  recentEvents: (projectId?: string | null, limit?: number) => invoke<T.EventRecord[]>("recent_events", { projectId: projectId ?? null, limit: limit ?? null }),

  generateHandoff: (projectId: string, target: T.AgentKind | null, budget?: number) =>
    invoke<T.Handoff>("generate_handoff", { projectId, target, budget: budget ?? null }),
  listHandoffs: (projectId?: string | null) => invoke<T.HandoffSummary[]>("list_handoffs", { projectId: projectId ?? null }),
  getHandoff: (id: string) => invoke<T.Handoff>("get_handoff", { id }),

  integrationsStatus: (projectPath?: string | null, refresh?: boolean) =>
    invoke<T.IntegrationsView>("integrations_status", { projectPath: projectPath ?? null, refresh: refresh ?? false }),
  integrationPlan: (provider: T.Provider, action: T.Action, projectPath?: string | null) =>
    invoke<T.PlanView>("integration_plan", { provider, action, projectPath: projectPath ?? null }),
  integrationApply: (plan: T.InstallPlan) => invoke<T.ApplyReport>("integration_apply", { plan }),
  integrationValidate: () => invoke<T.CheckResult[]>("integration_validate"),
  configBackups: () => invoke<T.BackupRecord[]>("config_backups"),
  restoreConfigBackup: (id: string, force: boolean) => invoke<{ restored: string[]; notes: string[] }>("restore_config_backup", { id, force }),

  getSettings: () => invoke<T.Settings>("get_settings"),
  saveSettings: (settings: T.Settings) => invoke<void>("save_settings", { settings }),
  exportData: (path: string, projectId?: string | null) => invoke<number>("export_data", { path, projectId: projectId ?? null }),
  importData: (path: string) => invoke<T.ImportReport>("import_data", { path }),
  backupDatabase: (path?: string | null) => invoke<string>("backup_database", { path: path ?? null }),
  listDatabaseBackups: () => invoke<T.BackupFile[]>("list_database_backups"),
  restoreDatabase: (path: string) => invoke<string>("restore_database", { path }),
  pruneEvents: () => invoke<number>("prune_events"),
  relocateDatabase: (folder: string) => invoke<string>("relocate_database", { folder }),
  diagnostics: () => invoke<T.Diagnostics>("diagnostics"),
  readRecentLog: (lines?: number) => invoke<string>("read_recent_log", { lines: lines ?? null }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
