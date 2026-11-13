import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { Database, Download, FolderOpen, HardDriveDownload, RotateCcw, Upload } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { useTilde } from "../lib/paths";
import { bytes, dateTime } from "../lib/format";
import type { Settings as S } from "../lib/types";
import { Badge, Button, Card, ConfirmModal, ErrorNote, Field, Input, PageHeader, Select, Spinner, TextArea, Toggle, useToast } from "../components/ui";

export default function Settings() {
  const qc = useQueryClient();
  const toast = useToast();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
  const diag = useQuery({ queryKey: ["diagnostics"], queryFn: api.diagnostics });
  const dbBackups = useQuery({ queryKey: ["db-backups"], queryFn: api.listDatabaseBackups });
  const [draft, setDraft] = useState<S | null>(null);
  const [globs, setGlobs] = useState("");
  const [restorePath, setRestorePath] = useState<string | null>(null);
  const [log, setLog] = useState<string | null>(null);
  const tilde = useTilde();
  useEffect(() => {
    if (settings.data && !draft) { setDraft(settings.data); setGlobs(settings.data.privacy.extra_sensitive_globs.join("\n")); }
  }, [settings.data, draft]);
  const save = useMutation({
    mutationFn: (s: S) => api.saveSettings(s),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["settings"] }); toast("ok", "Settings saved"); },
    onError: (e) => toast("error", errorMessage(e)),
  });
  const run = (p: Promise<unknown>, ok: (v: never) => string) => p.then((v) => { toast("ok", ok(v as never)); qc.invalidateQueries(); }).catch((e) => toast("error", errorMessage(e)));

  if (settings.isLoading || !draft) return <Spinner />;
  if (settings.error) return <ErrorNote error={errorMessage(settings.error)} />;
  const set = (fn: (s: S) => void) => { const next = structuredClone(draft); fn(next); setDraft(next); };
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings.data) || globs !== settings.data!.privacy.extra_sensitive_globs.join("\n");
  const commit = () => save.mutate({ ...draft, privacy: { extra_sensitive_globs: globs.split("\n").map((g) => g.trim()).filter(Boolean) } });
  const d = diag.data;

  return (
    <>
      <PageHeader title="Settings" subtitle="Stored locally in the ContextForge database; shared by the app, CLI, hooks and MCP server" actions={
        <Button variant="primary" disabled={!dirty} loading={save.isPending} onClick={commit}>Save changes</Button>
      } />
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="Context injection">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Startup brief budget (tokens)" hint="200–8000. Default 1200.">{(id) => <Input id={id} type="number" min={200} max={8000} value={draft.context.startup_budget_tokens} onChange={(e) => set((s) => { s.context.startup_budget_tokens = Number(e.target.value); })} />}</Field>
            <Field label="Per-prompt budget (tokens)" hint="50–4000. Default 300.">{(id) => <Input id={id} type="number" min={50} max={4000} value={draft.context.prompt_budget_tokens} onChange={(e) => set((s) => { s.context.prompt_budget_tokens = Number(e.target.value); })} />}</Field>
            <Field label="Handoff budget (tokens)" hint="300–12000.">{(id) => <Input id={id} type="number" min={300} max={12000} value={draft.context.handoff_budget_tokens} onChange={(e) => set((s) => { s.context.handoff_budget_tokens = Number(e.target.value); })} />}</Field>
          </div>
          <Toggle label="Inject prompt-relevant memories" description="On each new prompt, add up to the per-prompt budget of matching memories the session has not seen yet." checked={draft.context.prompt_injection} onChange={(v) => set((s) => { s.context.prompt_injection = v; })} />
          <Toggle label="Check freshness at session start" description="Re-evaluate memories against the working tree whenever an agent session starts." checked={draft.freshness.check_on_session_start} onChange={(v) => set((s) => { s.freshness.check_on_session_start = v; })} />
        </Card>

        <Card title="Privacy & capture">
          <Toggle label="Record shell commands" description="Commands run by agents are logged (redacted, truncated) in the session timeline." checked={draft.capture.commands} onChange={(v) => set((s) => { s.capture.commands = v; })} />
          <Toggle label="Remember successful build/test commands" description="Turns commands that agents report as successful into verified command memories." checked={draft.capture.auto_command_memories} onChange={(v) => set((s) => { s.capture.auto_command_memories = v; })} />
          <Toggle label="Store agent turn summaries" description="Keeps a redacted excerpt of the agent's last message per turn." checked={draft.capture.assistant_summaries} onChange={(v) => set((s) => { s.capture.assistant_summaries = v; })} />
          <Toggle label="Store prompt text" description="Off by default: prompts are used only transiently to find relevant memories." checked={draft.capture.prompts} onChange={(v) => set((s) => { s.capture.prompts = v; })} />
          <Toggle label="Auto-register Git repositories" description="Off by default: projects must be opted in explicitly." checked={draft.capture.auto_register_projects} onChange={(v) => set((s) => { s.capture.auto_register_projects = v; })} />
          <Field label="Additional sensitive paths (globs, one per line)" hint="Built-in: .env*, keys/certificates, SSH/AWS/GPG folders, credentials, secrets, Terraform state, service-account files. Contents of matching files are never stored; secrets are also redacted from all text.">
            {(id) => <TextArea id={id} rows={3} value={globs} onChange={(e) => setGlobs(e.target.value)} className="font-mono text-[12px]" placeholder="**/internal/**" />}
          </Field>
        </Card>

        <Card title="Retention, logging & appearance">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Keep activity events (days)" hint="Memories are never pruned automatically.">{(id) => <Input id={id} type="number" min={1} max={3650} value={draft.retention.events_days} onChange={(e) => set((s) => { s.retention.events_days = Number(e.target.value); })} />}</Field>
            <Field label="Handoffs kept per project">{(id) => <Input id={id} type="number" min={1} max={1000} value={draft.retention.handoffs_per_project} onChange={(e) => set((s) => { s.retention.handoffs_per_project = Number(e.target.value); })} />}</Field>
            <Field label="Log level">{(id) => <Select id={id} value={draft.logging.level} onChange={(e) => set((s) => { s.logging.level = e.target.value; })} options={["error", "warn", "info", "debug", "trace"].map((l) => ({ value: l, label: l }))} />}</Field>
            <Field label="Theme">{(id) => <Select id={id} value={draft.ui.theme} onChange={(e) => set((s) => { s.ui.theme = e.target.value as S["ui"]["theme"]; })} options={[{ value: "system", label: "Follow system" }, { value: "dark", label: "Dark" }, { value: "light", label: "Light" }]} />}</Field>
          </div>
          <div className="mt-3 flex gap-2">
            <Button size="sm" onClick={() => run(api.pruneEvents(), (n: number) => `Pruned ${n} old events`)}>Prune old events now</Button>
            <Button size="sm" onClick={() => api.readRecentLog(300).then((t) => setLog(t || "(log is empty)")).catch((e) => toast("error", errorMessage(e)))}>View recent log</Button>
          </div>
          {log !== null && <pre className="prose-brief mt-3 max-h-64 overflow-auto rounded-md border border-line bg-surface-2 p-2 text-[11px]">{log}</pre>}
        </Card>

        <Card title={<span className="flex items-center gap-2"><Database className="size-4" /> Data</span>}>
          {d && (
            <dl className="mb-3 grid grid-cols-[120px_1fr] gap-x-3 gap-y-1 text-[12px]">
              <dt className="text-muted">Database</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.databasePath)}</dd>
              <dt className="text-muted">Schema</dt><dd>v{d.schemaVersion} · integrity {d.integrity === "ok" ? <Badge tone="ok">ok</Badge> : <Badge tone="danger">{d.integrity}</Badge>}</dd>
              <dt className="text-muted">Data folder</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.dataDir)}</dd>
            </dl>
          )}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" icon={<FolderOpen className="size-3.5" />} onClick={() => d && openPath(d.dataDir).catch((e) => toast("error", errorMessage(e)))}>Open data folder</Button>
            <Button size="sm" onClick={async () => {
              const folder = await openDialog({ directory: true, title: "Move the database to…" });
              if (typeof folder === "string") run(api.relocateDatabase(folder), (p: string) => `Database moved to ${p}`);
            }}>Move database…</Button>
          </div>
          <div className="mt-4 grid grid-cols-2 gap-2">
            <Button icon={<HardDriveDownload className="size-4" />} onClick={() => run(api.backupDatabase(null), (p: string) => `Backup written to ${p}`)}>Back up now</Button>
            <Button icon={<RotateCcw className="size-4" />} onClick={async () => {
              const f = await openDialog({ title: "Restore from backup", filters: [{ name: "ContextForge database", extensions: ["db"] }], defaultPath: d?.backupsDir });
              if (typeof f === "string") setRestorePath(f);
            }}>Restore backup…</Button>
            <Button icon={<Download className="size-4" />} onClick={async () => {
              const f = await saveDialog({ title: "Export memories", defaultPath: "contextforge-export.json", filters: [{ name: "JSON", extensions: ["json"] }] });
              if (f) run(api.exportData(f), (n: number) => `Exported ${n} memories`);
            }}>Export JSON…</Button>
            <Button icon={<Upload className="size-4" />} onClick={async () => {
              const f = await openDialog({ title: "Import memories", filters: [{ name: "JSON", extensions: ["json"] }] });
              if (typeof f === "string") run(api.importData(f), (r: { memories_imported: number; memories_skipped: number }) => `Imported ${r.memories_imported} memories (${r.memories_skipped} already present)`);
            }}>Import JSON…</Button>
          </div>
          {!!dbBackups.data?.length && (
            <div className="mt-4">
              <div className="mb-1 text-[12px] font-semibold text-muted">Recent database backups</div>
              <ul className="flex flex-col gap-1 text-[11.5px]">{dbBackups.data.slice(0, 5).map((b) => (
                <li key={b.path} className="flex items-center gap-2"><span className="flex-1 truncate font-mono" title={b.path}>{b.path.split(/[\\/]/).pop()}</span><span className="text-faint">{bytes(b.size_bytes)} · {dateTime(b.modified)}</span><Button size="sm" variant="ghost" onClick={() => setRestorePath(b.path)}>Restore</Button></li>
              ))}</ul>
            </div>
          )}
        </Card>

        <Card title="Diagnostics" className="xl:col-span-2">
          {d ? (
            <dl className="grid grid-cols-[160px_1fr] gap-x-3 gap-y-1 text-[12px]">
              <dt className="text-muted">Version</dt><dd>ContextForge {d.version}</dd>
              <dt className="text-muted">CLI / hook binary</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.cliPath)} {d.cliExists ? <Badge tone="ok">found</Badge> : <Badge tone="danger">missing</Badge>}</dd>
              <dt className="text-muted">Git</dt><dd>{d.gitAvailable ? <Badge tone="ok">available</Badge> : <Badge tone="warn">not found — branch/commit awareness disabled</Badge>}</dd>
              <dt className="text-muted">Logs</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.logsDir)}</dd>
              <dt className="text-muted">DB backups</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.backupsDir)}</dd>
              <dt className="text-muted">Config backups</dt><dd className="break-all font-mono text-[11.5px]">{tilde(d.configBackupsDir)}</dd>
              <dt className="text-muted">Network</dt><dd>No telemetry, no network requests. MCP runs over stdio only.</dd>
            </dl>
          ) : <Spinner />}
        </Card>
      </div>
      <ConfirmModal open={!!restorePath} onClose={() => setRestorePath(null)} title="Restore the database?" confirmLabel="Restore"
        onConfirm={() => { const p = restorePath!; setRestorePath(null); run(api.restoreDatabase(p), (safety: string) => `Restored. Previous state saved to ${safety}`); }}
        body={<>All current data will be replaced with <span className="font-mono">{restorePath}</span>. A safety backup of the current state is made first.</>} />
    </>
  );
}
