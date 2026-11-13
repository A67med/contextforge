import { useEffect, useRef, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { CornerDownLeft, Search } from "lucide-react";
import { api } from "../lib/api";
import { KIND_LABEL } from "../lib/format";
import { useApp, type Screen } from "../App";
import { cx, FreshnessBadge, Kbd } from "./ui";

type Item = { id: string; label: string; hint: ReactNode; run: () => void };

export function CommandPalette({ open, onClose, nav }: { open: boolean; onClose: () => void; nav: { id: Screen; label: string; icon: ReactNode }[] }) {
  const app = useApp();
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (open) { setQ(""); setActive(0); setTimeout(() => inputRef.current?.focus(), 0); }
  }, [open]);
  const search = useQuery({
    queryKey: ["palette", q, app.projectId],
    queryFn: () => api.searchMemories({ text: q, project_id: app.projectId, limit: 8, mode: "all" }),
    enabled: open && q.trim().length >= 2,
  });
  const screens: Item[] = nav
    .filter((n) => !q || n.label.toLowerCase().includes(q.toLowerCase()))
    .map((n) => ({ id: `nav-${n.id}`, label: `Go to ${n.label}`, hint: n.icon, run: () => app.go(n.id) }));
  const memories: Item[] = (search.data?.hits ?? []).map((h) => ({
    id: h.memory.id,
    label: h.memory.title,
    hint: <span className="flex items-center gap-1.5 text-faint">{KIND_LABEL[h.memory.kind]} <FreshnessBadge value={h.memory.freshness} /></span>,
    run: () => app.openMemory(h.memory.id),
  }));
  const items = [...memories, ...screens];
  if (!open) return null;
  const choose = (i: number) => { items[i]?.run(); onClose(); };
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[12vh]" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Command palette" className="anim-pop w-[620px] overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow)]">
        <div className="flex items-center gap-2 border-b border-line px-3">
          <Search className="size-4 text-muted" />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => { setQ(e.target.value); setActive(0); }}
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
              if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(a + 1, items.length - 1)); }
              if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
              if (e.key === "Enter") { e.preventDefault(); choose(active); }
            }}
            placeholder="Search memories or jump to a screen…"
            aria-label="Search"
            className="h-12 flex-1 bg-transparent text-[14px] outline-none placeholder:text-faint"
          />
          <Kbd>Esc</Kbd>
        </div>
        <ul className="max-h-[52vh] overflow-y-auto p-1.5" role="listbox">
          {items.length === 0 && <li className="px-3 py-6 text-center text-[12.5px] text-muted">{search.isFetching ? "Searching…" : "No matches"}</li>}
          {items.map((it, i) => (
            <li key={it.id} role="option" aria-selected={i === active}>
              <button
                onMouseEnter={() => setActive(i)}
                onClick={() => choose(i)}
                className={cx("flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-[13px]", i === active ? "bg-surface-3" : "hover:bg-surface-2")}
              >
                <span className="flex-1 truncate">{it.label}</span>
                <span className="text-[11.5px]">{it.hint}</span>
                {i === active && <CornerDownLeft className="size-3.5 text-faint" />}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
