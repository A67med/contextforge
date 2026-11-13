import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, CircleAlert, History, Plug, RefreshCw, ShieldCheck, Stethoscope, Wrench, XCircle } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { useTilde, useTildeText } from "../lib/paths";
import { dateTime, relativeTime } from "../lib/format";
import type { Action, AgentKind, PlanView, Provider, ProviderStatus } from "../lib/types";
import { AgentChip, Badge, Button, Card, ConfirmModal, ErrorNote, Modal, PageHeader, Select, Spinner, StateBadge, useToast } from "../components/ui";

const AGENT_OF: Record<Provider, AgentKind> = { claude: "claude_code", codex: "codex", antigravity: "antigravity_cli" };
const ACTIVITY_AGENTS: Record<Provider, AgentKind[]> = {
  claude: ["claude_code"], codex: ["codex"], antigravity: ["antigravity_cli", "antigravity_ide", "antigravity_app", "antigravity"],
};

function DiffView({ text }: { text: string }) {
  return (
    <pre className="prose-brief max-h-[46vh] overflow-auto rounded-md border border-line bg-surface-2 p-3 text-[11.5px]">
      {text.split("\n").map((l, i) => (
        <div key={i} className={l.startsWith("+ ") ? "bg-ok-soft text-ok" : l.startsWith("- ") ? "bg-danger-soft text-danger" : l.startsWith("===") ? "mt-2 font-semibold text-fg" : "text-muted"}>{l || " "}</div>
      ))}
    </pre>
  );
}

function ProviderCard({ s, scopePath, lastEvent }: { s: ProviderStatus; scopePath: string | null; lastEvent: string | null }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [busy, setBusy] = useState<Action | null>(null);
  const tilde = useTilde();
  const tildeText = useTildeText();
  const makePlan = async (action: Action) => {
    setBusy(action);
    try {
      setPlan(await api.integrationPlan(s.provider, action, scopePath));
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  const apply = useMutation({
    mutationFn: () => api.integrationApply(plan!.plan),
    onSuccess: (r) => {
      toast("ok", `${s.label}: ${plan!.plan.action} applied${r.backup_id ? ` (backup ${r.backup_id})` : ""}`);
      setPlan(null);
      qc.invalidateQueries({ queryKey: ["integrations"] });
    },
    onError: (e) => toast("error", errorMessage(e)),
  });
  const overall = s.mcp.state === "installed" && s.hooks.state === "installed" ? (s.trust?.state === "pending_user_action" ? "pending_user_action" : "installed")
    : s.mcp.state === "not_installed" && s.hooks.state === "not_installed" ? "not_installed" : s.mcp.state === "error" || s.hooks.state === "error" ? "error" : "partial";
  const installedAgent = s.installations.some((i) => i.installed);
  return (
    <Card title={<span className="flex items-center gap-2"><AgentChip agent={AGENT_OF[s.provider]} compact /> {s.label} <StateBadge state={overall} /></span>} actions={<>
      {overall === "not_installed" ? (
        <Button size="sm" variant="primary" icon={<Plug className="size-3.5" />} loading={busy === "install"} onClick={() => makePlan("install")}>Install</Button>
      ) : (
        <>
          <Button size="sm" icon={<Wrench className="size-3.5" />} loading={busy === "repair"} onClick={() => makePlan("repair")}>Repair</Button>
          <Button size="sm" variant="ghost" loading={busy === "uninstall"} onClick={() => makePlan("uninstall")}>Uninstall</Button>
        </>
      )}
    </>}>
      <div className="flex flex-col gap-3 text-[12.5px]">
        <ul className="flex flex-col gap-1">
          {s.installations.map((i) => (
            <li key={i.id} className="flex items-center gap-2">
              {i.installed ? <CheckCircle2 className="size-3.5 text-ok" /> : <XCircle className="size-3.5 text-faint" />}
              <span className="font-medium">{i.label}</span>
              <span className="text-muted">{i.installed ? (i.version ? `v${i.version}` : "version unknown") : "not found"}</span>
              {i.executable && <span className="truncate font-mono text-[11px] text-faint" title={tilde(i.executable)}>{tilde(i.executable)}</span>}
            </li>
          ))}
        </ul>
        {!installedAgent && <p className="text-[12px] text-warn">The agent was not detected on this machine. You can still install the configuration for later.</p>}
        <div className="grid grid-cols-[90px_1fr] gap-x-3 gap-y-1.5">
          <span className="text-muted">MCP server</span><span className="flex items-start gap-2"><StateBadge state={s.mcp.state} /><span className="text-muted">{s.mcp.detail}</span></span>
          <span className="text-muted">Hooks</span><span className="flex items-start gap-2"><StateBadge state={s.hooks.state} /><span className="text-muted">{s.hooks.detail}</span></span>
          {s.trust && <><span className="text-muted">Trust</span><span className="flex items-start gap-2"><StateBadge state={s.trust.state} /><span className="text-muted">{s.trust.detail}</span></span></>}
          <span className="text-muted">Events</span><span className="text-muted">{lastEvent ? `last hook event ${relativeTime(lastEvent)}` : "no hook events received yet"}</span>
        </div>
        {s.trust?.state === "pending_user_action" && (
          <div className="flex gap-2 rounded-md border border-info/40 bg-info-soft p-2.5 text-[12px] text-info">
            <ShieldCheck className="size-4 shrink-0" /> Codex only runs hooks you have reviewed. Open Codex and run <code className="font-mono">/hooks</code> (or Settings → Hooks in the Codex app) to review and trust the ContextForge hooks. ContextForge never approves them for you.
          </div>
        )}
        {s.notes.map((n) => <p key={n} className="text-[11.5px] text-faint">{n}</p>)}
        <div className="font-mono text-[11px] text-faint">{[s.mcp.path, s.hooks.path].filter((v, i, a) => v && a.indexOf(v) === i).map((p) => tilde(p)).join(" · ")}</div>
      </div>
      <Modal open={!!plan} onClose={() => setPlan(null)} title={plan ? `${plan.plan.action[0].toUpperCase()}${plan.plan.action.slice(1)} ${s.label}` : ""} wide footer={plan && <>
        <Button variant="ghost" onClick={() => setPlan(null)}>Cancel</Button>
        <Button variant={plan.plan.action === "uninstall" ? "danger" : "primary"} disabled={plan.plan.changes.length === 0 && plan.plan.commands.length === 0} loading={apply.isPending} onClick={() => apply.mutate()}>
          {plan.plan.changes.length === 0 && plan.plan.commands.length === 0 ? "Nothing to change" : "Apply changes"}
        </Button>
      </>}>
        {plan && (
          <div className="flex flex-col gap-3 text-[12.5px]">
            <p className="text-muted">Review every change before it is applied. Each file is backed up first, written atomically, and rolled back if any step fails. Settings that are not ContextForge's are preserved.</p>
            {plan.plan.warnings.map((w) => <div key={w} className="flex gap-2 rounded-md border border-warn/40 bg-warn-soft p-2 text-warn"><CircleAlert className="size-4 shrink-0" />{tildeText(w)}</div>)}
            {plan.preview ? <DiffView text={tildeText(plan.preview)} /> : <p className="text-muted">No file changes.</p>}
            {plan.plan.notes.map((n) => <p key={n} className="rounded-md border border-line bg-surface-2 p-2 text-[12px]">{n}</p>)}
            {apply.error && <ErrorNote error={errorMessage(apply.error)} />}
          </div>
        )}
      </Modal>
    </Card>
  );
}

export default function Integrations() {
  const qc = useQueryClient();
  const toast = useToast();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.listProjects });
  const [scope, setScope] = useState<string>("");
  const tilde = useTilde();
  const status = useQuery({ queryKey: ["integrations", scope], queryFn: () => api.integrationsStatus(scope || null), staleTime: 30_000 });
  const overview = useQuery({ queryKey: ["overview", null], queryFn: () => api.overview(null) });
  const backups = useQuery({ queryKey: ["integrations", "backups"], queryFn: api.configBackups });
  const [checks, setChecks] = useState<Awaited<ReturnType<typeof api.integrationValidate>> | null>(null);
  const [restoreId, setRestoreId] = useState<{ id: string; force: boolean } | null>(null);
  const validate = useMutation({ mutationFn: api.integrationValidate, onSuccess: setChecks, onError: (e) => toast("error", errorMessage(e)) });
  const restore = useMutation({
    mutationFn: () => api.restoreConfigBackup(restoreId!.id, restoreId!.force),
    onSuccess: (r) => { toast("ok", `Restored ${r.restored.length} file(s)${r.notes.length ? ` — ${r.notes.join(" ")}` : ""}`); setRestoreId(null); qc.invalidateQueries({ queryKey: ["integrations"] }); },
    onError: (e) => {
      const msg = errorMessage(e);
      if (msg.includes("changed after ContextForge") && restoreId && !restoreId.force) setRestoreId({ ...restoreId, force: true });
      toast("error", msg);
    },
  });
  const lastEvent = (p: Provider) => overview.data?.agent_activity.filter((a) => ACTIVITY_AGENTS[p].includes(a.agent)).map((a) => a.last_event_at).sort().pop() ?? null;
  const scopePath = scope ? projects.data?.find((p) => p.project.id === scope)?.project.root_path ?? null : null;

  return (
    <>
      <PageHeader title="Integrations" subtitle="Connect Claude Code, Codex and Antigravity to the shared memory" actions={<>
        <Select aria-label="Configuration scope" value={scope} onChange={(e) => setScope(e.target.value)} className="w-64"
          options={[{ value: "", label: "User-level (all projects)" }, ...(projects.data ?? []).map((p) => ({ value: p.project.id, label: `Project: ${p.project.name}` }))]} />
        <Button icon={<RefreshCw className="size-4" />} onClick={() => api.integrationsStatus(scopePath, true).then(() => qc.invalidateQueries({ queryKey: ["integrations"] }))}>Re-detect</Button>
        <Button icon={<Stethoscope className="size-4" />} loading={validate.isPending} onClick={() => validate.mutate()}>Validate</Button>
      </>} />
      {status.isLoading ? <Spinner label="Detecting agents" /> : status.error ? <ErrorNote error={errorMessage(status.error)} onRetry={() => status.refetch()} /> : status.data && (
        <>
          <p className="mb-3 text-[12px] text-muted">
            Agents run <span className="font-mono">{tilde(status.data.cliPath)}</span>{!status.data.cliExists && <Badge tone="danger">missing</Badge>} for hooks and MCP; the desktop app does not need to stay open.
            {scope ? " Project scope writes into the project's .claude/, .codex/ and .agents/ folders." : ""}
          </p>
          {checks && (
            <Card title="Validation" className="mb-4" actions={<Button size="sm" variant="ghost" onClick={() => setChecks(null)}>Dismiss</Button>}>
              <ul className="flex flex-col gap-1.5 text-[12.5px]">{checks.map((c) => (
                <li key={c.name} className="flex items-start gap-2">{c.ok ? <CheckCircle2 className="mt-0.5 size-4 text-ok" /> : <XCircle className="mt-0.5 size-4 text-danger" />}<b className="w-56 shrink-0">{c.name}</b><span className="break-words text-muted">{c.detail}</span></li>
              ))}</ul>
            </Card>
          )}
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            {status.data.statuses.map((s) => <ProviderCard key={s.provider + scope} s={s} scopePath={scopePath} lastEvent={lastEvent(s.provider)} />)}
          </div>
          <Card title={<span className="flex items-center gap-2"><History className="size-4" /> Configuration backups</span>} className="mt-4" padded={false}>
            {!backups.data?.length ? <p className="p-4 text-[12.5px] text-muted">ContextForge backs up every configuration file before changing it. Backups will appear here.</p> : (
              <ul className="divide-y divide-line">{backups.data.map((b) => (
                <li key={b.id} className="flex items-center gap-3 px-4 py-2 text-[12.5px]">
                  <span className="w-44 text-muted">{dateTime(b.created_at)}</span>
                  <Badge>{b.action}</Badge>
                  <span className="flex-1 truncate">{b.provider} · {b.files.map((f) => f.original.split(/[\\/]/).slice(-2).join("/")).join(", ")}</span>
                  <Button size="sm" variant="ghost" onClick={() => setRestoreId({ id: b.id, force: false })}>Restore</Button>
                </li>
              ))}</ul>
            )}
          </Card>
        </>
      )}
      <ConfirmModal open={!!restoreId} onClose={() => setRestoreId(null)} onConfirm={() => restore.mutate()} busy={restore.isPending}
        title={restoreId?.force ? "Files changed since — restore anyway?" : "Restore configuration backup?"}
        confirmLabel={restoreId?.force ? "Discard later changes and restore" : "Restore"}
        body={restoreId?.force ? "Some files were edited after ContextForge changed them. Restoring discards those later edits." : "The configuration files will be returned to their exact content before this change. External registrations (e.g. claude mcp add) are not reverted by a file restore."} />
    </>
  );
}
