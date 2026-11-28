import { useEffect, useState } from "react";
import { Modal } from "./ui/Modal";
import { Badge, ErrorLine, Spinner, Tabs, Toggle } from "./ui/bits";
import { BackendUnavailable, rest, type GlobalModels, type Settings } from "./api";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";
import { SETTINGS_TABS, rowValue, setPath, type SettingRow } from "./settingsSchema";

const localGet = (path: string, fallback: unknown) => {
  const raw = localStorage.getItem(path);
  return raw === null ? fallback : JSON.parse(raw);
};

export function SettingsPanel({ onClose }: { onClose: () => void }) {
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
        // Send only the changed top-level key: the server merges per key, and other keys may have
        // been written since this window loaded (the model checklists save on their own).
        const key = row.path.split(".")[0];
        await rest.saveSettings({ [key]: next[key] ?? null });
        setDraft(next);
      }
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  return (
    <Modal title="Settings" subtitle="Global settings for every session. A session's own settings are under settings in the bar below its chat." size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <Tabs tabs={SETTINGS_TABS} active={tab} onChange={setTab} />
        {settings.loading && <Spinner label="Loading settings" />}
        {settings.error && (
          <ErrorLine>{settings.error.includes("does not provide") ? `${settings.error}. Settings stay read-only until it does.` : settings.error}</ErrorLine>
        )}
        <ErrorLine>{error}</ErrorLine>
        {!settings.loading && !settings.error && (
          <ul className="flex flex-col divide-y divide-grid/30">
            {current.rows.map((row) => (
              <li key={row.id} className="flex flex-wrap items-center gap-3 py-3">
                <div className="min-w-48 flex-1">
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {row.label}
                  </div>
                  <p className="text-xs text-muted">{row.description}</p>
                </div>
                <RowControl row={row} settings={draft} onChange={(value) => void change(row, value)} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </Modal>
  );
}

function RowControl({ row, settings, onChange }: { row: SettingRow; settings: Settings; onChange: (value: unknown) => void }) {
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
      return <SpecialRow id={row.id} settings={settings} />;
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

function SpecialRow({ id, settings }: { id: string; settings: Settings }) {
  switch (id) {
    case "default-model":
      return <DefaultModel settings={settings} />;
    case "allowed-models":
      return <AllowedModels />;
    default:
      return null;
  }
}

/** Pick the default model from the catalog (only globally allowed models). */
function DefaultModel({ settings }: { settings: Settings }) {
  const catalog = useAsync(() => rest.globalModels(), []);
  const [stored, setStored] = useState(settings.defaultProvider && settings.defaultModel ? `${settings.defaultProvider}/${settings.defaultModel}` : "");
  const choices = (catalog.data?.models ?? []).filter((m) => catalog.data?.unrestricted || m.allowed);
  const apply = async (selector: string) => {
    const slash = selector.indexOf("/");
    try {
      await rest.saveSettings({ defaultProvider: selector.slice(0, slash), defaultModel: selector.slice(slash + 1) });
      setStored(selector);
      toast.success(`Default model set to ${selector}`);
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  if (catalog.error) return <ErrorLine>{catalog.error}</ErrorLine>;
  if (!catalog.data) return <Spinner />;
  return (
    <select className="bh-input w-72! font-mono" aria-label="Default model" value={stored} onChange={(e) => void apply(e.target.value)}>
      {stored && !choices.some((m) => m.selector === stored) ? <option value={stored}>{stored}</option> : null}
      {choices.map((m) => <option key={m.selector} value={m.selector}>{m.selector}</option>)}
    </select>
  );
}

/** The global allowed-models list as a searchable checklist; nothing checked allows every model. */
function AllowedModels() {
  const [data, setData] = useState<GlobalModels>();
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState(false);
  const [error, setError] = useState("");
  const load = (next: GlobalModels) => {
    setData(next);
    setChecked(new Set(next.unrestricted ? [] : next.models.filter((m) => m.allowed).map((m) => m.selector)));
  };
  useEffect(() => { rest.globalModels().then(load, (e) => setError(errorText(e))); }, []);
  const save = async (models: string[]) => {
    try {
      load(await rest.setGlobalModels(models));
      setOpen(false);
      toast.success(models.length ? `${models.length} models allowed` : "All models allowed");
    } catch (reason) {
      setError(errorText(reason));
    }
  };
  if (error) return <ErrorLine>{error}</ErrorLine>;
  if (!data) return <Spinner />;
  const summary = data.unrestricted ? "All models" : `${data.models.filter((m) => m.allowed).length} allowed`;
  const shown = data.models.filter((m) => `${m.selector} ${m.name}`.toLowerCase().includes(filter.toLowerCase()));
  return (
    <div className="flex w-full flex-col items-end gap-2">
      <div className="flex items-center gap-2"><Badge>{summary}</Badge><button className="bh-btn" onClick={() => setOpen(!open)}>{open ? "Cancel" : "Edit…"}</button></div>
      {open ? (
        <div className="w-full rounded border border-line bg-surface p-2">
          <input className="bh-input mb-2" placeholder="Filter models…" value={filter} onChange={(e) => setFilter(e.target.value)} autoFocus />
          <ul className="max-h-72 overflow-y-auto text-[12px]">
            {shown.map((m) => (
              <li key={m.selector}>
                <label className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-selected">
                  <input type="checkbox" checked={checked.has(m.selector)} onChange={(e) => setChecked((prev) => { const next = new Set(prev); if (e.target.checked) next.add(m.selector); else next.delete(m.selector); return next; })} />
                  <span className="font-mono">{m.selector}</span>
                  <span className="truncate text-dim">{m.name}</span>
                </label>
              </li>
            ))}
          </ul>
          <div className="mt-2 flex justify-end gap-2">
            <button className="bh-btn" onClick={() => void save([])}>Allow all</button>
            <button className="bh-btn bh-btn-primary" disabled={checked.size === 0} onClick={() => void save([...checked])}>Save {checked.size} selected</button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

export { BackendUnavailable };
