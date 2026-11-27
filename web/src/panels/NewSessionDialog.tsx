import { useEffect, useState } from "react";
import { Modal } from "./ui/Modal";
import { ErrorLine } from "./ui/bits";
import { rest } from "./api";
import { startNewSession } from "./sessionActions";
import { errorText } from "./toasts";

const RECENT_KEY = "bh.recentCwds";
const THINKING = ["", "off", "minimal", "low", "medium", "high", "max"];

function recentCwds(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
  } catch {
    return [];
  }
}

/** `/new [--name "n" --] [prompt]` argument form of the TUI. */
export function parseNewArgs(args: string): { name: string; prompt: string } {
  const match = args.match(/^\s*--name\s+(?:"([^"]*)"|(\S+))\s*(?:--\s*)?([\s\S]*)$/);
  if (!match) return { name: "", prompt: args.trim() };
  return { name: match[1] ?? match[2] ?? "", prompt: match[3].trim() };
}

export function NewSessionDialog({ args, onClose }: { args: string; onClose: () => void }) {
  const initial = parseNewArgs(args);
  const [cwd, setCwd] = useState("");
  const [name, setName] = useState(initial.name);
  const [prompt, setPrompt] = useState(initial.prompt);
  const [model, setModel] = useState("");
  const [thinking, setThinking] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [suggestions, setSuggestions] = useState<string[]>(recentCwds());

  useEffect(() => {
    rest.info().then((info) => setCwd((current) => current || info.cwd), () => undefined);
  }, []);

  useEffect(() => {
    if (!cwd.includes("/")) return;
    const slash = cwd.lastIndexOf("/");
    const base = cwd.slice(0, slash + 1) || "/";
    const timer = window.setTimeout(() => {
      rest.files(base, cwd.slice(slash + 1)).then(
        (result) => {
          setSuggestions([...result.entries.filter((e) => e.isDir).map((e) => e.path.replace(/\/$/, "")).slice(0, 8), ...recentCwds()]);
        },
        () => undefined,
      );
    }, 200);
    return () => window.clearTimeout(timer);
  }, [cwd]);

  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const [provider, ...rest_] = model.includes("/") ? model.split("/") : ["", model];
      localStorage.setItem(RECENT_KEY, JSON.stringify([cwd, ...recentCwds().filter((c) => c !== cwd)].slice(0, 8)));
      await startNewSession({ cwd, name, prompt, provider: provider || undefined, model: rest_.join("/") || undefined, thinking: thinking || undefined });
      onClose();
    } catch (reason) {
      setError(errorText(reason));
      setBusy(false);
    }
  };

  return (
    <Modal
      title="New session"
      subtitle="Starts a fresh agent in the chosen directory."
      onClose={onClose}
      footer={
        <>
          <button className="bh-btn" onClick={onClose}>Cancel</button>
          <button className="bh-btn bh-btn-primary" disabled={busy || !cwd.trim()} onClick={() => void submit()}>
            {busy ? "Starting…" : prompt.trim() ? "Start and send" : "Start"}
          </button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && cwd.trim()) void submit();
        }}
      >
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted">Working directory</span>
          <input className="bh-input font-mono" list="bh-cwds" value={cwd} onChange={(e) => setCwd(e.target.value)} data-autofocus spellCheck={false} />
          <datalist id="bh-cwds">{suggestions.map((s) => <option key={s} value={s} />)}</datalist>
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted">Name <span className="text-dim">(optional)</span></span>
          <input className="bh-input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted">Initial prompt <span className="text-dim">(optional)</span></span>
          <textarea className="bh-input min-h-24! resize-y" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
        </label>
        <div className="grid gap-3 sm:grid-cols-2">
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-muted">Model <span className="text-dim">(provider/id, default if empty)</span></span>
            <input className="bh-input font-mono" value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-muted">Thinking level</span>
            <select className="bh-input" value={thinking} onChange={(e) => setThinking(e.target.value)}>
              {THINKING.map((level) => <option key={level} value={level}>{level || "default"}</option>)}
            </select>
          </label>
        </div>
        <ErrorLine>{error}</ErrorLine>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
