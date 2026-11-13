import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Activity, ChevronRight, FileCode, GitBranch, GitCommit, Terminal } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { useTilde } from "../lib/paths";
import { AGENT_LABEL, dateTime, relativeTime, shortSha } from "../lib/format";
import type { AgentKind, SessionInfo } from "../lib/types";
import { useApp } from "../App";
import { AgentChip, Badge, Card, cx, EmptyState, ErrorNote, PageHeader, Select, Spinner } from "../components/ui";

const EVENT_ICON: Record<string, React.ReactNode> = {
  file_edit: <FileCode className="size-3.5" />,
  command: <Terminal className="size-3.5" />,
  commits: <GitCommit className="size-3.5" />,
};

function SessionRow({ s, open, onToggle }: { s: SessionInfo; open: boolean; onToggle: () => void }) {
  const app = useApp();
  const tilde = useTilde();
  const detail = useQuery({ queryKey: ["session", s.id], queryFn: () => api.sessionDetail(s.id), enabled: open });
  return (
    <li className="rounded-lg border border-line bg-surface">
      <button onClick={onToggle} aria-expanded={open} className="flex w-full items-center gap-3 px-4 py-3 text-left">
        <ChevronRight className={cx("size-4 text-muted transition-transform", open && "rotate-90")} />
        <AgentChip agent={s.agent} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-[12.5px]">
            <span className="text-muted">{dateTime(s.started_at)}</span>
            {s.branch && <Badge tone="info"><GitBranch className="size-3" />{s.branch}</Badge>}
            {s.ended_at ? <Badge>ended{s.end_reason ? ` · ${s.end_reason}` : ""}</Badge> : <Badge tone="ok">active</Badge>}
            {s.model && <span className="truncate text-[11px] text-faint">{s.model}</span>}
          </div>
          <div className="mt-0.5 truncate text-[12.5px]">{s.summary ?? <span className="text-faint">No summary recorded</span>}</div>
        </div>
        <div className="flex shrink-0 gap-3 text-[11.5px] text-muted">
          <span>{s.turns} turns</span><span>{s.files_touched} files</span><span>{s.commands_run} cmds</span>
          <span className="w-16 text-right text-faint">{relativeTime(s.last_activity_at)}</span>
        </div>
      </button>
      {open && (
        <div className="border-t border-line px-4 py-3">
          {detail.isLoading ? <Spinner /> : detail.error ? <ErrorNote error={errorMessage(detail.error)} /> : detail.data && (
            <div className="grid grid-cols-[1fr_280px] gap-4">
              <ol className="relative flex flex-col gap-2 border-l border-line pl-4">
                {detail.data.events.map((e) => (
                  <li key={e.id} className="relative text-[12.5px]">
                    <span className="absolute -left-[22px] top-1 flex size-3 items-center justify-center rounded-full border border-line-strong bg-surface-2" />
                    <div className="flex items-start gap-2">
                      <span className="mt-0.5 text-muted">{EVENT_ICON[e.kind] ?? <Activity className="size-3.5" />}</span>
                      <span className="flex-1 break-words">{e.summary}</span>
                      <span className="shrink-0 text-[11px] text-faint">{new Date(e.occurred_at).toLocaleTimeString()}</span>
                    </div>
                  </li>
                ))}
              </ol>
              <div className="flex flex-col gap-3 text-[12px]">
                {(s.start_commit || s.end_commit) && (
                  <div><div className="mb-1 font-semibold text-muted">Git</div><div className="font-mono">{shortSha(s.start_commit)} → {shortSha(s.end_commit)}</div></div>
                )}
                <div>
                  <div className="mb-1 font-semibold text-muted">Files touched ({detail.data.files.length})</div>
                  {detail.data.files.length === 0 ? <span className="text-faint">none</span> : (
                    <ul className="flex flex-col gap-0.5">{detail.data.files.map((f) => <li key={f.path} className="truncate font-mono text-[11.5px]" title={f.path}>{f.path} <span className="text-faint">{f.action}{f.touches > 1 ? ` ×${f.touches}` : ""}</span>{f.sensitive && <Badge tone="warn">sensitive</Badge>}</li>)}</ul>
                  )}
                </div>
                <div>
                  <div className="mb-1 font-semibold text-muted">Memories recorded ({detail.data.memories.length})</div>
                  {detail.data.memories.length === 0 ? <span className="text-faint">none</span> : (
                    <ul className="flex flex-col gap-1">{detail.data.memories.map(([id, kind, title]) => <li key={id}><button className="text-left hover:underline" onClick={() => app.openMemory(id)}><Badge>{kind}</Badge> {title}</button></li>)}</ul>
                  )}
                </div>
                {s.cwd && <div><div className="mb-1 font-semibold text-muted">Working directory</div><div className="break-all font-mono text-[11px]">{tilde(s.cwd)}</div></div>}
              </div>
            </div>
          )}
        </div>
      )}
    </li>
  );
}

export default function Timeline() {
  const app = useApp();
  const [agent, setAgent] = useState<AgentKind | "">("");
  const [open, setOpen] = useState<string | null>(null);
  const [limit, setLimit] = useState(50);
  const q = useQuery({
    queryKey: ["sessions", app.projectId, agent, limit],
    queryFn: () => api.listSessions({ project_id: app.projectId, agent: agent || null, limit }),
  });
  const handoffs = useQuery({ queryKey: ["handoffs", app.projectId], queryFn: () => api.listHandoffs(app.projectId) });
  return (
    <>
      <PageHeader title="Session timeline" subtitle="Captured activity from every connected agent" actions={
        <Select aria-label="Filter by agent" value={agent} onChange={(e) => setAgent(e.target.value as AgentKind | "")} className="w-48"
          options={[{ value: "", label: "All agents" }, ...(["claude_code", "codex", "antigravity_cli", "antigravity_ide", "antigravity_app"] as AgentKind[]).map((a) => ({ value: a, label: AGENT_LABEL[a] }))]} />
      } />
      {q.isLoading ? <Spinner /> : q.error ? <ErrorNote error={errorMessage(q.error)} onRetry={() => q.refetch()} /> : !q.data?.length ? (
        <Card><EmptyState icon={<Activity className="size-6" />} title="No sessions captured yet">Sessions appear when a connected agent starts in a registered project. Set up hooks in Integrations.</EmptyState></Card>
      ) : (
        <div className="grid grid-cols-[1fr_260px] gap-4">
          <div>
            <ul className="flex flex-col gap-2">{q.data.map((s) => <SessionRow key={s.id} s={s} open={open === s.id} onToggle={() => setOpen(open === s.id ? null : s.id)} />)}</ul>
            {q.data.length >= limit && <button className="mt-3 text-[12.5px] text-accent-fg hover:underline" onClick={() => setLimit(limit + 50)}>Load more</button>}
          </div>
          <Card title="Handoff history">
            {!handoffs.data?.length ? <p className="text-[12px] text-muted">No handoffs generated yet.</p> : (
              <ul className="flex flex-col gap-2 text-[12px]">{handoffs.data.slice(0, 15).map((h) => (
                <li key={h.id}><button className="w-full text-left hover:underline" onClick={() => app.go("handoffs")}>
                  <div className="flex items-center gap-1.5">{AGENT_LABEL[h.requested_by]} → <b>{h.target_agent ? AGENT_LABEL[h.target_agent] : "any agent"}</b></div>
                  <div className="text-faint">{relativeTime(h.created_at)} · ~{h.token_estimate} tokens{h.branch ? ` · ${h.branch}` : ""}</div>
                </button></li>
              ))}</ul>
            )}
          </Card>
        </div>
      )}
    </>
  );
}
