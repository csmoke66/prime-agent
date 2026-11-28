import { useEffect, useState } from "react";
import { Modal } from "./ui/Modal";
import { ErrorLine, Spinner, Tabs, Toggle } from "./ui/bits";
import { BackendUnavailable, rest, type Settings } from "./api";
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
        await rest.saveSettings(next);
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
      return <SpecialRow id={row.id} settings={settings} onChange={onChange} />;
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

function SpecialRow({ id, settings, onChange }: { id: string; settings: Settings; onChange: (v: unknown) => void }) {
  switch (id) {
    case "default-model":
      return <DefaultModel settings={settings} />;
    default:
      void onChange;
      return null;
  }
}

function DefaultModel({ settings }: { settings: Settings }) {
  const stored = settings.defaultProvider && settings.defaultModel ? `${settings.defaultProvider}/${settings.defaultModel}` : "";
  const [value, setValue] = useState(stored);
  useEffect(() => setValue(stored), [stored]);
  const apply = async () => {
    const [provider, ...rest_] = value.split("/");
    const model = rest_.join("/");
    if (!provider || !model) return toast.error("Use provider/model");
    try {
      await rest.saveSettings({ ...settings, defaultProvider: provider, defaultModel: model });
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

export { BackendUnavailable };
