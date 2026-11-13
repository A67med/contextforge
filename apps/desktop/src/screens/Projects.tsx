import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { Eye, FolderGit2, FolderOpen, FolderPlus, GitBranch, RefreshCw, Trash2 } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { useTilde } from "../lib/paths";
import { relativeTime } from "../lib/format";
import type { ProjectSettings, ProjectSummary } from "../lib/types";
import { useApp } from "../App";
import { AgentChip, Badge, Button, Card, ConfirmModal, EmptyState, ErrorNote, Field, Input, Modal, PageHeader, Spinner, TextArea, Toggle, useToast } from "../components/ui";

function AddProjectModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const qc = useQueryClient();
  const toast = useToast();
  const app = useApp();
  const add = useMutation({
    mutationFn: () => api.addProject(path.trim(), name.trim() || undefined),
    onSuccess: (p) => {
      qc.invalidateQueries();
      toast("ok", `Registered ${p.name}${p.is_git ? " (Git repository)" : ""}`);
      app.setProjectId(p.id);
      setPath(""); setName(""); onClose();
    },
  });
  const browse = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: "Choose a project folder" });
    if (typeof picked === "string") setPath(picked);
  };
  return (
    <Modal open={open} onClose={onClose} title="Add project" footer={<>
      <Button variant="ghost" onClick={onClose}>Cancel</Button>
      <Button variant="primary" disabled={!path.trim()} loading={add.isPending} onClick={() => add.mutate()}>Register project</Button>
    </>}>
      <div className="flex flex-col gap-4">
        <Field label="Folder" hint="Inside a Git repository the repository root is used; linked worktrees join the same project.">
          {(id) => (
            <div className="flex gap-2">
              <Input id={id} value={path} onChange={(e) => setPath(e.target.value)} placeholder="C:\\Users\\you\\code\\app" />
              <Button onClick={browse} icon={<FolderOpen className="size-4" />}>Browse…</Button>
            </div>
          )}
        </Field>
        <Field label="Display name (optional)">{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="Defaults to the folder name" />}</Field>
        <p className="text-[12px] text-muted">Registering opts this project in: agent hooks record activity only for registered projects. Files are never modified.</p>
        {add.error && <ErrorNote error={errorMessage(add.error)} />}
      </div>
    </Modal>
  );
}

function SettingsEditor({ summary }: { summary: ProjectSummary }) {
  const p = summary.project;
  const [name, setName] = useState(p.name);
  const [settings, setSettings] = useState<ProjectSettings>(p.settings);
  const [globs, setGlobs] = useState(p.settings.excluded_globs.join("\n"));
  const qc = useQueryClient();
  const toast = useToast();
  const save = useMutation({
    mutationFn: () => api.updateProject(p.id, name, { ...settings, excluded_globs: globs.split("\n").map((g) => g.trim()).filter(Boolean) }),
    onSuccess: () => { qc.invalidateQueries(); toast("ok", "Project settings saved"); },
    onError: (e) => toast("error", errorMessage(e)),
  });
  return (
    <div className="flex flex-col gap-3">
      <Field label="Name">{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} />}</Field>
      <Toggle label="Capture agent activity" description="When off, ContextForge ignores hooks for this project (memories remain accessible)." checked={settings.capture_enabled} onChange={(v) => setSettings({ ...settings, capture_enabled: v })} />
      <Field label="Excluded paths (globs, one per line)" hint="Treated as sensitive in addition to the built-in patterns (.env, keys, credentials…): paths are recorded, contents never.">
        {(id) => <TextArea id={id} rows={3} value={globs} onChange={(e) => setGlobs(e.target.value)} placeholder={"private/**\n*.sqlite"} className="font-mono text-[12px]" />}
      </Field>
      <Field label="Startup context budget (tokens)" hint="Leave empty to use the global setting.">
        {(id) => <Input id={id} type="number" min={200} max={8000} value={settings.startup_budget_tokens ?? ""} onChange={(e) => setSettings({ ...settings, startup_budget_tokens: e.target.value ? Number(e.target.value) : null })} />}
      </Field>
      <div><Button variant="primary" loading={save.isPending} onClick={() => save.mutate()}>Save settings</Button></div>
    </div>
  );
}

function ProjectDetail({ summary }: { summary: ProjectSummary }) {
  const p = summary.project;
  const app = useApp();
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, setConfirm] = useState(false);
  const [preview, setPreview] = useState(false);
  const tilde = useTilde();
  const fresh = useMutation({
    mutationFn: () => api.checkFreshness(p.id),
    onSuccess: (r) => { qc.invalidateQueries(); toast("ok", `Checked ${r.checked} memories in ${r.duration_ms} ms: ${r.changes.length} changed, ${r.flagged_total} flagged`); },
    onError: (e) => toast("error", errorMessage(e)),
  });
  const remove = useMutation({
    mutationFn: () => api.removeProject(p.id),
    onSuccess: () => { qc.invalidateQueries(); toast("ok", `${p.name} removed from ContextForge (files untouched)`); setConfirm(false); if (app.projectId === p.id) app.setProjectId(null); },
    onError: (e) => toast("error", errorMessage(e)),
  });
  const brief = useQuery({ queryKey: ["brief", p.id], queryFn: () => api.previewContext(p.id), enabled: preview });
  return (
    <div className="flex flex-col gap-4">
      <Card title={p.name} actions={<>
        <Button size="sm" icon={<RefreshCw className="size-3.5" />} loading={fresh.isPending} onClick={() => fresh.mutate()}>Check freshness</Button>
        <Button size="sm" icon={<Eye className="size-3.5" />} onClick={() => setPreview(true)}>Preview agent context</Button>
        <Button size="sm" icon={<FolderOpen className="size-3.5" />} onClick={() => openPath(p.root_path).catch((e) => toast("error", errorMessage(e)))}>Open folder</Button>
      </>}>
        <dl className="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-[12.5px]">
          <dt className="text-muted">Root</dt><dd className="break-all font-mono text-[12px]">{tilde(p.root_path)}</dd>
          <dt className="text-muted">Repository</dt><dd>{p.is_git ? <span className="flex flex-wrap items-center gap-2"><Badge tone="info"><GitBranch className="size-3" /> Git</Badge>{p.default_branch && <span>default branch <b>{p.default_branch}</b></span>}{p.remote_url && <span className="font-mono text-[11.5px] text-muted">{p.remote_url}</span>}</span> : "Not a Git repository (content hashes still detect changes)"}</dd>
          <dt className="text-muted">Checkouts</dt>
          <dd className="flex flex-col gap-1">{p.roots.map((r) => <span key={r.path} className="font-mono text-[12px]">{tilde(r.path)} <Badge>{r.kind}</Badge>{r.branch && <Badge tone="info">{r.branch}</Badge>}</span>)}</dd>
          <dt className="text-muted">Memories</dt><dd>{summary.memory_count} · {summary.open_tasks} open tasks · {summary.open_bugs} bugs · {summary.needs_review} need review</dd>
          <dt className="text-muted">Linked agents</dt><dd className="flex flex-wrap gap-2">{summary.agents.length ? summary.agents.map((a) => <AgentChip key={a} agent={a} />) : <span className="text-muted">No sessions yet</span>}</dd>
          <dt className="text-muted">Last activity</dt><dd>{relativeTime(p.last_activity_at)}</dd>
        </dl>
        <div className="mt-4 flex gap-2">
          <Button size="sm" onClick={() => { app.setProjectId(p.id); app.go("memories"); }}>Browse memories</Button>
          <Button size="sm" onClick={() => { app.setProjectId(p.id); app.go("timeline"); }}>View timeline</Button>
        </div>
      </Card>
      <Card title="Project settings"><SettingsEditor key={p.id + p.updated_at} summary={summary} /></Card>
      <Card title="Danger zone">
        <div className="flex items-center justify-between gap-4">
          <p className="text-[12.5px] text-muted">Remove this project and all of its ContextForge memories, sessions and handoffs. Files in the project folder are not touched.</p>
          <Button variant="danger" icon={<Trash2 className="size-4" />} onClick={() => setConfirm(true)}>Remove project</Button>
        </div>
      </Card>
      <ConfirmModal open={confirm} onClose={() => setConfirm(false)} busy={remove.isPending} onConfirm={() => remove.mutate()} title={`Remove ${p.name}?`} confirmLabel="Remove from ContextForge"
        body={<>This permanently deletes {summary.memory_count} memories and {summary.session_count} sessions recorded for <b>{p.name}</b>. Consider exporting first (Settings → Data). Project files are not affected.</>} />
      <Modal open={preview} onClose={() => setPreview(false)} title="Context an agent receives at session start" wide>
        {brief.isLoading ? <Spinner /> : brief.error ? <ErrorNote error={errorMessage(brief.error)} /> : brief.data && (
          <>
            <p className="mb-2 text-[12px] text-muted">~{brief.data.token_estimate} tokens · {brief.data.memory_ids.length} memories{brief.data.omitted ? ` · ${brief.data.omitted} omitted by the budget` : ""}</p>
            <pre className="prose-brief rounded-md border border-line bg-surface-2 p-3">{brief.data.text}</pre>
          </>
        )}
      </Modal>
    </div>
  );
}

export default function Projects() {
  const app = useApp();
  const [adding, setAdding] = useState(false);
  const { data, isLoading, error, refetch } = useQuery({ queryKey: ["projects"], queryFn: api.listProjects });
  const tilde = useTilde();
  const selected = data?.find((p) => p.project.id === app.projectId) ?? data?.[0];
  return (
    <>
      <PageHeader title="Projects" subtitle="Folders opted in to shared memory" actions={<Button variant="primary" icon={<FolderPlus className="size-4" />} onClick={() => setAdding(true)}>Add project</Button>} />
      {isLoading ? <Spinner /> : error ? <ErrorNote error={errorMessage(error)} onRetry={() => refetch()} /> : !data?.length ? (
        <Card><EmptyState icon={<FolderGit2 className="size-6" />} title="No projects yet" action={<Button variant="primary" onClick={() => setAdding(true)}>Add project</Button>}>Register a local folder. Git repositories, branches and worktrees are detected automatically.</EmptyState></Card>
      ) : (
        <div className="grid grid-cols-[300px_1fr] gap-4">
          <ul className="flex flex-col gap-1.5" aria-label="Projects">
            {data.map((s) => (
              <li key={s.project.id}>
                <button onClick={() => app.setProjectId(s.project.id)} aria-current={selected?.project.id === s.project.id}
                  className={`w-full rounded-lg border px-3 py-2.5 text-left transition-colors ${selected?.project.id === s.project.id ? "border-accent/60 bg-accent-soft" : "border-line bg-surface hover:border-line-strong"}`}>
                  <div className="flex items-center gap-2">
                    <FolderGit2 className="size-4 text-muted" />
                    <span className="flex-1 truncate text-[13px] font-medium">{s.project.name}</span>
                    {s.needs_review > 0 && <Badge tone="warn">{s.needs_review}</Badge>}
                  </div>
                  <div className="mt-1 truncate font-mono text-[11px] text-faint">{tilde(s.project.root_path)}</div>
                  <div className="mt-1 text-[11.5px] text-muted">{s.memory_count} memories · {relativeTime(s.project.last_activity_at)}</div>
                </button>
              </li>
            ))}
          </ul>
          {selected && <ProjectDetail key={selected.project.id} summary={selected} />}
        </div>
      )}
      <AddProjectModal open={adding} onClose={() => setAdding(false)} />
    </>
  );
}
