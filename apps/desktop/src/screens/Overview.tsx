import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, ArrowRight, FolderPlus, Plug } from "lucide-react";
import { api } from "../lib/api";
import { AGENT_LABEL, FRESHNESS_LABEL, KIND_LABEL, relativeTime } from "../lib/format";
import type { AgentKind } from "../lib/types";
import { useApp } from "../App";
import { AgentChip, Badge, Button, Card, EmptyState, ErrorNote, FreshnessBadge, PageHeader, Spinner, Stat, StateBadge } from "../components/ui";

const AGENTS: { kind: AgentKind[]; label: string; provider: "claude" | "codex" | "antigravity" }[] = [
  { kind: ["claude_code"], label: "Claude Code", provider: "claude" },
  { kind: ["codex"], label: "Codex", provider: "codex" },
  { kind: ["antigravity_cli", "antigravity_ide", "antigravity_app", "antigravity"], label: "Antigravity", provider: "antigravity" },
];

export default function Overview() {
  const app = useApp();
  const ov = useQuery({ queryKey: ["overview", app.projectId], queryFn: () => api.overview(app.projectId) });
  const integ = useQuery({ queryKey: ["integrations", "user"], queryFn: () => api.integrationsStatus(null), staleTime: 60_000 });
  const review = useQuery({
    queryKey: ["review", app.projectId],
    queryFn: () => api.searchMemories({ project_id: app.projectId, freshness: ["potentially_stale", "needs_review"], limit: 6, sort: "updated" }),
  });

  if (ov.isLoading) return <Spinner />;
  if (ov.error) return <ErrorNote error={ov.error} onRetry={() => ov.refetch()} />;
  const o = ov.data!;

  if (o.projects === 0) {
    return (
      <>
        <PageHeader title="Welcome to ContextForge" subtitle="Shared, local memory for Claude Code, Codex and Antigravity." />
        <Card>
          <EmptyState icon={<FolderPlus className="size-6" />} title="Register your first project" action={
            <div className="flex gap-2">
              <Button variant="primary" onClick={() => app.go("projects")} icon={<FolderPlus className="size-4" />}>Add a project</Button>
              <Button onClick={() => app.go("integrations")} icon={<Plug className="size-4" />}>Connect agents</Button>
            </div>
          }>
            ContextForge only records activity for projects you opt in. Add a project folder, then connect your agents so their sessions share one memory.
          </EmptyState>
        </Card>
      </>
    );
  }

  const freshnessCount = (k: string) => o.by_freshness.find((f) => f.key === k)?.count ?? 0;
  return (
    <>
      <PageHeader title="Overview" subtitle={app.projectId ? "Scoped to the selected project" : `Across ${o.projects} project${o.projects === 1 ? "" : "s"}`} />
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
        <Stat label="Memories" value={o.memories} hint={o.by_kind.slice(0, 3).map((k) => `${k.count} ${KIND_LABEL[k.key].toLowerCase()}`).join(" · ") || "none yet"} />
        <Stat label="Open tasks" value={o.open_tasks} />
        <Stat label="Known bugs" value={o.open_bugs} tone={o.open_bugs ? "warn" : undefined} />
        <Stat label="Needs review" value={o.needs_review} tone={o.needs_review ? "danger" : "ok"} hint={`${freshnessCount("potentially_stale")} possibly stale · ${freshnessCount("needs_review")} review`} />
        <Stat label="Sessions" value={o.sessions_total} hint={`${o.handoffs} handoff${o.handoffs === 1 ? "" : "s"}`} />
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
        <Card title="Agents" className="lg:col-span-1" actions={<Button size="sm" variant="ghost" onClick={() => app.go("integrations")}>Manage <ArrowRight className="size-3" /></Button>}>
          <ul className="flex flex-col gap-3">
            {AGENTS.map((a) => {
              const act = o.agent_activity.filter((x) => a.kind.includes(x.agent)).sort((x, y) => y.last_event_at.localeCompare(x.last_event_at))[0];
              const state = integ.data?.overall.find(([p]) => p === a.provider)?.[1];
              const week = o.sessions_last_7d.filter((s) => a.kind.includes(s.key)).reduce((n, s) => n + s.count, 0);
              return (
                <li key={a.label} className="flex items-center gap-3">
                  <AgentChip agent={a.kind[0]} compact />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-[13px] font-medium">{a.label} {state && <StateBadge state={state} />}</div>
                    <div className="text-[11.5px] text-muted">
                      {act ? <>last event {relativeTime(act.last_event_at)} · {AGENT_LABEL[act.agent]} · {week} session{week === 1 ? "" : "s"} this week</> : "no events received yet"}
                    </div>
                  </div>
                  {act && <span className="size-2 rounded-full bg-ok pulse" title="events received" />}
                </li>
              );
            })}
          </ul>
        </Card>

        <Card title="Needs review" className="lg:col-span-2" actions={<Button size="sm" variant="ghost" onClick={() => app.go("memories")}>Open explorer <ArrowRight className="size-3" /></Button>}>
          {review.data && review.data.hits.length === 0 ? (
            <p className="text-[12.5px] text-muted">Every memory with file or commit references matches the current checkout.</p>
          ) : (
            <ul className="flex flex-col divide-y divide-line">
              {review.data?.hits.map((h) => (
                <li key={h.memory.id}>
                  <button onClick={() => app.openMemory(h.memory.id)} className="flex w-full items-start gap-3 py-2 text-left hover:bg-surface-2/60">
                    <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warn" />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px]">{h.memory.title}</div>
                      <div className="truncate text-[11.5px] text-muted">{h.memory.stale_reason ?? FRESHNESS_LABEL[h.memory.freshness]}</div>
                    </div>
                    <FreshnessBadge value={h.memory.freshness} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Card>

        <Card title="Recent sessions" className="lg:col-span-1" actions={<Button size="sm" variant="ghost" onClick={() => app.go("timeline")}>Timeline <ArrowRight className="size-3" /></Button>}>
          {o.recent_sessions.length === 0 ? <p className="text-[12.5px] text-muted">No agent sessions captured yet. Connect an agent in Integrations.</p> : (
            <ul className="flex flex-col gap-2.5">
              {o.recent_sessions.slice(0, 6).map((s) => (
                <li key={s.id} className="flex items-start gap-2.5">
                  <AgentChip agent={s.agent} compact />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-[12.5px]">
                      <span className="font-medium">{AGENT_LABEL[s.agent]}</span>
                      <span className="text-faint">{relativeTime(s.last_activity_at)}</span>
                      {!s.ended_at && <Badge tone="ok">active</Badge>}
                    </div>
                    <div className="truncate text-[11.5px] text-muted">{s.summary ?? `${s.files_touched} files · ${s.commands_run} commands`}</div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Card>

        <Card title="Recent activity" className="lg:col-span-2" padded={false}>
          {o.recent_events.length === 0 ? <p className="p-4 text-[12.5px] text-muted">Activity appears here as agents work.</p> : (
            <ul className="max-h-[320px] divide-y divide-line overflow-y-auto">
              {o.recent_events.map((e) => (
                <li key={e.id} className="flex items-center gap-3 px-4 py-2 text-[12.5px]">
                  <AgentChip agent={e.agent} compact />
                  <span className="min-w-0 flex-1 truncate">{e.summary}</span>
                  <Badge>{e.kind.replace(/_/g, " ")}</Badge>
                  <span className="w-16 shrink-0 text-right text-[11px] text-faint">{relativeTime(e.occurred_at)}</span>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
    </>
  );
}
