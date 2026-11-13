import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, Loader2, X } from "lucide-react";
import type { AgentKind, Certainty, ComponentState, Freshness } from "../lib/types";
import { AGENT_LABEL, AGENT_MONO, CERTAINTY_LABEL, FRESHNESS_LABEL } from "../lib/format";

export function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export function Button({
  variant = "secondary", size = "md", loading, icon, children, className, ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant; size?: "sm" | "md"; loading?: boolean; icon?: ReactNode }) {
  const styles: Record<ButtonVariant, string> = {
    primary: "bg-accent text-white hover:brightness-110 border-transparent shadow-sm",
    secondary: "bg-surface-2 text-fg border-line hover:bg-surface-3 hover:border-line-strong",
    ghost: "bg-transparent text-muted border-transparent hover:bg-surface-2 hover:text-fg",
    danger: "bg-danger-soft text-danger border-transparent hover:bg-danger hover:text-white",
  };
  return (
    <button
      {...rest}
      disabled={rest.disabled || loading}
      className={cx(
        "inline-flex items-center justify-center gap-1.5 rounded-md border font-medium transition-colors duration-150 select-none",
        "disabled:opacity-50 disabled:cursor-not-allowed whitespace-nowrap",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
        styles[variant],
        className,
      )}
    >
      {loading ? <Loader2 className="size-3.5 animate-spin" aria-hidden /> : icon}
      {children}
    </button>
  );
}

export function IconButton({ label, children, className, ...rest }: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      {...rest}
      aria-label={label}
      title={label}
      className={cx("inline-flex size-7 items-center justify-center rounded-md text-muted hover:bg-surface-2 hover:text-fg transition-colors disabled:opacity-40", className)}
    >
      {children}
    </button>
  );
}

type Tone = "neutral" | "accent" | "ok" | "warn" | "danger" | "info";
export function Badge({ tone = "neutral", children, className, title }: { tone?: Tone; children: ReactNode; className?: string; title?: string }) {
  const tones: Record<Tone, string> = {
    neutral: "bg-surface-3 text-muted",
    accent: "bg-accent-soft text-accent-fg",
    ok: "bg-ok-soft text-ok",
    warn: "bg-warn-soft text-warn",
    danger: "bg-danger-soft text-danger",
    info: "bg-info-soft text-info",
  };
  return (
    <span title={title} className={cx("inline-flex items-center gap-1 rounded px-1.5 py-px text-[11px] font-medium leading-4 whitespace-nowrap", tones[tone], className)}>
      {children}
    </span>
  );
}

export function FreshnessBadge({ value }: { value: Freshness }) {
  const tone: Record<Freshness, Tone> = { current: "ok", potentially_stale: "warn", needs_review: "danger", superseded: "neutral" };
  return <Badge tone={tone[value]}>{FRESHNESS_LABEL[value]}</Badge>;
}

export function CertaintyBadge({ value }: { value: Certainty }) {
  const tone: Record<Certainty, Tone> = { verified: "ok", decided: "info", unverified: "neutral", hypothesis: "warn" };
  return <Badge tone={tone[value]}>{CERTAINTY_LABEL[value]}</Badge>;
}

export function StateBadge({ state }: { state: ComponentState }) {
  const map: Record<ComponentState, [Tone, string]> = {
    installed: ["ok", "Installed"],
    not_installed: ["neutral", "Not installed"],
    partial: ["warn", "Partial"],
    outdated: ["warn", "Outdated"],
    pending_user_action: ["info", "Action needed"],
    error: ["danger", "Error"],
  };
  const [tone, label] = map[state];
  return <Badge tone={tone}>{label}</Badge>;
}

const AGENT_COLOR: Partial<Record<AgentKind, string>> = {
  claude_code: "#d97757",
  codex: "#8b9bb4",
  antigravity_cli: "#5b8def",
  antigravity_ide: "#5b8def",
  antigravity_app: "#5b8def",
  antigravity: "#5b8def",
  user: "#ff6a2b",
};
export function AgentChip({ agent, compact }: { agent: AgentKind; compact?: boolean }) {
  const color = AGENT_COLOR[agent] ?? "#7d8290";
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-[12px] text-fg" title={AGENT_LABEL[agent]}>
      <span className="inline-flex size-5 items-center justify-center rounded font-mono text-[9.5px] font-semibold text-white" style={{ background: color }} aria-hidden>
        {AGENT_MONO[agent]}
      </span>
      {!compact && AGENT_LABEL[agent]}
    </span>
  );
}

export function Card({ children, className, title, actions, padded = true }: { children: ReactNode; className?: string; title?: ReactNode; actions?: ReactNode; padded?: boolean }) {
  return (
    <section className={cx("rounded-lg border border-line bg-surface", className)}>
      {(title || actions) && (
        <header className="flex items-center justify-between gap-2 border-b border-line px-4 py-2.5">
          <h2 className="text-[12.5px] font-semibold tracking-wide text-fg">{title}</h2>
          <div className="flex items-center gap-1.5">{actions}</div>
        </header>
      )}
      <div className={padded ? "p-4" : ""}>{children}</div>
    </section>
  );
}

export function Field({ label, hint, children, error }: { label: string; hint?: ReactNode; children: (id: string) => ReactNode; error?: string | null }) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-[12px] font-medium text-muted">{label}</label>
      {children(id)}
      {hint && !error && <p className="text-[11.5px] text-faint">{hint}</p>}
      {error && <p className="text-[11.5px] text-danger" role="alert">{error}</p>}
    </div>
  );
}

const inputBase = "w-full rounded-md border border-line bg-surface-2 px-2.5 text-[13px] text-fg placeholder:text-faint outline-none transition-colors focus:border-accent focus:bg-surface";
export function Input(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cx(inputBase, "h-8", props.className)} />;
}
export function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} className={cx(inputBase, "py-2 leading-relaxed", props.className)} />;
}
export function Select({ options, ...props }: React.SelectHTMLAttributes<HTMLSelectElement> & { options: { value: string; label: string }[] }) {
  return (
    <select {...props} className={cx(inputBase, "h-8 pr-7", props.className)}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>{o.label}</option>
      ))}
    </select>
  );
}

export function Toggle({ checked, onChange, label, description }: { checked: boolean; onChange: (v: boolean) => void; label: string; description?: string }) {
  const id = useId();
  return (
    <div className="flex items-start justify-between gap-6 py-2.5">
      <div>
        <label htmlFor={id} className="text-[13px] font-medium text-fg">{label}</label>
        {description && <p className="mt-0.5 text-[12px] text-muted">{description}</p>}
      </div>
      <button
        id={id}
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={cx("relative mt-0.5 h-5 w-9 shrink-0 rounded-full p-0 transition-colors", checked ? "bg-accent" : "bg-surface-3 ring-1 ring-inset ring-line-strong")}
      >
        <span className={cx("absolute left-0 top-0.5 size-4 rounded-full bg-white shadow transition-transform", checked ? "translate-x-[18px]" : "translate-x-0.5")} />
      </button>
    </div>
  );
}

export function Spinner({ label = "Loading" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 p-6 text-muted" role="status">
      <Loader2 className="size-4 animate-spin" aria-hidden /> {label}…
    </div>
  );
}

export function EmptyState({ icon, title, children, action }: { icon?: ReactNode; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center anim-in">
      {icon && <div className="mb-1 rounded-xl border border-line bg-surface-2 p-3 text-muted">{icon}</div>}
      <h3 className="text-[14px] font-semibold text-fg">{title}</h3>
      {children && <div className="max-w-md text-[12.5px] text-muted">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function ErrorNote({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  return (
    <div role="alert" className="flex items-start gap-2 rounded-md border border-danger/30 bg-danger-soft p-3 text-[12.5px] text-danger">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
      <div className="flex-1 break-words">{typeof error === "string" ? error : error instanceof Error ? error.message : String(error)}</div>
      {onRetry && <Button size="sm" variant="ghost" onClick={onRetry}>Retry</Button>}
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded border border-line-strong bg-surface-2 px-1.5 py-px font-mono text-[10.5px] text-muted">{children}</kbd>;
}

export function Modal({ open, onClose, title, children, footer, wide }: { open: boolean; onClose: () => void; title: string; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const focusable = node?.querySelector<HTMLElement>("input, textarea, select, button[data-autofocus], button:not([aria-label='Close'])");
    focusable?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "Tab" && node) {
        const items = Array.from(node.querySelectorAll<HTMLElement>("button, input, textarea, select, a[href]")).filter((el) => !el.hasAttribute("disabled"));
        if (items.length === 0) return;
        const first = items[0], last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("keydown", onKey); prev?.focus(); };
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/55 p-6 pt-[8vh]" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} role="dialog" aria-modal="true" aria-label={title} className={cx("anim-pop flex max-h-[84vh] w-full flex-col rounded-xl border border-line-strong bg-surface shadow-[var(--shadow)]", wide ? "max-w-4xl" : "max-w-lg")}>
        <header className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="text-[14px] font-semibold">{title}</h2>
          <IconButton label="Close" onClick={onClose}><X className="size-4" /></IconButton>
        </header>
        <div className="flex-1 overflow-y-auto p-4">{children}</div>
        {footer && <footer className="flex justify-end gap-2 border-t border-line px-4 py-3">{footer}</footer>}
      </div>
    </div>
  );
}

/** Confirmation dialog for destructive actions. */
export function ConfirmModal({ open, title, body, confirmLabel, onConfirm, onClose, busy }: {
  open: boolean; title: string; body: ReactNode; confirmLabel: string; onConfirm: () => void; onClose: () => void; busy?: boolean;
}) {
  return (
    <Modal open={open} onClose={onClose} title={title} footer={<>
      <Button variant="ghost" onClick={onClose}>Cancel</Button>
      <Button variant="danger" data-autofocus onClick={onConfirm} loading={busy}>{confirmLabel}</Button>
    </>}>
      <div className="text-[13px] text-muted">{body}</div>
    </Modal>
  );
}

// ---------- toasts
type Toast = { id: number; tone: "ok" | "error" | "info"; text: string };
const ToastCtx = createContext<(tone: Toast["tone"], text: string) => void>(() => {});
export function useToast() {
  return useContext(ToastCtx);
}
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((tone: Toast["tone"], text: string) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, tone, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), tone === "error" ? 7000 : 3500);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[360px] flex-col gap-2" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className="anim-pop pointer-events-auto flex items-start gap-2 rounded-lg border border-line-strong bg-surface-2 p-3 text-[12.5px] shadow-[var(--shadow)]">
            {t.tone === "ok" ? <CheckCircle2 className="size-4 shrink-0 text-ok" /> : t.tone === "error" ? <AlertTriangle className="size-4 shrink-0 text-danger" /> : <Info className="size-4 shrink-0 text-info" />}
            <span className="flex-1 break-words text-fg">{t.text}</span>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-5 flex flex-wrap items-end justify-between gap-3">
      <div>
        <h1 className="text-[20px] font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-0.5 text-[12.5px] text-muted">{subtitle}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Stat({ label, value, hint, tone }: { label: string; value: ReactNode; hint?: ReactNode; tone?: "warn" | "danger" | "ok" }) {
  return (
    <div className="rounded-lg border border-line bg-surface px-4 py-3">
      <div className="text-[11.5px] font-medium uppercase tracking-wider text-faint">{label}</div>
      <div className={cx("mt-1 font-mono text-[22px] font-semibold tabular-nums", tone === "warn" && "text-warn", tone === "danger" && "text-danger", tone === "ok" && "text-ok")}>{value}</div>
      {hint && <div className="mt-0.5 text-[11.5px] text-muted">{hint}</div>}
    </div>
  );
}
