import { useEffect, useState } from "react";
import { wire } from "../wire";
import { Modal } from "./ui/Modal";
import { Badge, ConfirmButton, Empty, ErrorLine, Spinner } from "./ui/bits";
import { oneLine, relativeTime } from "./ui/format";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";

type Job = {
  id: string; status: string; source?: string; deliveryMode?: string; activeSessionId?: string; sessionId?: string; label?: string;
  prompt?: string; schedule?: { expression?: string }; nextRunAt?: string; lastError?: string; runCount?: number;
};
type Row = { job: Job; sessionName?: string; firstMessage?: string };

/** `/heartbeats` and `/heartbeat`: every recurring job, with pause/resume/stop/edit and a creator. */
export function HeartbeatsPanel({ activeSessionId, onClose }: { activeSessionId?: string; onClose: () => void }) {
  const list = useAsync(() => wire.call<{ heartbeats?: Row[] }>({ type: "heartbeats_list" }), []);
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState("");
  useEffect(() => wire.onFrame((frame) => frame.type === "heartbeats_changed" && list.reload()), []); // eslint-disable-line react-hooks/exhaustive-deps

  const manage = async (row: Row, action: string, edit?: Record<string, unknown>) => {
    setError("");
    try {
      await wire.call({ type: "heartbeat_manage", activeSessionId: row.job.activeSessionId, jobId: row.job.id, action, ...(edit ? { edit } : {}) });
      setEditing(null);
      list.reload();
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  const rows = list.data?.heartbeats ?? [];
  return (
    <Modal title="Heartbeats" subtitle="Recurring prompts that wake sessions." size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        {activeSessionId && <Creator activeSessionId={activeSessionId} onDone={list.reload} />}
        <ErrorLine>{list.error || error}</ErrorLine>
        {list.loading && !list.data && <Spinner />}
        {!list.loading && rows.length === 0 && <Empty title="No heartbeats" hint="Set one for this session above." />}
        {rows.map((row) => {
          const { job } = row;
          const active = job.status === "active";
          return (
            <div key={job.id} className="bh-card flex flex-col gap-2 px-3 py-2.5">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={active ? "success" : "warning"}>{job.status}</Badge>
                <span className="font-mono text-xs text-muted">{job.schedule?.expression}</span>
                <Badge>{job.source === "rlm_heartbeat" ? "agent" : "user"}</Badge>
                <Badge>{job.deliveryMode ?? "steer"}</Badge>
                <span className="ml-auto text-xs text-dim">{job.runCount ?? 0} runs{job.nextRunAt ? ` · next ${relativeTime(job.nextRunAt).replace(" ago", "") || job.nextRunAt}` : ""}</span>
              </div>
              <p className="text-sm">{oneLine(job.prompt, 200)}</p>
              <p className="text-xs text-dim">{row.sessionName ?? oneLine(row.firstMessage, 60)}</p>
              {job.lastError && <p className="text-xs text-error">{job.lastError}</p>}
              {editing === job.id ? (
                <EditForm job={job} onCancel={() => setEditing(null)} onSave={(edit) => void manage(row, "edit", edit)} />
              ) : (
                <div className="flex flex-wrap gap-1">
                  <button className="bh-btn" onClick={() => void manage(row, active ? "pause" : "resume")}>{active ? "Pause" : "Resume"}</button>
                  <button className="bh-btn" onClick={() => setEditing(job.id)}>Edit</button>
                  <ConfirmButton label="Stop" confirmLabel="Stop job?" onConfirm={() => void manage(row, "stop")} />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </Modal>
  );
}

function Creator({ activeSessionId, onDone }: { activeSessionId: string; onDone: () => void }) {
  const [schedule, setSchedule] = useState("every 10m");
  const [prompt, setPrompt] = useState("");
  const [mode, setMode] = useState("steer");
  const submit = async () => {
    try {
      await wire.call({ type: "heartbeat_set", activeSessionId, schedule, prompt, deliveryMode: mode });
      setPrompt("");
      toast.success("Heartbeat set");
      onDone();
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };
  return (
    <form className="bh-card flex flex-wrap items-end gap-2 p-3" onSubmit={(e) => { e.preventDefault(); if (prompt.trim()) void submit(); }}>
      <label className="flex flex-col gap-1 text-xs text-muted">Schedule<input className="bh-input w-36!" value={schedule} onChange={(e) => setSchedule(e.target.value)} /></label>
      <label className="flex min-w-48 flex-1 flex-col gap-1 text-xs text-muted">Instruction<input className="bh-input" value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="What should the agent do each time?" /></label>
      <label className="flex flex-col gap-1 text-xs text-muted">Delivery<select className="bh-input" value={mode} onChange={(e) => setMode(e.target.value)}><option value="steer">steer</option><option value="follow_up">follow-up</option></select></label>
      <button className="bh-btn bh-btn-primary" disabled={!prompt.trim()}>Set for this session</button>
    </form>
  );
}

function EditForm({ job, onSave, onCancel }: { job: Job; onSave: (edit: Record<string, unknown>) => void; onCancel: () => void }) {
  const [schedule, setSchedule] = useState(job.schedule?.expression ?? "");
  const [prompt, setPrompt] = useState(job.prompt ?? "");
  const [label, setLabel] = useState(job.label ?? "");
  const [deliveryMode, setDeliveryMode] = useState(job.deliveryMode ?? "steer");
  return (
    <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); onSave({ schedule, prompt, label, deliveryMode }); }}>
      <input className="bh-input" aria-label="Schedule" value={schedule} onChange={(e) => setSchedule(e.target.value)} />
      <textarea className="bh-input min-h-16!" aria-label="Instruction" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
      <div className="flex flex-wrap gap-2">
        <input className="bh-input w-48!" aria-label="Label" placeholder="Label" value={label} onChange={(e) => setLabel(e.target.value)} />
        <select className="bh-input w-auto!" aria-label="Delivery" value={deliveryMode} onChange={(e) => setDeliveryMode(e.target.value)}><option value="steer">steer</option><option value="follow_up">follow-up</option></select>
        <button className="bh-btn bh-btn-primary" type="submit">Save</button>
        <button className="bh-btn" type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}
