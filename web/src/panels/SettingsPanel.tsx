import { useEffect, useState } from "react";
import { wire } from "../wire";
import { Modal } from "./ui/Modal";
import { Badge, ErrorLine, Spinner, Tabs, Toggle } from "./ui/bits";
import { BackendUnavailable, rest, type Settings } from "./api";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";
import { SETTINGS_TABS, rowValue, setPath, type SettingRow } from "./settingsSchema";

const localGet = (path: string, fallback: unknown) => {
  const raw = localStorage.getItem(path);
  return raw === null ? fallback : JSON.parse(raw);
};

export function SettingsPanel({ activeSessionId, onClose }: { activeSessionId?: string; onClose: () => void }) {
  const [tab, setTab] = useState(SETTINGS_TABS[0].id);
  const settings = useAsync(() => rest.settings(), []);
  const [draft, setDraft] = useState<Settings>({});
  const [error, setError] = useState("");
  useEffect(() => {
    if (settings.data) setDraft(settings.data);
  }, [settings.data]);

  const current = SETTINGS_TABS.find((t) => t.id === tab)!;

  const change = async (row: SettingRow, value: unknown) => {
    setError("");
    try {
      if (row.local && row.path) {
        localStorage.setItem(row.path, JSON.stringify(value));
        window.dispatchEvent(new CustomEvent("bh:setting-changed", { detail: { path: row.path, value } }));
        setDraft((d) => ({ ...d }));
        return;
      }
      const stored = row.kind === "number" && value === "" ? undefined : value;
      const next = row.path ? setPath(draft, row.path, stored) : draft;
      if (row.path) {
        await rest.saveSettings(next);
        setDraft(next);
      }
      if (row.session && activeSessionId) {
        await wire.call({ type: row.session.command, activeSessionId, [row.session.field]: value });
      }
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  return (
    <Modal title="Settings" subtitle={activeSessionId ? "Changes apply to new sessions; marked rows also apply to the open session." : "Changes apply to new sessions."} size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <Tabs tabs={SETTINGS_TABS} active={tab} onChange={setTab} />
        {settings.loading && <Spinner label="Loading settings" />}
        {settings.error && (
          <ErrorLine>{settings.error.includes("does not provide") ? `${settings.error}. Settings stay read-only until it does.` : settings.error}</ErrorLine>
        )}
        <ErrorLine>{error}</ErrorLine>
        {!settings.loading && !settings.error && (
          <ul className="flex flex-col divide-y divide-grid/30">
            {tab === "models" && activeSessionId && <SessionModelRows activeSessionId={activeSessionId} />}
            {current.rows.map((row) => (
              <li key={row.id} className="flex flex-wrap items-center gap-3 py-3">
                <div className="min-w-48 flex-1">
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {row.label}
                    {row.session && activeSessionId && <Badge tone="info" title="Also applied to the attached session">session</Badge>}
                  </div>
                  <p className="text-xs text-muted">{row.description}</p>
                </div>
                <RowControl row={row} settings={draft} activeSessionId={activeSessionId} onChange={(value) => void change(row, value)} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </Modal>
  );
}

function RowControl({ row, settings, activeSessionId, onChange }: { row: SettingRow; settings: Settings; activeSessionId?: string; onChange: (value: unknown) => void }) {
  const value = row.local && row.path ? localGet(row.path, row.fallback) : rowValue(row, settings);
  switch (row.kind) {
    case "bool":
      return <Toggle checked={Boolean(value)} onChange={onChange} label={row.label} />;
    case "enum":
      return (
        <select className="bh-input w-auto! min-w-40!" aria-label={row.label} value={String(value ?? "")} onChange={(e) => onChange(row.id === "idle-eviction" && e.target.value !== "off" ? Number(e.target.value) : e.target.value)}>
          {(row.values ?? []).map((v) => <option key={v}>{v}</option>)}
        </select>
      );
    case "number":
      return (
        <NumberField label={row.label} value={value as number | undefined} min={row.min} max={row.max} step={row.step} onCommit={onChange} />
      );
    case "text":
      return <input className="bh-input w-64!" aria-label={row.label} defaultValue={String(value ?? "")} onBlur={(e) => onChange(e.target.value)} />;
    case "lines":
      return <LinesField label={row.label} value={(value as string[] | undefined) ?? []} onCommit={onChange} />;
    case "special":
      return <SpecialRow id={row.id} settings={settings} activeSessionId={activeSessionId} onChange={onChange} />;
  }
}

function NumberField({ label, value, min, max, step, onCommit }: { label: string; value: number | undefined; min?: number; max?: number; step?: number; onCommit: (v: unknown) => void }) {
  const [text, setText] = useState(value === undefined ? "" : String(value));
  useEffect(() => setText(value === undefined ? "" : String(value)), [value]);
  const commit = () => {
    if (text === "") return onCommit("");
    const n = Number(text);
    if (Number.isFinite(n)) onCommit(Math.min(max ?? Infinity, Math.max(min ?? -Infinity, n)));
  };
  return <input className="bh-input w-28!" type="number" aria-label={label} value={text} min={min} max={max} step={step} onChange={(e) => setText(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />;
}

function LinesField({ label, value, onCommit }: { label: string; value: string[]; onCommit: (v: unknown) => void }) {
  const [text, setText] = useState(value.join("\n"));
  useEffect(() => setText(value.join("\n")), [value]);
  return (
    <textarea
      className="bh-input min-h-20! w-full! font-mono text-xs sm:w-72!"
      aria-label={label}
      value={text}
      placeholder="provider/*"
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const lines = text.split("\n").map((l) => l.trim()).filter(Boolean);
        onCommit(lines.length ? lines : undefined);
      }}
    />
  );
}

function SpecialRow({ id, settings, activeSessionId, onChange }: { id: string; settings: Settings; activeSessionId?: string; onChange: (v: unknown) => void }) {
  switch (id) {
    case "default-model":
      return <DefaultModel settings={settings} activeSessionId={activeSessionId} />;
    case "rlm-max-depth":
      return <RlmDepth activeSessionId={activeSessionId} />;
    case "system-prompt-segments":
      return activeSessionId ? <PromptSegments activeSessionId={activeSessionId} /> : <span className="text-xs text-dim">open a session</span>;
    default:
      void onChange;
      return null;
  }
}

function DefaultModel({ settings, activeSessionId }: { settings: Settings; activeSessionId?: string }) {
  const stored = settings.defaultProvider && settings.defaultModel ? `${settings.defaultProvider}/${settings.defaultModel}` : "";
  const [value, setValue] = useState(stored);
  useEffect(() => setValue(stored), [stored]);
  const apply = async () => {
    const [provider, ...rest_] = value.split("/");
    const model = rest_.join("/");
    if (!provider || !model) return toast.error("Use provider/model");
    try {
      await rest.saveSettings({ ...settings, defaultProvider: provider, defaultModel: model });
      if (activeSessionId) await wire.call({ type: "set_model", activeSessionId, provider, modelId: model });
      toast.success(`Default model set to ${value}`);
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  return (
    <div className="flex gap-2">
      <input className="bh-input w-56! font-mono" aria-label="Default model" value={value} placeholder="provider/model" onChange={(e) => setValue(e.target.value)} />
      <button className="bh-btn" onClick={() => void apply()}>Save</button>
    </div>
  );
}

function RlmDepth({ activeSessionId }: { activeSessionId?: string }) {
  const status = useAsync(() => (activeSessionId ? wire.call<{ maxDepth?: number; source?: string }>({ type: "get_rlm_max_depth_status", activeSessionId }) : Promise.resolve(undefined)), [activeSessionId]);
  const [depth, setDepth] = useState("");
  useEffect(() => setDepth(status.data?.maxDepth === undefined ? "" : String(status.data.maxDepth)), [status.data]);
  if (!activeSessionId) return <span className="text-xs text-dim">open a session</span>;
  const save = async (global: boolean) => {
    try {
      await wire.call({ type: "set_rlm_max_depth", activeSessionId, maxDepth: Number(depth), global });
      toast.success(global ? "RLM max depth saved as global default" : "RLM max depth set for this session");
      status.reload();
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  return (
    <div className="flex flex-wrap items-center gap-2">
      {status.data?.source && <Badge>{status.data.source}</Badge>}
      <input className="bh-input w-20!" type="number" min={0} aria-label="RLM max depth" value={depth} onChange={(e) => setDepth(e.target.value)} />
      <button className="bh-btn" onClick={() => void save(false)}>Session</button>
      <button className="bh-btn" onClick={() => void save(true)}>Global</button>
    </div>
  );
}

type Segment = { name: string; body: string; state: "default" | "modified" | "removed" | "added" };

function PromptSegments({ activeSessionId }: { activeSessionId: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button className="bh-btn" onClick={() => setOpen(true)}>Edit…</button>
      {open && <PromptSegmentsDialog activeSessionId={activeSessionId} onClose={() => setOpen(false)} />}
    </>
  );
}

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
function SessionModelRows({ activeSessionId }: { activeSessionId: string }) {
  const state = useAsync(() => wire.call<Record<string, any>>({ type: "get_state", activeSessionId }), [activeSessionId]);
  const levels: string[] = state.data?.availableThinkingLevels ?? ["off", "minimal", "low", "medium", "high", "max"];
  const apply = async (command: { type: string; [key: string]: unknown }) => {
    try {
      await wire.call({ ...command, activeSessionId });
      state.reload();
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  if (state.loading || !state.data) return null;
  const model = state.data.model;
  return (
    <>
      <li className="flex flex-wrap items-center gap-3 py-3">
        <div className="min-w-48 flex-1">
          <div className="text-sm font-medium">Session model</div>
          <p className="text-xs text-muted">{model ? `${model.provider}/${model.id}` : "unknown"} — change it from the chat model picker.</p>
        </div>
      </li>
      <li className="flex flex-wrap items-center gap-3 py-3">
        <div className="min-w-48 flex-1">
          <div className="text-sm font-medium">Session thinking level</div>
          <p className="text-xs text-muted">Current session's reasoning effort</p>
        </div>
        <select className="bh-input w-auto!" aria-label="Session thinking level" value={state.data.thinkingLevel ?? ""} onChange={(e) => void apply({ type: "set_thinking_level", level: e.target.value })}>
          {levels.map((l) => <option key={l}>{l}</option>)}
        </select>
      </li>
      <li className="flex flex-wrap items-center gap-3 py-3">
        <div className="min-w-48 flex-1">
          <div className="text-sm font-medium">Session service tier</div>
          <p className="text-xs text-muted">Priority (“fast”) tier for the current model when supported</p>
        </div>
        <select className="bh-input w-auto!" aria-label="Session service tier" defaultValue={state.data.serviceTier ?? "default"} onChange={(e) => void apply({ type: "set_service_tier", serviceTier: e.target.value === "default" ? null : e.target.value })}>
          {["default", "priority", "flex"].map((t) => <option key={t}>{t}</option>)}
        </select>
      </li>
    </>
  );
}

export { BackendUnavailable };
