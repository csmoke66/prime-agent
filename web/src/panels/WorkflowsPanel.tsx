import { useEffect, useState } from "react";
import { wire } from "../wire";
import { Modal } from "./ui/Modal";
import { Badge, ConfirmButton, Empty, ErrorLine, Spinner, Tabs, type Tone } from "./ui/bits";
import { fmtDuration } from "./ui/format";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";
import { buildInput, type FieldDef } from "./workflowInput";
import { isLive, parseFactoryRuns, type FactoryRun } from "./factory";

type Catalog = { names: string[]; definitions: Record<string, { description?: string; inputFields: Record<string, FieldDef> }>; errors?: Record<string, string> };

const RUN_TONE: Record<string, Tone> = { running: "success", paused: "warning", stopping: "warning", done: "info", failed: "error", stopped: "neutral" };

/** `/run` (workflow launcher) and the factory activity page for the session. */
export function WorkflowsPanel({ activeSessionId, initial, onClose }: { activeSessionId: string; initial: string; onClose: () => void }) {
  const [tab, setTab] = useState<"run" | "factory">("run");
  return (
    <Modal title="Workflows" size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <Tabs tabs={[{ id: "run", label: "Run workflow" }, { id: "factory", label: "Factory runs" }]} active={tab} onChange={setTab} />
        {tab === "run" ? <RunTab activeSessionId={activeSessionId} initial={initial.trim().split(/\s+/)[0] ?? ""} onDone={onClose} /> : <FactoryTab activeSessionId={activeSessionId} />}
      </div>
    </Modal>
  );
}

function RunTab({ activeSessionId, initial, onDone }: { activeSessionId: string; initial: string; onDone: () => void }) {
  const catalog = useAsync(() => wire.call<Catalog>({ type: "list_workflows", activeSessionId }), [activeSessionId]);
  const [name, setName] = useState(initial);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const def = name ? catalog.data?.definitions[name] : undefined;

  const run = async () => {
    if (!def) return;
    const built = buildInput(def.inputFields, texts);
    setErrors(built.errors);
    if (!built.input) return;
    setBusy(true);
    try {
      const receipt = await wire.call({ type: "run_workflow", activeSessionId, name, input: built.input, interrupt: false, continueTurn: false });
      toast.success(receipt?.deduplicated ? `Workflow ${name} is already queued or running` : `Workflow ${name} started`);
      onDone();
    } catch (reason) {
      setErrors([errorText(reason)]);
      setBusy(false);
    }
  };

  if (catalog.loading) return <Spinner label="Loading workflows" />;
  return (
    <div className="flex flex-col gap-3">
      <ErrorLine>{catalog.error}</ErrorLine>
      {!catalog.error && (catalog.data?.names.length ?? 0) === 0 && <Empty title="No workflows available" />}
      <div className="flex flex-wrap gap-2">
        {catalog.data?.names.filter((n) => catalog.data!.definitions[n]).map((n) => (
          <button key={n} className={`bh-btn ${n === name ? "border-primary bg-selected" : ""}`} onClick={() => { setName(n); setTexts({}); setErrors([]); }}>{n}</button>
        ))}
      </div>
      {Object.entries(catalog.data?.errors ?? {}).map(([n, e]) => <p key={n} className="text-xs text-warning">{n}: {e}</p>)}
      {def && (
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); void run(); }}>
          {def.description && <p className="text-sm text-muted">{def.description}</p>}
          {Object.entries(def.inputFields).map(([field, d]) => (
            <label key={field} className="flex flex-col gap-1 text-sm">
              <span>{field} <span className="text-xs text-dim">{d.type}{d.required ? " · required" : ""}</span></span>
              {d.description && <span className="text-xs text-muted">{d.description}</span>}
              {d.choices ? (
                <select className="bh-input" value={texts[field] ?? ""} onChange={(e) => setTexts({ ...texts, [field]: e.target.value })}>
                  <option value="">—</option>
                  {d.choices.map((c) => <option key={String(c)} value={JSON.stringify(c)}>{String(c)}</option>)}
                </select>
              ) : d.type === "boolean" ? (
                <select className="bh-input" value={texts[field] ?? ""} onChange={(e) => setTexts({ ...texts, [field]: e.target.value })}><option value="">—</option><option>true</option><option>false</option></select>
              ) : (
                <textarea className="bh-input min-h-10! font-mono text-xs" value={texts[field] ?? ""} onChange={(e) => setTexts({ ...texts, [field]: e.target.value })} />
              )}
            </label>
          ))}
          {errors.map((e) => <ErrorLine key={e}>{e}</ErrorLine>)}
          <button className="bh-btn bh-btn-primary self-end" disabled={busy}>{busy ? "Starting…" : `Run ${name}`}</button>
        </form>
      )}
    </div>
  );
}

function FactoryTab({ activeSessionId }: { activeSessionId: string }) {
  const [runs, setRuns] = useState<FactoryRun[]>();
  const [error, setError] = useState("");
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const tick = async () => {
      try {
        const data = await wire.call({ type: "factory_activity", activeSessionId, action: "graph" });
        if (live) {
          setRuns(parseFactoryRuns(data));
          setError("");
        }
      } catch (reason) {
        if (live) setError(errorText(reason));
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 2000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [activeSessionId]);

  const control = async (run: FactoryRun, action: "stop" | "resume") => {
    try {
      await wire.call({ type: "factory_activity", activeSessionId, action, runId: run.runId });
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  if (!runs && !error) return <Spinner label="Loading factory runs" />;
  return (
    <div className="flex flex-col gap-2">
      <ErrorLine>{error}</ErrorLine>
      {runs?.length === 0 && <Empty title="No factory runs" hint="Enable the factory in Settings › Agents." />}
      {runs?.map((run) => (
        <div key={run.runId || run.specId} className="bh-card flex flex-col gap-2 px-3 py-2.5">
          <button className="flex flex-wrap items-center gap-2 text-left" onClick={() => setOpen(open === run.runId ? null : run.runId)}>
            <span className="font-medium">{run.name}</span>
            <Badge tone={RUN_TONE[run.state] ?? "neutral"}>{run.state}</Badge>
            <span className="ml-auto text-xs text-dim">{fmtDuration(run.elapsedMs)} · {run.running} running · {run.settled} settled</span>
          </button>
          {isLive(run) && (
            <div className="flex gap-1">
              {run.state === "paused" && <button className="bh-btn" onClick={() => void control(run, "resume")}>Resume</button>}
              <ConfirmButton label="Stop run" confirmLabel="Stop run?" onConfirm={() => void control(run, "stop")} />
            </div>
          )}
          {open === run.runId && (
            <ul className="flex flex-col gap-1 text-sm">
              {run.states.map((state) => {
                const node = run.nodes[state.id];
                return (
                  <li key={state.id} className="flex flex-wrap items-center gap-2 rounded-lg bg-panel px-2 py-1">
                    <span className="font-mono text-xs">{state.id}{state.entry ? " ⟶" : ""}</span>
                    <Badge tone={node?.status === "running" ? "success" : node?.status === "failed" ? "error" : "neutral"}>{node?.status ?? "pending"}</Badge>
                    {node && node.running + node.queued > 0 && <span className="text-xs text-muted">{node.running} running · {node.queued} queued</span>}
                    <span className="ml-auto text-xs text-dim">→ {run.transitions.filter((t) => t.from.includes(state.id)).map((t) => t.to).join(", ") || "end"}</span>
                    {node?.error && <span className="basis-full text-xs text-error">{node.error}</span>}
                  </li>
                );
              })}
              {run.milestones.length > 0 && <li className="text-xs text-muted">Milestones: {run.milestones.join(" · ")}</li>}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
}
