import { useEffect, useRef, useState, type ReactNode } from "react";
import { formatCount, contextPercent, formatCost, formatTokens, modelLabel, queueOf, usageTotals } from "./model";
import { IconMore } from "../app/icons";
import { ModelPicker } from "./panels";
import { abortCompaction, cycleDetail, setServiceTier, setThinking, useChat } from "./store";
import { runSlash } from "./slash";
import { SessionSettings } from "./SessionSettings";

const LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const TIERS: { value: string | null; label: string }[] = [
  { value: null, label: "Default" },
  { value: "auto", label: "Auto" },
  { value: "flex", label: "Flex" },
  { value: "priority", label: "Priority (fast)" },
];

function Menu({ label, button, children, hideOnMobile }: { label: string; button: ReactNode; hideOnMobile?: boolean; children: (close: () => void) => ReactNode }) {
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
    <div ref={ref} className={`relative ${hideOnMobile ? "max-sm:hidden" : ""}`}>
      <button type="button" aria-haspopup="menu" aria-expanded={open} aria-label={label} className="rounded px-1.5 py-0.5 text-xs hover:bg-selected" onClick={() => setOpen(!open)}>{button}</button>
      {open ? <div role="menu" className="bh-fade-in absolute bottom-full right-0 z-30 mb-1 min-w-44 rounded border border-line bg-surface p-0.5 shadow-xl sm:left-0 sm:right-auto">{children(() => setOpen(false))}</div> : null}
    </div>
  );
}

const Item = ({ active, onClick, children }: { active?: boolean; onClick: () => void; children: ReactNode }) => (
  <button type="button" role="menuitemradio" aria-checked={!!active} className={`flex w-full items-center gap-2 rounded px-2 py-0.5 text-left text-[12px] hover:bg-selected ${active ? "text-primary" : ""}`} onClick={onClick}>
    <span className="w-3">{active ? "✓" : ""}</span>{children}
  </button>
);

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
      <div className="relative z-20 flex min-w-0 items-center gap-x-0.5 whitespace-nowrap border-t border-line bg-bg px-1.5 py-0.5 font-mono text-[11px] text-muted">
        <button type="button" className="min-w-0 max-w-[14rem] truncate rounded px-1.5 py-0.5 font-medium text-fg hover:bg-selected" title="Select model (Ctrl+L)" onClick={() => setPicker(true)}>{modelLabel(state)}</button>
        {reasoning ? (
          <Menu hideOnMobile label="Thinking level" button={<span>thinking: <b className="font-medium text-fg">{state.thinkingLevel ?? "off"}</b></span>}>
            {(close) => levels.map((level) => <Item key={level} active={level === state.thinkingLevel} onClick={() => { void setThinking(level); close(); }}>{level}</Item>)}
          </Menu>
        ) : null}
        <Menu hideOnMobile label="Service tier" button={<span>tier: {state.serviceTier ?? "default"}</span>}>
          {(close) => TIERS.map((t) => <Item key={t.label} active={(t.value ?? "default") === (state.serviceTier ?? "default") || (t.value === null && state.serviceTier === "auto" && false)} onClick={() => { void setServiceTier(t.value); close(); }}>{t.label}</Item>)}
        </Menu>
        <button type="button" className="rounded px-1.5 py-0.5 hover:bg-selected max-sm:hidden" title="This session's settings" onClick={() => setOptions(true)}>settings</button>
        <span className="flex-1" />
        {state.isCompacting ? <button className="bh-btn !px-2 !py-0.5" onClick={() => void abortCompaction()}>Stop compaction</button> : null}
        {state.goal?.status === "active" ? <span className="px-1 text-success max-sm:hidden" title="Goal active">◎ goal</span> : null}
        {queued ? <span className="px-1 text-warning" title="Queued messages">⏳ {queued}</span> : null}
        <span className={`px-1 ${pctTone}`} title={state.contextUsage ? `${formatCount(state.contextUsage.tokens)} / ${formatCount(state.contextUsage.contextWindow)} tokens` : "Context usage unknown"}>
          ctx {pct === null ? "–" : `${pct.toFixed(0)}%`}
        </span>
        <span className="whitespace-nowrap px-1 max-sm:hidden" title={`in ${formatCount(totals.input)} · out ${formatCount(totals.output)} · cache r/w ${formatCount(totals.cacheRead)}/${formatCount(totals.cacheWrite)} (loaded transcript)`}>
          ↑{formatTokens(totals.input)} ↓{formatTokens(totals.output)} · {formatCost(totals.cost)}
        </span>
        <span className="px-1 sm:hidden" title="Session cost">{formatCost(totals.cost)}</span>
        <button type="button" className="rounded px-1.5 py-0.5 hover:bg-selected max-sm:hidden" title="Compact the context" disabled={!!state.isCompacting || !!state.isStreaming} onClick={() => void runSlash("/compact", "steer")}>compact</button>
        <button type="button" className="rounded px-1.5 py-0.5 hover:bg-selected max-sm:hidden" title="Conversation detail (Ctrl+O)" onClick={cycleDetail}>detail {["▫", "◪", "▣"][detail]}</button>
        <div className="sm:hidden">
          <Menu label="Session details" button={<IconMore />}>
            {(close) => (
              <div className="w-56 space-y-2 p-2 text-[12px]">
                {reasoning ? (
                  <div>
                    <div className="mb-1 text-dim">Thinking</div>
                    <div className="flex flex-wrap gap-1">{levels.map((level) => <button key={level} type="button" className={`bh-btn !min-h-6 !px-1.5 !py-0 ${level === state.thinkingLevel ? "!border-primary text-primary" : ""}`} onClick={() => { void setThinking(level); close(); }}>{level}</button>)}</div>
                  </div>
                ) : null}
                <div>
                  <div className="mb-1 text-dim">Service tier</div>
                  <div className="flex flex-wrap gap-1">{TIERS.map((tier) => <button key={tier.label} type="button" className={`bh-btn !min-h-6 !px-1.5 !py-0 ${(tier.value ?? "default") === (state.serviceTier ?? "default") ? "!border-primary text-primary" : ""}`} onClick={() => { void setServiceTier(tier.value); close(); }}>{tier.label}</button>)}</div>
                </div>
                <div className="text-dim">{formatCount(totals.input)} in · {formatCount(totals.output)} out · {formatCost(totals.cost)}</div>
                {state.goal?.status === "active" ? <div className="text-success">◎ goal active</div> : null}
                <div className="flex flex-wrap gap-1 border-t border-line pt-2">
                  <button type="button" className="bh-btn" disabled={!!state.isCompacting || !!state.isStreaming} onClick={() => { close(); void runSlash("/compact", "steer"); }}>Compact</button>
                  <button type="button" className="bh-btn" onClick={cycleDetail}>Detail {["▫", "◪", "▣"][detail]}</button>
                  <button type="button" className="bh-btn" onClick={() => { close(); setOptions(true); }}>Settings</button>
                </div>
              </div>
            )}
          </Menu>
        </div>
      </div>
      {picker ? <ModelPicker onClose={() => setPicker(false)} /> : null}
      {options ? <SessionSettings onClose={() => setOptions(false)} onPickModel={() => setPicker(true)} /> : null}
    </>
  );
}
