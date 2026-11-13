import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Send, Sparkles } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { AGENT_LABEL, dateTime, HANDOFF_TARGETS, relativeTime } from "../lib/format";
import type { AgentKind, Handoff } from "../lib/types";
import { useApp } from "../App";
import { Button, Card, cx, EmptyState, ErrorNote, Field, Input, PageHeader, Select, Spinner, useToast } from "../components/ui";

export default function Handoffs() {
  const app = useApp();
  const qc = useQueryClient();
  const toast = useToast();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.listProjects });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
  const [projectId, setProjectId] = useState<string>(app.projectId ?? "");
  const [target, setTarget] = useState<AgentKind>("codex");
  const [budget, setBudget] = useState<number | "">("");
  const [current, setCurrent] = useState<Handoff | null>(null);
  const [copied, setCopied] = useState(false);
  useEffect(() => { if (!projectId && projects.data?.[0]) setProjectId(app.projectId ?? projects.data[0].project.id); }, [projects.data, projectId, app.projectId]);
  const history = useQuery({ queryKey: ["handoffs", projectId], queryFn: () => api.listHandoffs(projectId || null), enabled: !!projectId });
  const gen = useMutation({
    mutationFn: () => api.generateHandoff(projectId, target, budget === "" ? undefined : budget),
    onSuccess: (h) => { setCurrent(h); qc.invalidateQueries({ queryKey: ["handoffs"] }); },
    onError: (e) => toast("error", errorMessage(e)),
  });
  const openOld = (id: string) => api.getHandoff(id).then(setCurrent).catch((e) => toast("error", errorMessage(e)));
  const copy = async () => {
    if (!current) return;
    await navigator.clipboard.writeText(current.markdown);
    setCopied(true);
    toast("ok", "Handoff copied — paste it into the next agent if it is not connected");
    setTimeout(() => setCopied(false), 1500);
  };

  if (projects.isLoading) return <Spinner />;
  if (!projects.data?.length) return <><PageHeader title="Handoff center" /><Card><EmptyState icon={<Send className="size-6" />} title="Register a project first" action={<Button onClick={() => app.go("projects")}>Go to Projects</Button>} /></Card></>;

  return (
    <>
      <PageHeader title="Handoff center" subtitle="Structured briefs for moving work between Claude Code, Codex and Antigravity" />
      <div className="grid grid-cols-[300px_1fr] gap-4">
        <div className="flex flex-col gap-4">
          <Card title="Generate">
            <div className="flex flex-col gap-3">
              <Field label="Project">{(id) => <Select id={id} value={projectId} onChange={(e) => { setProjectId(e.target.value); setCurrent(null); }} options={projects.data!.map((p) => ({ value: p.project.id, label: p.project.name }))} />}</Field>
              <Field label="Next agent">{(id) => <Select id={id} value={target} onChange={(e) => setTarget(e.target.value as AgentKind)} options={HANDOFF_TARGETS.map((a) => ({ value: a, label: AGENT_LABEL[a] }))} />}</Field>
              <Field label="Token budget" hint={`Default ${settings.data?.context.handoff_budget_tokens ?? 1500}`}>
                {(id) => <Input id={id} type="number" min={300} max={12000} value={budget} onChange={(e) => setBudget(e.target.value ? Number(e.target.value) : "")} />}
              </Field>
              <Button variant="primary" icon={<Sparkles className="size-4" />} loading={gen.isPending} disabled={!projectId} onClick={() => gen.mutate()}>Generate handoff</Button>
              <p className="text-[11.5px] text-faint">Connected agents also receive context automatically at session start and can call generate_handoff over MCP.</p>
            </div>
          </Card>
          <Card title="History" padded={false}>
            {!history.data?.length ? <p className="p-4 text-[12px] text-muted">No handoffs yet.</p> : (
              <ul className="max-h-[360px] divide-y divide-line overflow-y-auto">{history.data.map((h) => (
                <li key={h.id}><button onClick={() => openOld(h.id)} className={cx("w-full px-4 py-2 text-left text-[12px] hover:bg-surface-2", current?.id === h.id && "bg-accent-soft")}>
                  <div>{AGENT_LABEL[h.requested_by]} → <b>{h.target_agent ? AGENT_LABEL[h.target_agent] : "any"}</b></div>
                  <div className="text-faint">{relativeTime(h.created_at)} · ~{h.token_estimate} tokens</div>
                </button></li>
              ))}</ul>
            )}
          </Card>
        </div>
        <Card title={current ? `Handoff → ${current.target_agent ? AGENT_LABEL[current.target_agent] : "next agent"}` : "Preview"} actions={current && (
          <Button size="sm" variant="primary" icon={copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />} onClick={copy}>{copied ? "Copied" : "Copy markdown"}</Button>
        )}>
          {gen.error ? <ErrorNote error={errorMessage(gen.error)} /> : !current ? (
            <EmptyState icon={<Send className="size-6" />} title="Generate a handoff">It includes project identity, branch, last activity, decisions, completed work, active issues, pending tasks, relevant files, possibly stale assumptions and suggested next steps.</EmptyState>
          ) : (
            <>
              <p className="mb-2 text-[12px] text-muted">Generated {dateTime(current.created_at)} · ~{current.token_estimate} tokens · {current.id}</p>
              <pre className="prose-brief max-h-[calc(100vh-260px)] overflow-y-auto rounded-md border border-line bg-surface-2 p-4">{current.markdown}</pre>
            </>
          )}
        </Card>
      </div>
    </>
  );
}
