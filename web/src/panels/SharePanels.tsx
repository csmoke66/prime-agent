import { useRef, useState } from "react";
import { wire } from "../wire";
import { navigate } from "../app/router";
import { Modal } from "./ui/Modal";
import { ErrorLine, Spinner } from "./ui/bits";
import { rest } from "./api";
import { createSession } from "./sessionActions";
import { toast, errorText } from "./toasts";

/** Have the daemon write the session into the export dir; returns the file name to download or share. */
async function exportSession(activeSessionId: string, format: "html" | "jsonl"): Promise<string> {
  const { exportDir } = await rest.info();
  const name = `session-${Date.now()}.${format}`;
  await wire.call({ type: format === "html" ? "export_html" : "export_jsonl", activeSessionId, outputPath: `${exportDir}/${name}` }, 120_000);
  return name;
}

/** `/export`: download the session as HTML or JSONL. */
export function ExportPanel({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const download = async (format: "html" | "jsonl") => {
    setBusy(true);
    setError("");
    try {
      const link = document.createElement("a");
      link.href = rest.downloadUrl(await exportSession(activeSessionId, format));
      link.download = "";
      link.click();
      onClose();
    } catch (reason) {
      setError(errorText(reason));
      setBusy(false);
    }
  };
  return (
    <Modal title="Export session" subtitle="Downloads a copy to this browser." size="sm" onClose={onClose}>
      <div className="flex flex-col gap-2">
        <button className="bh-btn justify-center" disabled={busy} onClick={() => void download("html")}>HTML (readable transcript)</button>
        <button className="bh-btn justify-center" disabled={busy} onClick={() => void download("jsonl")}>JSONL (re-importable)</button>
        <ErrorLine>{error}</ErrorLine>
      </div>
    </Modal>
  );
}

/** `/import`: upload a session JSONL and resume it. */
export function ImportPanel({ onClose }: { onClose: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [drag, setDrag] = useState(false);

  const importFile = async (file: File) => {
    setBusy(true);
    setError("");
    try {
      const { path } = await rest.upload(file);
      const info = await rest.info().catch(() => undefined);
      const id = await createSession({ cwd: info?.cwd ?? "/" });
      await wire.call({ type: "import_jsonl", activeSessionId: id, inputPath: path }, 120_000);
      toast.success("Session imported");
      navigate({ name: "session", activeSessionId: id });
      onClose();
    } catch (reason) {
      setError(errorText(reason));
      setBusy(false);
    }
  };

  return (
    <Modal title="Import session" subtitle="Choose a session .jsonl file to import and resume." size="sm" onClose={onClose}>
      <div
        className={`grid place-items-center gap-2 rounded-xl border-2 border-dashed px-4 py-10 text-center transition-colors ${drag ? "border-primary bg-selected" : "border-grid/60"}`}
        onDragOver={(e) => { e.preventDefault(); setDrag(true); }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => { e.preventDefault(); setDrag(false); const file = e.dataTransfer.files[0]; if (file) void importFile(file); }}
      >
        {busy ? <Spinner label="Importing" /> : (
          <>
            <p className="text-sm text-muted">Drop a .jsonl file here</p>
            <button className="bh-btn" onClick={() => input.current?.click()}>Choose file…</button>
          </>
        )}
        <input ref={input} type="file" accept=".jsonl,application/jsonl,application/json" hidden onChange={(e) => { const file = e.target.files?.[0]; if (file) void importFile(file); }} />
      </div>
      <div className="mt-3"><ErrorLine>{error}</ErrorLine></div>
    </Modal>
  );
}

/** `/share`: publish a secret GitHub gist (needs `gh` logged in on the host). */
export function SharePanel({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const [state, setState] = useState<{ busy: boolean; error: string; result?: { previewUrl: string; gistUrl: string } }>({ busy: false, error: "" });
  const start = async () => {
    setState({ busy: true, error: "" });
    try {
      setState({ busy: false, error: "", result: await rest.share(await exportSession(activeSessionId, "html")) });
    } catch (reason) {
      setState({ busy: false, error: errorText(reason) });
    }
  };
  return (
    <Modal title="Share session" subtitle="Creates a secret GitHub gist with the HTML transcript." size="sm" onClose={onClose} footer={!state.result && <button className="bh-btn bh-btn-primary" disabled={state.busy} onClick={() => void start()}>{state.busy ? "Creating gist…" : "Create gist"}</button>}>
      <ErrorLine>{state.error}</ErrorLine>
      {state.result && (
        <div className="flex flex-col gap-2 text-sm">
          <a className="text-info underline" href={state.result.previewUrl} target="_blank" rel="noreferrer">{state.result.previewUrl}</a>
          <a className="text-muted underline" href={state.result.gistUrl} target="_blank" rel="noreferrer">{state.result.gistUrl}</a>
          <button className="bh-btn self-start" onClick={() => void navigator.clipboard.writeText(state.result!.previewUrl).then(() => toast.success("Link copied"))}>Copy link</button>
        </div>
      )}
    </Modal>
  );
}
