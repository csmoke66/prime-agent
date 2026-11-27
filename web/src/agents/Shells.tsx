import { useCallback, useEffect, useState } from "react";
import { wire } from "../wire";
import { Badge, ConfirmButton, Empty, ErrorLine, Spinner } from "../panels/ui/bits";
import { fmtDuration, oneLine } from "../panels/ui/format";
import { toast, errorText } from "../panels/toasts";

export type Activity = { id: string; command: string; pid?: number; status: string; exitCode?: number; durationMs?: number; persistent?: boolean; sessionId?: string };

/** Mirrors `parse_bash_activities`: rows with an id, running ones first. */
export function parseActivities(data: any): Activity[] {
  const rows = Array.isArray(data) ? data : data?.activities ?? data?.shells ?? [];
  return (rows as any[])
    .filter((row) => typeof (row?.id ?? row?.shellId) === "string" && (row.id ?? row.shellId).trim() !== "")
    .map((row) => ({
      id: String(row.id ?? row.shellId),
      command: String(row.command ?? ""),
      pid: row.pid,
      status: String(row.status ?? "unknown"),
      exitCode: row.exitCode,
      durationMs: row.durationMs,
      persistent: row.persistent === true,
      sessionId: row.sessionId,
    }))
    .sort((a, b) => Number(b.status === "running") - Number(a.status === "running"));
}

type Source = { list: () => Promise<any>; tail: (id: string) => Promise<any>; kill: (id: string) => Promise<any> };

/** Shared list / tail / kill view for kernel bash activities and machine-wide persistent shells. */
export function ShellList({ source, empty }: { source: Source; empty: string }) {
  const [rows, setRows] = useState<Activity[]>();
  const [error, setError] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [output, setOutput] = useState("");

  const refresh = useCallback(() => source.list().then((d) => (setRows(parseActivities(d)), setError("")), (e) => setError(errorText(e))), [source]);
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);
  useEffect(() => {
    if (!open) return;
    let live = true;
    const load = () => source.tail(open).then((d) => live && setOutput(String(d?.tail ?? d?.output ?? "")), (e) => live && setOutput(errorText(e)));
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [open, source]);

  if (!rows && !error) return <Spinner />;
  return (
    <div className="flex flex-col gap-2">
      <ErrorLine>{error}</ErrorLine>
      {rows?.length === 0 && <Empty title={empty} />}
      {rows?.map((row) => (
        <div key={row.id} className="bh-card flex flex-col gap-1 px-2 py-1.5">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <button className="w-full truncate text-left font-mono text-[11px]" onClick={() => setOpen(open === row.id ? null : row.id)} title={row.command}>{oneLine(row.command, 90) || row.id}</button>
            <Badge tone={row.status === "running" ? "success" : "neutral"}>{row.status}{row.exitCode !== undefined && row.status !== "running" ? ` (${row.exitCode})` : ""}</Badge>
            {row.persistent && <Badge tone="primary">persistent</Badge>}
            <span className="text-xs text-dim">{fmtDuration(row.durationMs)}</span>
            {row.status === "running" && (
              <ConfirmButton label="Kill" confirmLabel="Kill?" onConfirm={() => void source.kill(row.id).then(() => (toast.info("Kill requested"), refresh()), (e) => toast.error(errorText(e)))} />
            )}
          </div>
          {open === row.id && <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-panel p-2 text-xs">{output || "(no output yet)"}</pre>}
        </div>
      ))}
    </div>
  );
}

export function KernelBash({ activeSessionId }: { activeSessionId: string }) {
  const [source] = useState<Source>(() => ({
    list: () => wire.call({ type: "list_kernel_bash", activeSessionId }),
    tail: (activityId) => wire.call({ type: "tail_kernel_bash", activeSessionId, activityId, lines: 200 }),
    kill: (activityId) => wire.call({ type: "kill_kernel_bash", activeSessionId, activityId }),
  }));
  return (
    <ShellList source={source} empty="No bash activity" />
  );
}

const persistentSource: Source = {
  list: () => wire.call({ type: "list_persistent_shells" }),
  tail: (shellId) => wire.call({ type: "tail_persistent_shell", shellId, lines: 200 }),
  kill: (shellId) => wire.call({ type: "kill_persistent_shell", shellId }),
};

export function PersistentShells() {
  return <ShellList source={persistentSource} empty="No persistent shells" />;
}
