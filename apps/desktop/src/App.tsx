import { createContext, lazy, Suspense, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Activity, Boxes, Brain, ChevronDown, Command, FolderGit2, LayoutDashboard, Plug, Search, Send, Settings as SettingsIcon } from "lucide-react";
import { api } from "./lib/api";
import { cx, Kbd, Spinner } from "./components/ui";
import { CommandPalette } from "./components/CommandPalette";

export type Screen = "overview" | "projects" | "memories" | "timeline" | "handoffs" | "integrations" | "settings";

interface AppCtx {
  screen: Screen;
  go: (s: Screen) => void;
  projectId: string | null;
  setProjectId: (id: string | null) => void;
  /** Memory to open in the Memory Explorer detail pane. */
  focusMemory: string | null;
  openMemory: (id: string) => void;
  clearFocus: () => void;
}
const Ctx = createContext<AppCtx | null>(null);
export function useApp() {
  const c = useContext(Ctx);
  if (!c) throw new Error("useApp outside provider");
  return c;
}

const Overview = lazy(() => import("./screens/Overview"));
const Projects = lazy(() => import("./screens/Projects"));
const Memories = lazy(() => import("./screens/Memories"));
const Timeline = lazy(() => import("./screens/Timeline"));
const Handoffs = lazy(() => import("./screens/Handoffs"));
const Integrations = lazy(() => import("./screens/Integrations"));
const SettingsScreen = lazy(() => import("./screens/Settings"));

const NAV: { id: Screen; label: string; icon: ReactNode; key: string }[] = [
  { id: "overview", label: "Overview", icon: <LayoutDashboard className="size-4" />, key: "1" },
  { id: "projects", label: "Projects", icon: <FolderGit2 className="size-4" />, key: "2" },
  { id: "memories", label: "Memory", icon: <Brain className="size-4" />, key: "3" },
  { id: "timeline", label: "Timeline", icon: <Activity className="size-4" />, key: "4" },
  { id: "handoffs", label: "Handoffs", icon: <Send className="size-4" />, key: "5" },
  { id: "integrations", label: "Integrations", icon: <Plug className="size-4" />, key: "6" },
  { id: "settings", label: "Settings", icon: <SettingsIcon className="size-4" />, key: "7" },
];

function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writeStored(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: non-essential */
  }
}

/** Refresh all views when any process (agent hooks, MCP servers, CLI) commits to the database. */
function useLiveRefresh() {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["data_version"], queryFn: api.dataVersion, refetchInterval: 2000, refetchIntervalInBackground: false });
  const [last, setLast] = useState<number | null>(null);
  useEffect(() => {
    if (data === undefined) return;
    if (last !== null && data !== last) {
      qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "data_version" && q.queryKey[0] !== "integrations" });
    }
    setLast(data);
  }, [data, last, qc]);
}

function useTheme() {
  const { data } = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
  const theme = data?.ui.theme ?? "system";
  useEffect(() => {
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

function ProjectSwitcher() {
  const { projectId, setProjectId } = useApp();
  const { data } = useQuery({ queryKey: ["projects"], queryFn: api.listProjects });
  useEffect(() => {
    if (projectId && data && !data.some((p) => p.project.id === projectId)) setProjectId(null);
  }, [data, projectId, setProjectId]);
  return (
    <label className="relative flex items-center">
      <span className="sr-only">Project scope</span>
      <select
        value={projectId ?? ""}
        onChange={(e) => setProjectId(e.target.value || null)}
        className="h-8 appearance-none rounded-md border border-line bg-surface-2 pl-3 pr-8 text-[13px] text-fg outline-none hover:border-line-strong focus:border-accent"
      >
        <option value="">All projects</option>
        {data?.map((p) => (
          <option key={p.project.id} value={p.project.id}>{p.project.name}</option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted" />
    </label>
  );
}

export default function App() {
  const [screen, setScreen] = useState<Screen>(() => (readStored("cf.screen") as Screen) || "overview");
  const [projectId, setProjectIdState] = useState<string | null>(() => readStored("cf.project"));
  const [focusMemory, setFocusMemory] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  useLiveRefresh();
  useTheme();

  const ctx = useMemo<AppCtx>(() => ({
    screen,
    go: (s) => { setScreen(s); writeStored("cf.screen", s); },
    projectId,
    setProjectId: (id) => { setProjectIdState(id); writeStored("cf.project", id); },
    focusMemory,
    openMemory: (id) => { setFocusMemory(id); setScreen("memories"); writeStored("cf.screen", "memories"); },
    clearFocus: () => setFocusMemory(null),
  }), [screen, projectId, focusMemory]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "k") { e.preventDefault(); setPaletteOpen((v) => !v); return; }
      if (mod && /^[1-7]$/.test(e.key)) {
        e.preventDefault();
        const target = NAV.find((n) => n.key === e.key);
        if (target) ctx.go(target.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ctx]);

  return (
    <Ctx.Provider value={ctx}>
      <div className="flex h-full">
        <aside className="flex w-[212px] shrink-0 flex-col border-r border-line bg-surface">
          <div className="flex items-center gap-2.5 px-4 pb-4 pt-4">
            <img src="/logo.svg" alt="" className="size-7 rounded-md" />
            <div>
              <div className="text-[13.5px] font-semibold tracking-tight">ContextForge</div>
              <div className="text-[10.5px] text-faint">One memory. Every agent.</div>
            </div>
          </div>
          <nav className="flex flex-1 flex-col gap-0.5 px-2" aria-label="Main">
            {NAV.map((n) => (
              <button
                key={n.id}
                onClick={() => ctx.go(n.id)}
                aria-current={screen === n.id ? "page" : undefined}
                className={cx(
                  "group flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13px] transition-colors",
                  screen === n.id ? "bg-accent-soft text-fg font-medium" : "text-muted hover:bg-surface-2 hover:text-fg",
                )}
              >
                <span className={screen === n.id ? "text-accent" : ""}>{n.icon}</span>
                <span className="flex-1 text-left">{n.label}</span>
                <span className="font-mono text-[10px] text-faint opacity-0 group-hover:opacity-100">^{n.key}</span>
              </button>
            ))}
          </nav>
          <div className="border-t border-line p-3 text-[11px] text-faint">
            <div className="flex items-center gap-1.5"><Boxes className="size-3" /> Local only · no telemetry</div>
          </div>
        </aside>
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-bg/80 px-5 backdrop-blur">
            <ProjectSwitcher />
            <button
              onClick={() => setPaletteOpen(true)}
              className="ml-auto flex h-8 w-[300px] items-center gap-2 rounded-md border border-line bg-surface-2 px-2.5 text-[12.5px] text-faint hover:border-line-strong"
            >
              <Search className="size-3.5" /> Search memories, jump to…
              <span className="ml-auto flex items-center gap-1"><Kbd><Command className="inline size-2.5" /> K</Kbd></span>
            </button>
          </header>
          <main className="min-h-0 flex-1 overflow-y-auto">
            <div key={screen} className="anim-in mx-auto max-w-[1280px] px-6 py-6">
              <Suspense fallback={<Spinner />}>
                {screen === "overview" && <Overview />}
                {screen === "projects" && <Projects />}
                {screen === "memories" && <Memories />}
                {screen === "timeline" && <Timeline />}
                {screen === "handoffs" && <Handoffs />}
                {screen === "integrations" && <Integrations />}
                {screen === "settings" && <SettingsScreen />}
              </Suspense>
            </div>
          </main>
        </div>
      </div>
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} nav={NAV} />
    </Ctx.Provider>
  );
}
