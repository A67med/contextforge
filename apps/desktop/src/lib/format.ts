import type { AgentKind, Certainty, Freshness, MemoryKind, MemoryStatus } from "./types";

export const KIND_LABEL: Record<MemoryKind, string> = {
  decision: "Decision", convention: "Convention", fact: "Fact", task: "Task", bug: "Bug", note: "Note", command: "Command", research: "Research",
};
export const KINDS = Object.keys(KIND_LABEL) as MemoryKind[];

export const STATUS_LABEL: Record<MemoryStatus, string> = {
  active: "Active", pending: "Pending", in_progress: "In progress", completed: "Completed", cancelled: "Cancelled",
  open: "Open", resolved: "Resolved", wont_fix: "Won't fix",
};
export const STATUSES_FOR: Record<MemoryKind, MemoryStatus[]> = {
  task: ["pending", "in_progress", "completed", "cancelled"],
  bug: ["open", "resolved", "wont_fix"],
  research: ["open", "resolved"],
  decision: ["active"], convention: ["active"], fact: ["active"], note: ["active"], command: ["active"],
};

export const CERTAINTY_LABEL: Record<Certainty, string> = { verified: "Verified", decided: "Decided", unverified: "Unverified", hypothesis: "Hypothesis" };
export const FRESHNESS_LABEL: Record<Freshness, string> = {
  current: "Current", potentially_stale: "Possibly stale", needs_review: "Needs review", superseded: "Superseded",
};

export const AGENT_LABEL: Record<AgentKind, string> = {
  claude_code: "Claude Code", codex: "Codex", antigravity_cli: "Antigravity CLI", antigravity_ide: "Antigravity IDE",
  antigravity_app: "Antigravity App", antigravity: "Antigravity", mcp_client: "MCP client", user: "You", cli: "CLI", system: "ContextForge",
};
/** Two-letter monogram used instead of vendor logos. */
export const AGENT_MONO: Record<AgentKind, string> = {
  claude_code: "CC", codex: "CX", antigravity_cli: "AG", antigravity_ide: "AG", antigravity_app: "AG", antigravity: "AG",
  mcp_client: "MC", user: "ME", cli: "CL", system: "CF",
};
export const HANDOFF_TARGETS: AgentKind[] = ["claude_code", "codex", "antigravity_cli", "antigravity_ide", "antigravity_app"];

/** Category used in the UI, derived from kind + lifecycle status (see docs/ARCHITECTURE.md). */
export function category(kind: MemoryKind, status: MemoryStatus): string {
  if (kind === "task") return status === "completed" ? "Completed task" : status === "cancelled" ? "Cancelled task" : "Pending task";
  if (kind === "bug") return status === "open" ? "Known bug" : "Resolved bug";
  if (kind === "research") return status === "open" ? "Open question" : "Research finding";
  return KIND_LABEL[kind];
}

export function relativeTime(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const s = Math.round((now - t) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  if (s < 86400 * 14) return `${Math.round(s / 86400)}d ago`;
  return new Date(t).toLocaleDateString();
}

export function dateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function shortId(id: string): string {
  const [prefix, rest] = id.split("_");
  return rest ? `${prefix}_${rest.slice(0, 8)}…` : id;
}

export function shortSha(sha: string | null | undefined): string {
  return sha ? sha.slice(0, 8) : "—";
}
