import { useState } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { Modal } from "./ui/Modal";
import { ErrorLine, Spinner } from "./ui/bits";
import { fmtBytes } from "./ui/format";
import { rest } from "./api";
import { useAsync } from "./useAsync";
import { HOTKEYS } from "./hotkeys";

export function HotkeysPanel({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="Keyboard shortcuts" size="lg" onClose={onClose}>
      <div className="grid gap-5 sm:grid-cols-2">
        {HOTKEYS.map((group) => (
          <section key={group.title}>
            <h3 className="mb-2 text-xs font-medium uppercase tracking-wider text-dim">{group.title}</h3>
            <dl className="flex flex-col gap-1.5">
              {group.keys.map((k) => (
                <div key={k.keys + k.action} className="flex items-baseline justify-between gap-3 text-sm">
                  <dt className="text-muted">{k.action}</dt>
                  <dd className="shrink-0"><kbd className="bh-kbd">{k.keys}</kbd></dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Modal>
  );
}

export function ChangelogPanel({ onClose }: { onClose: () => void }) {
  const log = useAsync(() => rest.changelog(), []);
  const html = log.data ? DOMPurify.sanitize(marked.parse(log.data.markdown, { async: false }) as string) : "";
  return (
    <Modal title="What's new" size="lg" onClose={onClose}>
      <ErrorLine>{log.error}</ErrorLine>
      {log.loading && <Spinner />}
      <div className="bh-prose text-sm leading-relaxed [&_a]:text-info [&_h2]:mt-4 [&_h2]:text-base [&_h2]:font-semibold [&_li]:ml-5 [&_li]:list-disc [&_p]:my-1.5" dangerouslySetInnerHTML={{ __html: html }} />
    </Modal>
  );
}

export function LogsPanel({ onClose }: { onClose: () => void }) {
  const logs = useAsync(() => rest.logs(), []);
  const [open, setOpen] = useState<string | null>(null);
  const tail = useAsync(() => (open ? rest.logTail(open, 300) : Promise.resolve("")), [open]);
  return (
    <Modal title="Logs" subtitle={logs.data?.dir} size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <ErrorLine>{logs.error}</ErrorLine>
        {logs.loading && <Spinner />}
        {logs.data && logs.data.files.length === 0 && <p className="text-sm text-muted">No logs written yet.</p>}
        <ul className="flex flex-col gap-1">
          {logs.data?.files.map((file) => (
            <li key={file.name}>
              <button className={`flex w-full justify-between rounded-lg px-2 py-1.5 text-sm hover:bg-selected/60 ${open === file.name ? "bg-selected" : ""}`} onClick={() => setOpen(open === file.name ? null : file.name)}>
                <span className="font-mono">{file.name}</span>
                <span className="text-xs text-dim">{fmtBytes(file.size)}</span>
              </button>
            </li>
          ))}
        </ul>
        {open && (tail.loading ? <Spinner /> : <pre className="max-h-[50vh] overflow-auto whitespace-pre-wrap break-all rounded-lg bg-panel p-3 text-xs">{tail.error || tail.data}</pre>)}
        <p className="text-xs text-dim">Daemon crashes log to &lt;socket&gt;.log; agent-open failures log to client-errors.log.</p>
      </div>
    </Modal>
  );
}
