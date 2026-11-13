import { useEffect, useMemo, useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Brain, CheckCheck, FileCode, GitCommit, History, Lock, Pencil, Pin, PinOff, Plus, Replace, Search, ShieldAlert, Trash2 } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { AGENT_LABEL, category, CERTAINTY_LABEL, dateTime, FRESHNESS_LABEL, KIND_LABEL, KINDS, relativeTime, shortSha, STATUS_LABEL, STATUSES_FOR } from "../lib/format";
import type { AgentKind, Certainty, Freshness, MemoryDetail, MemoryKind, MemoryStatus } from "../lib/types";
import { useApp } from "../App";
import { AgentChip, Badge, Button, Card, CertaintyBadge, ConfirmModal, cx, EmptyState, ErrorNote, Field, FreshnessBadge, IconButton, Input, Modal, PageHeader, Select, Spinner, TextArea, useToast } from "../components/ui";

const AGENT_FILTERS: AgentKind[] = ["claude_code", "codex", "antigravity_cli", "antigravity_ide", "antigravity_app", "user"];
const FRESHNESS_FILTERS: Freshness[] = ["current", "potentially_stale", "needs_review", "superseded"];

function Chip({ on, onClick, children }: { on: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} aria-pressed={on} className={cx("rounded-full border px-2.5 py-0.5 text-[11.5px] transition-colors", on ? "border-accent/60 bg-accent-soft text-fg" : "border-line text-muted hover:border-line-strong hover:text-fg")}>
      {children}
    </button>
  );
}

function toggle<T>(list: T[], v: T): T[] {
  return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
}

function MemoryForm({ initial, projectId, onDone, mode }: { initial?: MemoryDetail; projectId?: string; onDone: () => void; mode: "create" | "edit" | "supersede" }) {
  const m = initial?.memory;
  const [kind, setKind] = useState<MemoryKind>(m?.kind ?? "decision");
  const [title, setTitle] = useState(m?.title ?? "");
  const [content, setContent] = useState(m?.content ?? "");
  const [status, setStatus] = useState<MemoryStatus>(m?.status ?? STATUSES_FOR.decision[0]);
  const [certainty, setCertainty] = useState<Certainty>(m?.certainty ?? "decided");
  const [tags, setTags] = useState((m?.tags ?? []).join(", "));
  const [files, setFiles] = useState("");
  const [reason, setReason] = useState("");
  const [pinned, setPinned] = useState(false);
  const qc = useQueryClient();
  const toast = useToast();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.listProjects });
  const [pid, setPid] = useState(projectId ?? m?.project_id ?? "");
  useEffect(() => { if (!pid && projects.data?.[0]) setPid(projects.data[0].project.id); }, [pid, projects.data]);
  useEffect(() => { if (!STATUSES_FOR[kind].includes(status)) setStatus(STATUSES_FOR[kind][0]); }, [kind, status]);
  const save = useMutation({
    mutationFn: async () => {
      const tagList = tags.split(",").map((t) => t.trim()).filter(Boolean);
      if (mode === "create") return api.createMemory({ projectId: pid, kind, title, content, status, certainty, tags: tagList, files: files.split("\n").map((f) => f.trim()).filter(Boolean), pinned });
      if (mode === "edit") return api.updateMemory(m!.id, { title, content, status, certainty, tags: tagList, kind, reason: "edited in the desktop app" });
      return api.supersedeMemory(m!.id, title, content, reason || "superseded in the desktop app");
    },
    onSuccess: (o) => {
      qc.invalidateQueries();
      toast("ok", o.notes.length ? `${o.action === "created" ? "Saved" : "Updated"} — ${o.notes.join("; ")}` : "Saved");
      onDone();
    },
  });
  return (
    <div className="flex flex-col gap-3">
      {mode === "create" && (
        <Field label="Project">{(id) => <Select id={id} value={pid} onChange={(e) => setPid(e.target.value)} options={(projects.data ?? []).map((p) => ({ value: p.project.id, label: p.project.name }))} />}</Field>
      )}
      <div className="grid grid-cols-3 gap-3">
        <Field label="Kind">{(id) => <Select id={id} value={kind} disabled={mode === "supersede"} onChange={(e) => setKind(e.target.value as MemoryKind)} options={KINDS.map((k) => ({ value: k, label: KIND_LABEL[k] }))} />}</Field>
        <Field label="Status">{(id) => <Select id={id} value={status} disabled={mode === "supersede"} onChange={(e) => setStatus(e.target.value as MemoryStatus)} options={STATUSES_FOR[kind].map((s) => ({ value: s, label: STATUS_LABEL[s] }))} />}</Field>
        <Field label="Certainty">{(id) => <Select id={id} value={certainty} disabled={mode === "supersede"} onChange={(e) => setCertainty(e.target.value as Certainty)} options={(["verified", "decided", "unverified", "hypothesis"] as Certainty[]).map((c) => ({ value: c, label: CERTAINTY_LABEL[c] }))} />}</Field>
      </div>
      <Field label="Title">{(id) => <Input id={id} value={title} onChange={(e) => setTitle(e.target.value)} maxLength={200} placeholder="Short and specific" />}</Field>
      <Field label="Content" hint="Secrets are redacted automatically before saving.">{(id) => <TextArea id={id} rows={6} value={content} onChange={(e) => setContent(e.target.value)} />}</Field>
      {mode !== "supersede" && <Field label="Tags (comma separated)">{(id) => <Input id={id} value={tags} onChange={(e) => setTags(e.target.value)} />}</Field>}
      {mode === "create" && (
        <>
          <Field label="Related files (one per line, relative to the project root)" hint="ContextForge snapshots these files and flags the memory when they change.">
            {(id) => <TextArea id={id} rows={2} value={files} onChange={(e) => setFiles(e.target.value)} className="font-mono text-[12px]" />}
          </Field>
          <label className="flex items-center gap-2 text-[12.5px]"><input type="checkbox" checked={pinned} onChange={(e) => setPinned(e.target.checked)} /> Pin (always included in agent context)</label>
        </>
      )}
      {mode === "supersede" && <Field label="Why is the old memory outdated?">{(id) => <Input id={id} value={reason} onChange={(e) => setReason(e.target.value)} />}</Field>}
      {save.error && <ErrorNote error={errorMessage(save.error)} />}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>Cancel</Button>
        <Button variant="primary" disabled={!title.trim() || (mode === "create" && !pid)} loading={save.isPending} onClick={() => save.mutate()}>
          {mode === "create" ? "Save memory" : mode === "edit" ? "Save changes" : "Supersede"}
        </Button>
      </div>
    </div>
  );
}

type Tab = "provenance" | "files" | "history";

function Detail({ id, onClose }: { id: string; onClose: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const app = useApp();
  const [tab, setTab] = useState<Tab>("provenance");
  const [mode, setMode] = useState<"edit" | "supersede" | null>(null);
  const [del, setDel] = useState(false);
  const { data: d, isLoading, error } = useQuery({ queryKey: ["memory", id], queryFn: () => api.getMemory(id) });
  const act = (fn: () => Promise<unknown>, ok: string) => fn().then(() => { qc.invalidateQueries(); toast("ok", ok); }).catch((e) => toast("error", errorMessage(e)));
  const remove = useMutation({ mutationFn: () => api.deleteMemory(id), onSuccess: () => { qc.invalidateQueries(); toast("ok", "Memory deleted"); setDel(false); onClose(); } });
  if (isLoading) return <Spinner />;
  if (error || !d) return <ErrorNote error={errorMessage(error ?? "not found")} />;
  const m = d.memory;
  const openFindings = d.findings.filter((f) => !f.resolved_at);
  return (
    <div className="flex flex-col gap-3 anim-in">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="mb-1 flex flex-wrap items-center gap-1.5">
            <Badge tone="accent">{category(m.kind, m.status)}</Badge>
            {m.kind === "task" || m.kind === "bug" || m.kind === "research" ? <Badge>{STATUS_LABEL[m.status]}</Badge> : null}
            <CertaintyBadge value={m.certainty} />
            <FreshnessBadge value={m.freshness} />
            {m.pinned && <Badge tone="accent"><Pin className="size-3" /> pinned</Badge>}
            {m.user_locked && <Badge title="Written or edited by you: agents cannot rewrite it"><Lock className="size-3" /> yours</Badge>}
          </div>
          <h2 className="text-[16px] font-semibold leading-snug">{m.title}</h2>
          <p className="mt-1 font-mono text-[11px] text-faint">{m.id} · rev {m.revision}{m.reinforced_count ? ` · reinforced ${m.reinforced_count}×` : ""}</p>
        </div>
        <div className="flex shrink-0 gap-0.5">
          <IconButton label={m.pinned ? "Unpin" : "Pin"} onClick={() => act(() => api.setPinned(m.id, !m.pinned), m.pinned ? "Unpinned" : "Pinned")}>{m.pinned ? <PinOff className="size-4" /> : <Pin className="size-4" />}</IconButton>
          <IconButton label="Edit" onClick={() => setMode("edit")}><Pencil className="size-4" /></IconButton>
          <IconButton label="Supersede" disabled={m.freshness === "superseded"} onClick={() => setMode("supersede")}><Replace className="size-4" /></IconButton>
          <IconButton label="Revalidate (still accurate)" disabled={m.freshness === "superseded"} onClick={() => act(() => api.revalidateMemory(m.id, "Confirmed in the desktop app"), "Marked current; snapshots refreshed")}><CheckCheck className="size-4" /></IconButton>
          <IconButton label="Delete" onClick={() => setDel(true)} className="hover:text-danger"><Trash2 className="size-4" /></IconButton>
        </div>
      </div>

      {m.injection_flag && (
        <div className="flex gap-2 rounded-md border border-warn/40 bg-warn-soft p-2.5 text-[12px] text-warn">
          <ShieldAlert className="size-4 shrink-0" /> This memory contains instruction-like text. Its body is withheld from automatic agent context. Edit it to remove the instructions, or keep it for reference only.
        </div>
      )}
      {openFindings.length > 0 && (
        <div className="rounded-md border border-warn/40 bg-warn-soft p-2.5 text-[12px]">
          <div className="mb-1 font-medium text-warn">Why this may be outdated</div>
          <ul className="list-disc pl-5 text-fg">{openFindings.map((f) => <li key={f.id}>{f.detail} <span className="text-faint">({relativeTime(f.detected_at)}{f.branch ? ` on ${f.branch}` : ""})</span></li>)}</ul>
          <p className="mt-1.5 text-muted">Revalidate if it is still accurate, or supersede it with a corrected memory.</p>
        </div>
      )}
      {m.superseded_by && <button onClick={() => app.openMemory(m.superseded_by!)} className="text-left text-[12px] text-accent-fg hover:underline">Superseded by {m.superseded_by} →</button>}

      <div className="whitespace-pre-wrap rounded-md border border-line bg-surface-2 p-3 text-[13px] leading-relaxed">{m.content || <span className="text-faint">No content.</span>}</div>
      {m.tags.length > 0 && <div className="flex flex-wrap gap-1">{m.tags.map((t) => <Badge key={t}>#{t}</Badge>)}</div>}

      <div role="tablist" className="flex gap-1 border-b border-line">
        {(["provenance", "files", "history"] as Tab[]).map((t) => (
          <button key={t} role="tab" aria-selected={tab === t} onClick={() => setTab(t)} className={cx("-mb-px border-b-2 px-3 py-1.5 text-[12.5px] capitalize", tab === t ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg")}>
            {t === "files" ? `Files & commits (${d.files.length + d.commits.length})` : t === "history" ? `History (${d.revisions.length})` : "Provenance"}
          </button>
        ))}
      </div>
      {tab === "provenance" && (
        <div className="flex flex-col gap-3 text-[12.5px]">
          <dl className="grid grid-cols-[130px_1fr] gap-x-3 gap-y-1.5">
            <dt className="text-muted">Recorded by</dt><dd className="flex items-center gap-2"><AgentChip agent={m.source_agent} /> <span className="text-faint">via {m.source_origin}</span>{typeof m.metadata.mcp_client === "string" && <Badge>{m.metadata.mcp_client}</Badge>}</dd>
            <dt className="text-muted">Created</dt><dd>{dateTime(m.created_at)}{m.origin_branch ? ` on ${m.origin_branch}` : ""}</dd>
            <dt className="text-muted">Updated</dt><dd>{dateTime(m.updated_at)}</dd>
            <dt className="text-muted">Last validated</dt><dd>{dateTime(m.last_validated_at)} {m.validated_commit && <span className="font-mono text-faint">@ {shortSha(m.validated_commit)}</span>}</dd>
            {m.branch && <><dt className="text-muted">Branch scope</dt><dd><Badge tone="info">{m.branch}</Badge></dd></>}
            {d.source_session && <><dt className="text-muted">Session</dt><dd>{AGENT_LABEL[d.source_session.agent]} · started {dateTime(d.source_session.started_at)}{d.source_session.model ? ` · ${d.source_session.model}` : ""}</dd></>}
          </dl>
          <div>
            <h3 className="mb-1.5 text-[12px] font-semibold text-muted">Evidence ({d.evidence.length})</h3>
            {d.evidence.length === 0 ? <p className="text-faint">No evidence recorded{m.certainty === "verified" ? "" : " — this memory is not verified"}.</p> : (
              <ul className="flex flex-col gap-1.5">{d.evidence.map((ev) => (
                <li key={ev.id} className="rounded-md border border-line bg-surface-2 px-2.5 py-1.5">
                  <div className="flex items-center gap-2"><Badge>{ev.kind.replace(/_/g, " ")}</Badge><span className="min-w-0 flex-1 truncate font-mono text-[11.5px]">{ev.reference}</span><span className="text-[11px] text-faint">{AGENT_LABEL[ev.agent]} · {relativeTime(ev.captured_at)}</span></div>
                  {ev.excerpt && <p className="mt-1 text-[12px] text-muted">{ev.excerpt}</p>}
                </li>
              ))}</ul>
            )}
          </div>
          {d.links.length > 0 && (
            <div>
              <h3 className="mb-1.5 text-[12px] font-semibold text-muted">Relationships</h3>
              <ul className="flex flex-col gap-1">{d.links.map((l) => {
                const other = l.from_id === m.id ? l.to_id : l.from_id;
                return <li key={l.from_id + l.to_id + l.relation}><button className="text-left hover:underline" onClick={() => app.openMemory(other)}><Badge>{l.from_id === m.id ? l.relation.replace(/_/g, " ") : `${l.relation.replace(/_/g, " ")} (by)`}</Badge> {l.other_title ?? other}</button></li>;
              })}</ul>
            </div>
          )}
        </div>
      )}
      {tab === "files" && (
        <div className="flex flex-col gap-2 text-[12.5px]">
          {d.files.length === 0 && d.commits.length === 0 && <p className="text-faint">No file or commit references — freshness cannot be tracked automatically for this memory.</p>}
          {d.files.map((f) => (
            <div key={f.path} className="flex items-center gap-2 rounded-md border border-line bg-surface-2 px-2.5 py-1.5">
              <FileCode className="size-4 text-muted" /><span className="flex-1 font-mono text-[12px]">{f.path}{f.line_start ? `:${f.line_start}${f.line_end ? `-${f.line_end}` : ""}` : ""}</span>
              {f.sensitive ? <Badge tone="warn">sensitive · content never stored</Badge> : <span className="font-mono text-[11px] text-faint" title="SHA-256 of the file when last validated">{f.content_hash ? `snapshot ${f.content_hash.slice(0, 10)}` : "no snapshot"}</span>}
            </div>
          ))}
          {d.commits.map((c) => <div key={c} className="flex items-center gap-2 rounded-md border border-line bg-surface-2 px-2.5 py-1.5"><GitCommit className="size-4 text-muted" /><span className="font-mono text-[12px]">{c}</span></div>)}
          {d.findings.length > 0 && (
            <>
              <h3 className="mt-2 text-[12px] font-semibold text-muted">Freshness findings</h3>
              {d.findings.map((f) => (
                <div key={f.id} className="rounded-md border border-line px-2.5 py-1.5 text-[12px]">
                  <div className="flex items-center gap-2"><Badge tone={f.resolved_at ? "neutral" : "warn"}>{f.kind.replace(/_/g, " ")}</Badge><span className="flex-1">{f.detail}</span></div>
                  <div className="mt-0.5 text-[11px] text-faint">detected {dateTime(f.detected_at)}{f.head_commit ? ` @ ${shortSha(f.head_commit)}` : ""}{f.resolved_at ? ` · resolved (${f.resolution}) ${relativeTime(f.resolved_at)}` : " · open"}</div>
                </div>
              ))}
            </>
          )}
        </div>
      )}
      {tab === "history" && (
        <ol className="flex flex-col gap-2">{d.revisions.map((r) => (
          <li key={r.revision} className="rounded-md border border-line bg-surface-2 p-2.5 text-[12px]">
            <div className="flex items-center gap-2"><History className="size-3.5 text-muted" /><b>rev {r.revision}</b><span className="text-muted">{r.reason}</span><span className="ml-auto text-faint">{r.changed_by} · {dateTime(r.changed_at)}</span></div>
            <div className="mt-1 font-medium">{r.title}</div>
            <div className="mt-0.5 line-clamp-3 whitespace-pre-wrap text-muted">{r.content}</div>
            <div className="mt-1 flex gap-1"><Badge>{STATUS_LABEL[r.status]}</Badge><Badge>{CERTAINTY_LABEL[r.certainty]}</Badge></div>
          </li>
        ))}</ol>
      )}

      <Modal open={mode !== null} onClose={() => setMode(null)} title={mode === "edit" ? "Edit memory" : "Supersede memory"} wide>
        {mode && <MemoryForm key={mode} initial={d} mode={mode} onDone={() => setMode(null)} />}
      </Modal>
      <ConfirmModal open={del} onClose={() => setDel(false)} onConfirm={() => remove.mutate()} busy={remove.isPending} title="Delete memory permanently?" confirmLabel="Delete"
        body={<>“{m.title}” and its evidence and history will be deleted. This cannot be undone. Superseding keeps history instead.</>} />
    </div>
  );
}

export default function Memories() {
  const app = useApp();
  const [text, setText] = useState("");
  const [kinds, setKinds] = useState<MemoryKind[]>([]);
  const [agents, setAgents] = useState<AgentKind[]>([]);
  const [fresh, setFresh] = useState<Freshness[]>([]);
  const [pinnedOnly, setPinnedOnly] = useState(false);
  const [selected, setSelected] = useState<string | null>(app.focusMemory);
  const [creating, setCreating] = useState(false);
  useEffect(() => { if (app.focusMemory) { setSelected(app.focusMemory); app.clearFocus(); } }, [app]);
  const q = useQuery({
    queryKey: ["memories", app.projectId, text, kinds, agents, fresh, pinnedOnly],
    queryFn: () => api.searchMemories({
      project_id: app.projectId, text: text.trim() || null, mode: "all", kinds, agents, freshness: fresh, pinned_only: pinnedOnly,
      include_superseded: fresh.includes("superseded"), limit: 100, sort: text.trim() ? "relevance" : "updated",
    }),
    placeholderData: keepPreviousData,
  });
  const hits = useMemo(() => q.data?.hits ?? [], [q.data]);
  useEffect(() => { if (!selected && hits[0]) setSelected(hits[0].memory.id); }, [hits, selected]);

  return (
    <>
      <PageHeader title="Memory" subtitle="Decisions, conventions, facts, tasks, bugs, notes, commands and research — with provenance" actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setCreating(true)}>New memory</Button>} />
      <div className="mb-3 flex flex-col gap-2.5">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-faint" />
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Full-text search (prefix matching on the last word)…" className="pl-8" aria-label="Search memories" />
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="mr-1 text-[11px] uppercase tracking-wider text-faint">Kind</span>
          {KINDS.map((k) => <Chip key={k} on={kinds.includes(k)} onClick={() => setKinds(toggle(kinds, k))}>{KIND_LABEL[k]}</Chip>)}
          <span className="ml-3 mr-1 text-[11px] uppercase tracking-wider text-faint">Freshness</span>
          {FRESHNESS_FILTERS.map((f) => <Chip key={f} on={fresh.includes(f)} onClick={() => setFresh(toggle(fresh, f))}>{FRESHNESS_LABEL[f]}</Chip>)}
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="mr-1 text-[11px] uppercase tracking-wider text-faint">Agent</span>
          {AGENT_FILTERS.map((a) => <Chip key={a} on={agents.includes(a)} onClick={() => setAgents(toggle(agents, a))}>{AGENT_LABEL[a]}</Chip>)}
          <span className="ml-3" />
          <Chip on={pinnedOnly} onClick={() => setPinnedOnly(!pinnedOnly)}>Pinned only</Chip>
        </div>
      </div>
      {q.error ? <ErrorNote error={errorMessage(q.error)} onRetry={() => q.refetch()} /> : (
        <div className="grid min-h-[480px] grid-cols-[minmax(320px,420px)_1fr] gap-4">
          <Card padded={false} className="overflow-hidden">
            <div className="border-b border-line px-3 py-2 text-[11.5px] text-muted">{q.isFetching ? "Searching…" : `${q.data?.total ?? 0} memor${q.data?.total === 1 ? "y" : "ies"}`}</div>
            {hits.length === 0 && !q.isLoading ? (
              <EmptyState icon={<Brain className="size-6" />} title={text || kinds.length || agents.length || fresh.length ? "No memories match" : "No memories yet"}>
                Agents record memories through the ContextForge MCP tools; you can also add them here.
              </EmptyState>
            ) : (
              <ul className="max-h-[calc(100vh-330px)] divide-y divide-line overflow-y-auto">
                {hits.map((h) => (
                  <li key={h.memory.id}>
                    <button onClick={() => setSelected(h.memory.id)} aria-current={selected === h.memory.id} className={cx("w-full px-3 py-2.5 text-left transition-colors", selected === h.memory.id ? "bg-accent-soft" : "hover:bg-surface-2")}>
                      <div className="flex items-center gap-1.5">
                        <Badge tone="accent">{category(h.memory.kind, h.memory.status)}</Badge>
                        {h.memory.freshness !== "current" && <FreshnessBadge value={h.memory.freshness} />}
                        {h.memory.pinned && <Pin className="size-3 text-accent" />}
                        <span className="ml-auto"><AgentChip agent={h.memory.source_agent} compact /></span>
                      </div>
                      <div className="mt-1 line-clamp-2 text-[13px] font-medium leading-snug">{h.memory.title}</div>
                      <div className="mt-0.5 line-clamp-1 text-[11.5px] text-muted">{h.snippet?.replace(/[«»]/g, "") ?? h.memory.content}</div>
                      <div className="mt-1 text-[10.5px] text-faint">{relativeTime(h.memory.updated_at)}{h.files.length ? ` · ${h.files.length} file${h.files.length > 1 ? "s" : ""}` : ""}</div>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Card>
          <Card>{selected ? <Detail key={selected} id={selected} onClose={() => setSelected(null)} /> : <EmptyState title="Select a memory" />}</Card>
        </div>
      )}
      <Modal open={creating} onClose={() => setCreating(false)} title="New memory" wide>
        {creating && <MemoryForm mode="create" projectId={app.projectId ?? undefined} onDone={() => setCreating(false)} />}
      </Modal>
    </>
  );
}
