import { useState, type ReactNode } from "react";

export function Spinner({ label }: { label?: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-muted" role="status">
      <span className="bh-pulse text-primary-soft">&#x2B22;</span>
      {label ? <span className="text-sm">{label}</span> : <span className="sr-only">Loading</span>}
    </span>
  );
}

export function ErrorLine({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <p role="alert" className="rounded-lg border border-error/40 bg-tool-error px-3 py-2 text-sm text-error">
      {children}
    </p>
  );
}

export function Empty({ title, hint }: { title: string; hint?: ReactNode }) {
  return (
    <div className="grid place-items-center gap-0.5 px-3 py-3 text-center">
      <p className="text-xs text-muted">{title}</p>
      {hint ? <p className="text-xs text-dim">{hint}</p> : null}
    </div>
  );
}

const TONES = {
  neutral: "border-grid/60 text-muted",
  success: "border-success/50 text-success",
  warning: "border-warning/50 text-warning",
  error: "border-error/50 text-error",
  info: "border-info/50 text-info",
  primary: "border-primary/60 text-primary-soft",
} as const;
export type Tone = keyof typeof TONES;

export function Badge({ tone = "neutral", children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span title={title} className={`inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-0.5 text-[0.7rem] leading-4 ${TONES[tone]}`}>
      {children}
    </span>
  );
}

export function StatusDot({ tone }: { tone: Tone }) {
  const color = { neutral: "bg-dim", success: "bg-success", warning: "bg-warning", error: "bg-error", info: "bg-info", primary: "bg-primary-soft" }[tone];
  return <span aria-hidden className={`inline-block size-2 shrink-0 rounded-full ${color} ${tone === "success" || tone === "info" ? "bh-pulse" : ""}`} />;
}

export function SearchInput({ value, onChange, placeholder, autoFocus, label }: { value: string; onChange: (v: string) => void; placeholder: string; autoFocus?: boolean; label?: string }) {
  return (
    <input
      className="bh-input"
      type="search"
      value={value}
      onChange={(event) => onChange(event.target.value)}
      placeholder={placeholder}
      aria-label={label ?? placeholder}
      data-autofocus={autoFocus ? "" : undefined}
      autoComplete="off"
      spellCheck={false}
    />
  );
}

export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full border transition-colors disabled:opacity-40 ${checked ? "border-primary bg-primary" : "border-grid bg-panel"}`}
    >
      <span className={`absolute top-0.5 size-4.5 rounded-full bg-fg transition-all ${checked ? "left-5.5" : "left-0.5"}`} />
    </button>
  );
}

export function Tabs<T extends string>({ tabs, active, onChange }: { tabs: readonly { id: T; label: string }[]; active: T; onChange: (id: T) => void }) {
  return (
    <div role="tablist" className="flex gap-1 overflow-x-auto border-b border-grid/40 pb-1">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          aria-selected={tab.id === active}
          onClick={() => onChange(tab.id)}
          className={`whitespace-nowrap rounded-md px-3 py-1.5 text-sm transition-colors ${tab.id === active ? "bg-selected text-fg" : "text-muted hover:bg-selected/60"}`}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

/** Two-step destructive button: first press arms it, second press confirms. */
export function ConfirmButton({ label, confirmLabel = "Confirm?", onConfirm, disabled, className = "" }: { label: ReactNode; confirmLabel?: string; onConfirm: () => void; disabled?: boolean; className?: string }) {
  const [armed, setArmed] = useState(false);
  return (
    <button
      disabled={disabled}
      className={`bh-btn bh-btn-danger ${armed ? "border-error text-error" : ""} ${className}`}
      onBlur={() => setArmed(false)}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else setArmed(true);
      }}
    >
      {armed ? confirmLabel : label}
    </button>
  );
}
