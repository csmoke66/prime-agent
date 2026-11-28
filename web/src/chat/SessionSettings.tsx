import { useEffect, useState, type ReactNode } from "react";
import { wire } from "../wire";
import { Modal } from "../panels/ui/Modal";
import { Badge, ErrorLine, Spinner } from "../panels/ui/bits";
import { useAsync } from "../panels/useAsync";
import { toast, errorText } from "../panels/toasts";
import { modelLabel } from "./model";
import { setServiceTier, setSessionOption, setThinking, useChat } from "./store";

const LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const TIERS: [string, string][] = [["default", "Default"], ["auto", "Auto"], ["flex", "Flex"], ["priority", "Priority (fast)"]];

type ChildModel = { selector: string; name: string; allowed: boolean; inherited: boolean };
type ChildPolicy = { modelRules: number | null; inherited: "all" | "unreadable" | number; effort: string | null; models: ChildModel[] };

async function childPolicyCall(method: "GET" | "PUT", sessionFile: string, body?: Record<string, unknown>): Promise<ChildPolicy> {
  const response = await fetch(method === "GET" ? `/api/child-policy?sessionFile=${encodeURIComponent(sessionFile)}` : "/api/child-policy", {
    method,
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: method === "PUT" ? JSON.stringify({ sessionFile, ...body }) : undefined,
  });
  const text = await response.text();
  if (!response.ok) throw new Error(text || response.statusText);
  return JSON.parse(text);
}

const Group = ({ title, children }: { title: string; children: ReactNode }) => (
  <section className="border-b border-line py-2 last:border-0">
    <h3 className="bh-section-title mb-1">{title}</h3>
    {children}
  </section>
);

const Row = ({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) => (
  <div className="flex items-center justify-between gap-4 py-1.5 text-sm">
    <div className="min-w-0">
      <div>{label}</div>
      {hint ? <div className="text-xs text-dim">{hint}</div> : null}
    </div>
    <div className="shrink-0">{children}</div>
  </div>
);

const Select = ({ label, value, options, onChange }: { label: string; value: string; options: [string, string][]; onChange: (v: string) => void }) => (
  <select className="bh-input !w-auto min-w-40" aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
    {options.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
  </select>
);

/** Everything that applies only to the open session (and, for subagent policy, its family). Global settings live in the sidebar. */
export function SessionSettings({ onClose, onPickModel }: { onClose: () => void; onPickModel: () => void }) {
  const activeSessionId = useChat((s) => s.activeSessionId) ?? "";
  const state = useChat((s) => s.data.state);
  const [segments, setSegments] = useState(false);
  const levels: string[] = Array.isArray(state.availableThinkingLevels) && state.availableThinkingLevels.length ? state.availableThinkingLevels : LEVELS;
  const option = (command: Parameters<typeof setSessionOption>[0], payload: Record<string, unknown>) => void setSessionOption(command, payload);

  return (
    <Modal title="Session settings" subtitle="These apply to this session only. Global settings are under Settings at the bottom of the sidebar." size="lg" onClose={onClose}>
      <Group title="Model">
        <Row label="Model"><button className="bh-btn font-mono" onClick={() => { onClose(); onPickModel(); }}>{modelLabel(state)}</button></Row>
        <Row label="Thinking level"><Select label="Thinking level" value={state.thinkingLevel ?? "off"} options={levels.map((l) => [l, l])} onChange={(level) => void setThinking(level)} /></Row>
        <Row label="Service tier" hint="Priority is the fast tier, when the model supports it"><Select label="Service tier" value={state.serviceTier ?? "default"} options={TIERS} onChange={(tier) => void setServiceTier(tier === "default" ? null : tier)} /></Row>
        <Row label="Transport"><Select label="Transport" value={state.transport ?? "auto"} options={[["auto", "Auto"], ["sse", "SSE"], ["websocket", "WebSocket"], ["websocket-cached", "WebSocket (cached)"]]} onChange={(transport) => option("set_transport", { transport })} /></Row>
      </Group>
      <Group title="Messages">
        <Row label="Steering delivery" hint="Messages sent while the agent works"><Select label="Steering delivery" value={state.steeringMode ?? "one-at-a-time"} options={[["one-at-a-time", "One at a time"], ["all", "All at once"]]} onChange={(mode) => option("set_steering_mode", { mode })} /></Row>
        <Row label="Follow-up delivery" hint="Messages queued until the agent stops"><Select label="Follow-up delivery" value={state.followUpMode ?? "one-at-a-time"} options={[["one-at-a-time", "One at a time"], ["all", "All at once"]]} onChange={(mode) => option("set_follow_up_mode", { mode })} /></Row>
        <Row label="Auto-compaction"><input type="checkbox" aria-label="Auto-compaction" checked={!!state.autoCompactionEnabled} onChange={(e) => option("set_auto_compaction", { enabled: e.target.checked })} /></Row>
        <Row label="Auto-retry"><input type="checkbox" aria-label="Auto-retry" checked={state.autoRetryEnabled !== false} onChange={(e) => option("set_auto_retry", { enabled: e.target.checked })} /></Row>
        <Row label="System prompt segments" hint="Named parts of this session's system prompt"><button className="bh-btn" onClick={() => setSegments(true)}>Edit…</button></Row>
      </Group>
      <Group title="Subagents">
        <RlmDepth activeSessionId={activeSessionId} />
        {state.sessionFile ? <ChildPolicyRows sessionFile={String(state.sessionFile)} levels={levels} /> : <p className="py-1.5 text-xs text-dim">Subagent policy is available once the session is saved.</p>}
      </Group>
      {segments && <PromptSegmentsDialog activeSessionId={activeSessionId} onClose={() => setSegments(false)} />}
    </Modal>
  );
}

function RlmDepth({ activeSessionId }: { activeSessionId: string }) {
  const status = useAsync(() => wire.call<{ maxDepth?: number; source?: string }>({ type: "get_rlm_max_depth_status", activeSessionId }), [activeSessionId]);
  const [depth, setDepth] = useState("");
  const [global, setGlobal] = useState(false);
  useEffect(() => setDepth(status.data?.maxDepth === undefined ? "" : String(status.data.maxDepth)), [status.data]);
  const save = async () => {
    try {
      await wire.call({ type: "set_rlm_max_depth", activeSessionId, maxDepth: Number(depth), global });
      toast.success(global ? "Max depth set for this session and saved as the global default" : "Max depth set for this session");
      status.reload();
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  const changed = depth !== "" && depth !== String(status.data?.maxDepth ?? "");
  return (
    <Row label="Max subagent depth" hint={status.data?.source ? `Currently from: ${status.data.source}` : "How deep subagents may nest"}>
      <div className="flex items-center gap-2">
        <label className="flex items-center gap-1 text-xs text-dim"><input type="checkbox" checked={global} onChange={(e) => setGlobal(e.target.checked)} /> also global default</label>
        <input className="bh-input !w-16" type="number" min={0} aria-label="Max subagent depth" value={depth} onChange={(e) => setDepth(e.target.value)} />
        <button className="bh-btn" disabled={!changed && !global} onClick={() => void save()}>Apply</button>
      </div>
    </Row>
  );
}

function ChildPolicyRows({ sessionFile, levels }: { sessionFile: string; levels: string[] }) {
  const [policy, setPolicy] = useState<ChildPolicy>();
  const [error, setError] = useState("");
  const [editing, setEditing] = useState(false);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const run = async (method: "GET" | "PUT", body?: Record<string, unknown>) => {
    try {
      const next = await childPolicyCall(method, sessionFile, body);
      setPolicy(next);
      setError("");
      return next;
    } catch (reason) {
      setError(errorText(reason));
      return undefined;
    }
  };
  useEffect(() => void run("GET"), [sessionFile]); // eslint-disable-line react-hooks/exhaustive-deps

  if (error && !policy) return <ErrorLine>{error}</ErrorLine>;
  if (!policy) return <Spinner label="Loading subagent policy" />;

  const inheritedText = policy.inherited === "all" ? "all models" : policy.inherited === "unreadable" ? "the global list (unreadable)" : `the global list (${policy.inherited} rule${policy.inherited === 1 ? "" : "s"})`;
  const summary = policy.modelRules === null ? `Inherits ${inheritedText}` : policy.modelRules === 0 ? "All models" : `${policy.modelRules} selected`;
  const startEdit = () => {
    setChecked(new Set(policy.models.filter((m) => m.allowed).map((m) => m.selector)));
    setFilter("");
    setEditing(true);
  };
  const shown = policy.models.filter((m) => `${m.selector} ${m.name}`.toLowerCase().includes(filter.toLowerCase()));
  const saveModels = async (models: string[] | null) => {
    if (await run("PUT", { models })) {
      setEditing(false);
      toast.success("Subagent models saved for this session family");
    }
  };

  return (
    <>
      <Row label="Allowed subagent models" hint="Applies to this session and the subagents it spawns">
        <div className="flex items-center gap-2"><Badge>{summary}</Badge><button className="bh-btn" onClick={() => (editing ? setEditing(false) : startEdit())}>{editing ? "Cancel" : "Edit…"}</button></div>
      </Row>
      {editing ? (
        <div className="mb-2 rounded border border-line bg-surface p-2">
          <input className="bh-input mb-2" placeholder="Filter models…" value={filter} onChange={(e) => setFilter(e.target.value)} autoFocus />
          <ul className="max-h-64 overflow-y-auto text-[12px]">
            {shown.map((m) => (
              <li key={m.selector}>
                <label className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-selected">
                  <input type="checkbox" checked={checked.has(m.selector)} onChange={(e) => setChecked((prev) => { const next = new Set(prev); if (e.target.checked) next.add(m.selector); else next.delete(m.selector); return next; })} />
                  <span className="font-mono">{m.selector}</span>
                  <span className="truncate text-dim">{m.name}</span>
                  {!m.inherited ? <span className="ml-auto text-[10px] text-warning" title="Not allowed by the global list">outside global</span> : null}
                </label>
              </li>
            ))}
          </ul>
          <div className="mt-2 flex flex-wrap justify-end gap-2">
            <button className="bh-btn" onClick={() => void saveModels(null)}>Inherit global</button>
            <button className="bh-btn" onClick={() => void saveModels([])}>Allow all</button>
            <button className="bh-btn bh-btn-primary" disabled={checked.size === 0} onClick={() => void saveModels([...checked])}>Save {checked.size} selected</button>
          </div>
        </div>
      ) : null}
      <Row label="Subagent thinking level" hint="For newly spawned subagents">
        <Select
          label="Subagent thinking level"
          value={policy.effort ?? ""}
          options={[["", "Inherit"], ...levels.map((l): [string, string] => [l, l])]}
          onChange={(effort) => void run("PUT", { effort: effort || null })}
        />
      </Row>
      <ErrorLine>{error}</ErrorLine>
    </>
  );
}

type Segment = { name: string; body: string; state: "default" | "modified" | "removed" | "added" };

function PromptSegmentsDialog({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const [segments, setSegments] = useState<Segment[]>();
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<Record<string, string>>({});
  const [added, setAdded] = useState({ name: "", body: "" });
  const run = async (edit?: Record<string, unknown>) => {
    try {
      const data = await wire.call({ type: "session_prompt_segments", activeSessionId, edit });
      setSegments(Array.isArray(data) ? data : data?.segments ?? []);
      setError("");
    } catch (reason) {
      setError(errorText(reason));
    }
  };
  useEffect(() => void run(), []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Modal title="System prompt segments" subtitle="Edits apply to this session only." size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <ErrorLine>{error}</ErrorLine>
        {!segments && !error && <Spinner />}
        {segments?.map((segment) => (
          <div key={segment.name} className="bh-card flex flex-col gap-2 p-3">
            <div className="flex items-center gap-2">
              <span className="font-mono text-sm">{segment.name}</span>
              <Badge tone={segment.state === "default" ? "neutral" : segment.state === "removed" ? "error" : "warning"}>{segment.state}</Badge>
              <span className="ml-auto flex gap-1">
                <button className="bh-btn px-2! py-1!" onClick={() => void run({ op: "reset", name: segment.name })}>Reset</button>
                <button className="bh-btn bh-btn-danger px-2! py-1!" onClick={() => void run({ op: "remove", name: segment.name })}>Remove</button>
              </span>
            </div>
            <textarea
              className="bh-input min-h-24! font-mono text-xs"
              aria-label={`${segment.name} body`}
              value={editing[segment.name] ?? segment.body}
              onChange={(e) => setEditing((prev) => ({ ...prev, [segment.name]: e.target.value }))}
            />
            {editing[segment.name] !== undefined && editing[segment.name] !== segment.body && (
              <button className="bh-btn bh-btn-primary self-end" onClick={() => void run({ op: "replace", name: segment.name, body: editing[segment.name] }).then(() => setEditing(({ [segment.name]: _, ...rest_ }) => rest_))}>
                Save segment
              </button>
            )}
          </div>
        ))}
        <div className="bh-card flex flex-col gap-2 p-3">
          <span className="text-sm text-muted">Add a segment</span>
          <input className="bh-input" placeholder="name" value={added.name} onChange={(e) => setAdded({ ...added, name: e.target.value })} />
          <textarea className="bh-input min-h-16! font-mono text-xs" placeholder="body" value={added.body} onChange={(e) => setAdded({ ...added, body: e.target.value })} />
          <button className="bh-btn self-end" disabled={!added.name.trim()} onClick={() => void run({ op: "add", name: added.name.trim(), body: added.body }).then(() => setAdded({ name: "", body: "" }))}>Add</button>
        </div>
      </div>
    </Modal>
  );
}

/** Session-scoped model rows: thinking level and service tier straight from the attached session. */
