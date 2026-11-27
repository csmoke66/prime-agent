import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Markdown } from "./markdown";
import { queueOf, type SideTurn } from "./model";
import { matches } from "../app/keybindings";
import {
  abortAndSendQueued,
  clearQueue,
  closeSidePane,
  insertIntoComposer,
  loadModels,
  mutateQueued,
  openInfo,
  sendPrompt,
  setModel,
  startSideQuestion,
  useChat,
} from "./store";

export function Modal({ title, onClose, children, wide }: { title: string; onClose: () => void; children: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopPropagation(); onClose(); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/60 p-3 backdrop-blur-sm" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label={title} className={`bh-fade-in flex max-h-[85vh] w-full flex-col overflow-hidden rounded-lg border border-grid/50 bg-surface shadow-2xl outline-none ${wide ? "max-w-3xl" : "max-w-xl"}`}>
        <div className="flex items-center gap-2 border-b border-grid/30 px-4 py-2.5">
          <h2 className="flex-1 text-sm font-semibold">{title}</h2>
          <button className="bh-btn !px-2 !py-0.5" onClick={onClose} aria-label="Close">✕</button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-4">{children}</div>
      </div>
    </div>
  );
}

export function InfoModal() {
  const info = useChat((s) => s.info);
  if (!info) return null;
  return (
    <Modal title={info.title} onClose={() => openInfo(null)} wide>
      {info.markdown ? <Markdown text={info.markdown} /> : <pre className="bh-pre whitespace-pre-wrap break-words text-[12px] leading-5 text-muted">{info.text}</pre>}
      <div className="mt-3 flex justify-end">
        <button className="bh-btn" onClick={() => void navigator.clipboard?.writeText(info.text ?? info.markdown ?? "")}>Copy</button>
      </div>
    </Modal>
  );
}

export function ModelPicker({ onClose }: { onClose: () => void }) {
  const models = useChat((s) => s.models);
  const state = useChat((s) => s.data.state);
  const providers = useChat((s) => s.configuredProviders);
  const [query, setQuery] = useState("");
  const [onlyConfigured, setOnlyConfigured] = useState(true);
  useEffect(() => { void loadModels(); }, []);
  const current = state.model ?? {};
  const scopedKeys = useMemo(() => ((state.scopedModels ?? []) as { model?: { provider?: string; id?: string } }[]).flatMap((e) => (e.model ? [`${e.model.provider}/${e.model.id}`] : [])), [state.scopedModels]);
  const [scoped, setScoped] = useState(scopedKeys.length > 0);
  useEffect(() => {
    if (!scopedKeys.length) return;
    const onKey = (e: KeyboardEvent) => { if (matches(e, "app.model.toggleScope")) { e.preventDefault(); setScoped((v) => !v); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [scopedKeys.length]);
  const list = useMemo(() => {
    const q = query.toLowerCase();
    return (models ?? [])
      .filter((m) => (!scoped || scopedKeys.includes(`${m.provider}/${m.id}`)) && (!onlyConfigured || providers.includes(m.provider)) && `${m.provider}/${m.id} ${m.name ?? ""}`.toLowerCase().includes(q))
      .slice(0, 200);
  }, [models, query, onlyConfigured, providers, scoped, scopedKeys]);
  return (
    <Modal title="Select model" onClose={onClose}>
      <div className="mb-3 flex items-center gap-2">
        <input autoFocus className="bh-input" placeholder="Search models…" value={query} onChange={(e) => setQuery(e.target.value)} />
        {scopedKeys.length ? <label className="flex items-center gap-1.5 whitespace-nowrap text-xs text-muted" title="Alt+S"><input type="checkbox" checked={scoped} onChange={(e) => setScoped(e.target.checked)} /> scoped</label> : null}
        <label className="flex items-center gap-1.5 whitespace-nowrap text-xs text-muted"><input type="checkbox" checked={onlyConfigured} onChange={(e) => setOnlyConfigured(e.target.checked)} /> signed in</label>
      </div>
      {!models ? <div className="text-muted">Loading…</div> : null}
      <ul className="space-y-0.5">
        {list.map((m) => {
          const active = m.id === current.id && m.provider === current.provider;
          return (
            <li key={`${m.provider}/${m.id}`}>
              <button className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm hover:bg-selected ${active ? "bg-selected" : ""}`} onClick={() => { void setModel(m.provider, m.id); onClose(); }}>
                <span className="w-24 shrink-0 truncate text-xs text-dim">{m.provider}</span>
                <span className="min-w-0 flex-1 truncate">{m.name ?? m.id}</span>
                {m.reasoning ? <span className="text-[11px] text-primary-soft">reasoning</span> : null}
                {m.contextWindow ? <span className="text-[11px] text-dim">{Math.round(m.contextWindow / 1000)}k</span> : null}
                {active ? <span className="text-success">✓</span> : null}
              </button>
            </li>
          );
        })}
        {models && !list.length ? <li className="px-3 py-6 text-center text-muted">No models match.</li> : null}
      </ul>
    </Modal>
  );
}

export function QueuePanel() {
  const state = useChat((s) => s.data.state);
  const queue = queueOf(state);
  const [editing, setEditing] = useState<{ lane: "steering" | "followUp"; index: number; text: string } | null>(null);
  const count = queue.steering.length + queue.followUps.length;
  if (!count && !queue.starting) return null;
  const lanes: { lane: "steering" | "followUp"; label: string; items: string[] }[] = [
    { lane: "steering", label: "Steer", items: queue.steering },
    { lane: "followUp", label: "Next", items: queue.followUps },
  ];
  return (
    <div className="border-t border-grid/30 bg-surface px-2 pt-1.5">
      <div className="max-h-28 w-full overflow-y-auto rounded border border-grid/40 bg-panel p-1 text-[12px]">
        <div className="mb-1 flex items-center gap-2 text-xs text-muted">
          <span className="font-medium">Queued ({count})</span>
          {queue.starting ? <span className="truncate text-dim">starting: {queue.starting}</span> : null}
          <span className="flex-1" />
          <button className="bh-btn !px-2 !py-0.5 text-xs" title="Abort and send the queue now" onClick={() => void abortAndSendQueued()}>Send now</button>
          <button className="bh-btn bh-btn-danger !px-2 !py-0.5 text-xs" onClick={() => void clearQueue()}>Clear</button>
        </div>
        {lanes.map(({ lane, label, items }) =>
          items.map((text, index) => {
            const isEditing = editing?.lane === lane && editing.index === index;
            return (
              <div key={`${lane}-${index}-${text}`} className="flex items-center gap-2 rounded px-1.5 hover:bg-selected/60">
                <span className={`w-12 shrink-0 font-mono text-[10px] uppercase ${lane === "steering" ? "text-warning" : "text-info"}`}>{label}</span>
                {isEditing ? (
                  <input
                    autoFocus
                    className="bh-input !py-0.5"
                    value={editing.text}
                    onChange={(e) => setEditing({ ...editing, text: e.target.value })}
                    onKeyDown={(e) => {
                      if (e.key === "Escape") setEditing(null);
                      if (e.key === "Enter") {
                        const trimmed = editing.text.trim();
                        void mutateQueued(lane, index, text, trimmed ? { type: "replace", text: trimmed, lane } : { type: "delete" });
                        setEditing(null);
                      }
                    }}
                  />
                ) : (
                  <span className="min-w-0 flex-1 truncate" title={text}>{text}</span>
                )}
                <span className="flex shrink-0 gap-0.5 text-xs">
                  <IconBtn label="Move earlier" disabled={index === 0} onClick={() => void mutateQueued(lane, index, text, { type: "move", direction: -1 })}>↑</IconBtn>
                  <IconBtn label="Move later" disabled={index === items.length - 1} onClick={() => void mutateQueued(lane, index, text, { type: "move", direction: 1 })}>↓</IconBtn>
                  <IconBtn label="Edit" onClick={() => setEditing({ lane, index, text })}>✎</IconBtn>
                  <IconBtn label="Restore into the composer" onClick={async () => { if ((await mutateQueued(lane, index, text, { type: "delete" })) === "applied") insertIntoComposer(text); }}>⤴</IconBtn>
                  <IconBtn label="Delete" onClick={() => void mutateQueued(lane, index, text, { type: "delete" })}>✕</IconBtn>
                </span>
              </div>
            );
          }),
        )}
      </div>
    </div>
  );
}

const IconBtn = ({ label, onClick, children, disabled }: { label: string; onClick: () => void; children: ReactNode; disabled?: boolean }) => (
  <button type="button" aria-label={label} title={label} disabled={disabled} onClick={onClick} className="grid h-6 w-6 place-items-center rounded text-muted hover:bg-grid/40 hover:text-fg disabled:opacity-30">{children}</button>
);

export function GoalBar() {
  const goal = useChat((s) => s.data.state.goal);
  if (!goal || (!goal.objective && !goal.active)) return null;
  const budget = goal.tokenBudget ? Math.min(100, Math.round((goal.tokensUsed / goal.tokenBudget) * 100)) : null;
  const tone = goal.status === "active" ? "text-success" : goal.status === "paused" ? "text-warning" : goal.status === "error" || goal.status === "budget_limited" ? "text-error" : "text-muted";
  const act = (text: string) => void sendPrompt(text, "steer");
  return (
    <div className="border-b border-grid/30 bg-surface px-2.5 py-1 text-[11px]">
      <div className="flex w-full flex-wrap items-center gap-x-3 gap-y-1">
        <span className={`font-medium ${tone}`}>◎ goal · {String(goal.status).replace("_", " ")}</span>
        <span className="min-w-0 flex-1 truncate text-muted" title={goal.objective}>{goal.objective}</span>
        {budget !== null ? <span className="text-dim">{budget}% of {goal.tokenBudget.toLocaleString()} tokens</span> : <span className="text-dim">{(goal.tokensUsed ?? 0).toLocaleString()} tokens</span>}
        {goal.status === "active" ? <button className="bh-btn !px-2 !py-0.5" onClick={() => act("/goal pause")}>Pause</button> : null}
        {goal.status === "paused" || goal.status === "budget_limited" ? <button className="bh-btn !px-2 !py-0.5" onClick={() => act("/goal resume")}>Resume</button> : null}
        <button className="bh-btn bh-btn-danger !px-2 !py-0.5" onClick={() => act("/goal clear")}>Clear</button>
      </div>
      {goal.lastError ? <div className="mt-1 w-full text-error">{goal.lastError}</div> : null}
    </div>
  );
}

export function SidePane() {
  const side = useChat((s) => s.side);
  const open = useChat((s) => s.sideOpen);
  const [question, setQuestion] = useState("");
  if (!open) return null;
  const running = side.some((t: SideTurn) => t.status === "running");
  return (
    <div className="border-t border-primary/40 bg-panel px-2.5 py-1.5">
      <div className="w-full">
        <div className="mb-1 flex items-center text-xs text-primary-soft">
          <span className="flex-1 font-medium">/btw — side conversation (not added to the session)</span>
          <button className="bh-btn !px-2 !py-0.5" onClick={() => void closeSidePane()}>Close (Esc)</button>
        </div>
        <div className="max-h-64 space-y-2 overflow-auto">
          {side.map((turn) => (
            <div key={turn.id}>
              <div className="text-sm font-medium">{turn.question}</div>
              {turn.answer ? <Markdown text={turn.answer} className="text-sm" /> : null}
              {turn.status === "running" ? <span className="bh-pulse text-xs text-dim">thinking…</span> : null}
              {turn.errorMessage ? <div className="text-sm text-error">{turn.errorMessage}</div> : null}
            </div>
          ))}
        </div>
        <form className="mt-2 flex gap-2" onSubmit={(e) => { e.preventDefault(); if (question.trim() && !running) { void startSideQuestion(question.trim()); setQuestion(""); } }}>
          <input className="bh-input" placeholder="Follow-up question…" value={question} onChange={(e) => setQuestion(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); void closeSidePane(); } }} />
          <button className="bh-btn bh-btn-primary" disabled={running || !question.trim()}>Ask</button>
        </form>
      </div>
    </div>
  );
}
