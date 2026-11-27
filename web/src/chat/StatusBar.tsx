import { useEffect, useRef, useState, type ReactNode } from "react";
import { contextPercent, formatCost, formatTokens, modelLabel, queueOf, usageTotals } from "./model";
import { Modal, ModelPicker } from "./panels";
import { abortCompaction, cycleDetail, setServiceTier, setSessionOption, setThinking, useChat } from "./store";
import { runSlash } from "./slash";

const LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const TIERS: { value: string | null; label: string }[] = [
  { value: null, label: "Default" },
  { value: "auto", label: "Auto" },
  { value: "flex", label: "Flex" },
  { value: "priority", label: "Priority (fast)" },
];

function Menu({ label, button, children }: { label: string; button: ReactNode; children: (close: () => void) => ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onDown); document.removeEventListener("keydown", onKey); };
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button type="button" aria-haspopup="menu" aria-expanded={open} aria-label={label} className="rounded-md px-2 py-1 text-xs hover:bg-selected" onClick={() => setOpen(!open)}>{button}</button>
      {open ? <div role="menu" className="bh-fade-in absolute bottom-full right-0 z-30 mb-1 min-w-44 rounded-xl border border-grid/50 bg-surface p-1 shadow-xl sm:left-0 sm:right-auto">{children(() => setOpen(false))}</div> : null}
    </div>
  );
}

const Item = ({ active, onClick, children }: { active?: boolean; onClick: () => void; children: ReactNode }) => (
  <button type="button" role="menuitemradio" aria-checked={!!active} className={`flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left text-sm hover:bg-selected ${active ? "text-primary-soft" : ""}`} onClick={onClick}>
    <span className="w-3">{active ? "✓" : ""}</span>{children}
  </button>
);

export function SessionSettings({ onClose }: { onClose: () => void }) {
  const state = useChat((s) => s.data.state);
  const row = (label: string, control: ReactNode) => (
    <div className="flex items-center justify-between gap-4 border-b border-grid/20 py-2 text-sm"><span>{label}</span>{control}</div>
  );
  const select = (value: string, options: [string, string][], onChange: (v: string) => void) => (
    <select className="bh-input !w-auto" value={value} onChange={(e) => onChange(e.target.value)}>{options.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
  );
  return (
    <Modal title="Session options" onClose={onClose}>
      {row("Steering delivery", select(state.steeringMode ?? "all", [["all", "All queued at once"], ["one-at-a-time", "One at a time"]], (mode) => void setSessionOption("set_steering_mode", { mode })))}
      {row("Follow-up delivery", select(state.followUpMode ?? "one-at-a-time", [["all", "All queued at once"], ["one-at-a-time", "One at a time"]], (mode) => void setSessionOption("set_follow_up_mode", { mode })))}
      {row("Transport", select(state.transport ?? "auto", [["auto", "Auto"], ["sse", "SSE"], ["websocket", "WebSocket"], ["websocket-cached", "WebSocket (cached)"]], (transport) => void setSessionOption("set_transport", { transport })))}
      {row("Auto-compaction", <input type="checkbox" checked={!!state.autoCompactionEnabled} onChange={(e) => void setSessionOption("set_auto_compaction", { enabled: e.target.checked })} />)}
      {row("Auto-retry", <input type="checkbox" checked={state.autoRetryEnabled !== false} onChange={(e) => void setSessionOption("set_auto_retry", { enabled: e.target.checked })} />)}
      <p className="pt-3 text-xs text-dim">Scoped models (the Alt+M cycle list) are configured in settings.</p>
    </Modal>
  );
}

export function StatusBar() {
  const data = useChat((s) => s.data);
  const detail = useChat((s) => s.detail);
  const state = data.state;
  const [picker, setPicker] = useState(false);
  const [options, setOptions] = useState(false);
  const pct = contextPercent(state);
  const totals = usageTotals(data.messages);
  const queue = queueOf(state);
  const queued = queue.steering.length + queue.followUps.length;
  const levels: string[] = Array.isArray(state.availableThinkingLevels) && state.availableThinkingLevels.length ? state.availableThinkingLevels : LEVELS;
  const reasoning = !(levels.length === 1 && levels[0] === "off");
  const pctTone = pct === null ? "text-dim" : pct > 85 ? "text-error" : pct > 65 ? "text-warning" : "text-muted";

  useEffect(() => {
    const open = () => setPicker(true);
    window.addEventListener("bh:chat-model-picker", open);
    return () => window.removeEventListener("bh:chat-model-picker", open);
  }, []);

  return (
    <>
      <div className="flex flex-wrap items-center gap-x-1 gap-y-0.5 border-t border-grid/30 bg-surface px-2 py-1 text-xs text-muted sm:px-3">
        <button type="button" className="max-w-[14rem] truncate rounded-md px-2 py-1 font-medium text-fg hover:bg-selected" title="Select model (Ctrl+L)" onClick={() => setPicker(true)}>{modelLabel(state)}</button>
        {reasoning ? (
          <Menu label="Thinking level" button={<span>thinking: <b className="font-medium text-primary-soft">{state.thinkingLevel ?? "off"}</b></span>}>
            {(close) => levels.map((level) => <Item key={level} active={level === state.thinkingLevel} onClick={() => { void setThinking(level); close(); }}>{level}</Item>)}
          </Menu>
        ) : null}
        <Menu label="Service tier" button={<span>tier: {state.serviceTier ?? "default"}</span>}>
          {(close) => TIERS.map((t) => <Item key={t.label} active={(t.value ?? "default") === (state.serviceTier ?? "default") || (t.value === null && state.serviceTier === "auto" && false)} onClick={() => { void setServiceTier(t.value); close(); }}>{t.label}</Item>)}
        </Menu>
        <span className="flex-1" />
        {state.isCompacting ? <button className="bh-btn !px-2 !py-0.5" onClick={() => void abortCompaction()}>Stop compaction</button> : null}
        {state.goal?.status === "active" ? <span className="px-1 text-success" title="Goal active">◎</span> : null}
        {queued ? <span className="px-1 text-warning" title="Queued messages">⏳ {queued}</span> : null}
        <span className={`px-1 ${pctTone}`} title={state.contextUsage ? `${state.contextUsage.tokens.toLocaleString()} / ${state.contextUsage.contextWindow.toLocaleString()} tokens` : "Context usage unknown"}>
          ctx {pct === null ? "–" : `${pct.toFixed(0)}%`}
        </span>
        <span className="px-1" title={`in ${totals.input.toLocaleString()} · out ${totals.output.toLocaleString()} · cache r/w ${totals.cacheRead.toLocaleString()}/${totals.cacheWrite.toLocaleString()} (loaded transcript)`}>
          ↑{formatTokens(totals.input)} ↓{formatTokens(totals.output)} · {formatCost(totals.cost)}
        </span>
        <button type="button" className="rounded-md px-2 py-1 hover:bg-selected" title="Compact the context" disabled={!!state.isCompacting || !!state.isStreaming} onClick={() => void runSlash("/compact", "steer")}>compact</button>
        <button type="button" className="rounded-md px-2 py-1 hover:bg-selected" title="Conversation detail (Ctrl+O)" onClick={cycleDetail}>detail {["▫", "◪", "▣"][detail]}</button>
        <button type="button" className="rounded-md px-2 py-1 hover:bg-selected" aria-label="Session options" title="Session options" onClick={() => setOptions(true)}>⚙</button>
      </div>
      {picker ? <ModelPicker onClose={() => setPicker(false)} /> : null}
      {options ? <SessionSettings onClose={() => setOptions(false)} /> : null}
    </>
  );
}
