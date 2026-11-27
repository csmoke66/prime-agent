import { useEffect, useState } from "react";
import { wire } from "../wire";
import { navigate } from "../app/router";
import { Modal } from "../panels/ui/Modal";
import { Badge, ConfirmButton, ErrorLine, Spinner } from "../panels/ui/bits";
import { fmtCost, fmtDuration, relativeTime } from "../panels/ui/format";
import { openPanel } from "../panels/store";
import { toast, errorText } from "../panels/toasts";
import { createSession } from "../panels/sessionActions";
import { activityLabel, modelLabel, sessionTitle, type RosterEntry } from "./roster";
import { destructiveKind, destructiveLabel, openEntry, runDestructive } from "./useAgentActions";
import { KernelBash } from "./Shells";

/** Detail drawer: reply, inter-agent messaging controls, kernel shells, heartbeats and factory shortcuts. */
export function AgentDetail({ entry, onClose }: { entry: RosterEntry; onClose: () => void }) {
  const s = entry.summary;
  const active: string | undefined = s.activeSessionId;
  const [reply, setReply] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [messaging, setMessaging] = useState<{ paused?: boolean; maxPendingPerSession?: number } | null>(null);

  useEffect(() => {
    if (!active) return;
    wire.call({ type: "agent_messages_status", activeSessionId: active }).then(setMessaging, () => undefined);
  }, [active]);

  const messagingAction = async (type: "agent_messages_pause" | "agent_messages_resume" | "agent_messages_clear") => {
    try {
      const data = await wire.call({ type, activeSessionId: active });
      if (type === "agent_messages_clear") toast.info("Queued agent messages cleared");
      else setMessaging(data);
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };

  const sendReply = async (kind: "prompt" | "message") => {
    if (!reply.trim()) return;
    setBusy(true);
    setError("");
    try {
      let target = active;
      if (!target) {
        target = await createSession({ cwd: s.cwd ?? "/", sessionPath: s.sessionFile ?? s.path });
      }
      if (kind === "message") await wire.call({ type: "send_message", targetActiveSessionId: target, message: reply });
      else await wire.call({ type: "prompt", activeSessionId: target, message: reply, input: { streamingBehavior: entry.status === "running" ? "steer" : undefined } });
      toast.success("Reply sent");
      setReply("");
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const destructive = destructiveLabel(entry);
  return (
    <Modal
      title={sessionTitle(entry)}
      subtitle={`${entry.status} · ${activityLabel(entry)}${s.cwd ? ` · ${s.cwd}` : ""}`}
      variant="drawer"
      size="md"
      onClose={onClose}
      actions={<button className="bh-btn bh-btn-primary" onClick={() => void openEntry(entry, (id) => { navigate({ name: "session", activeSessionId: id }); onClose(); })}>Open</button>}
    >
      <div className="flex flex-col gap-4">
        <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
          {([
            ["Model", modelLabel(s)],
            ["Thinking", s.thinkingLevel],
            ["Messages", s.messageCount],
            ["Cost", fmtCost(s.cost ?? s.usage?.cost)],
            ["Created", relativeTime(s.created)],
            ["Last active", relativeTime(s.lastActivityAt ?? s.modified)],
            ["Heard from", relativeTime(entry.lastHeardFromAt)],
            ["Depth", s.rlmDepth],
          ] as [string, unknown][]).filter(([, v]) => v !== undefined && v !== "" && v !== null).map(([k, v]) => (
            <div key={k} className="contents"><dt className="text-muted">{k}</dt><dd className="truncate">{String(v)}</dd></div>
          ))}
        </dl>
        {s.progressNote && <p className="rounded-lg bg-panel px-3 py-2 text-sm text-muted">{String(s.progressNote)}</p>}

        <section className="flex flex-col gap-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-dim">Reply</h3>
          <textarea className="bh-input min-h-20!" value={reply} onChange={(e) => setReply(e.target.value)} placeholder={active ? "Send a prompt to this agent…" : "Resumes the session, then sends…"} aria-label="Reply" />
          <ErrorLine>{error}</ErrorLine>
          <div className="flex flex-wrap gap-2">
            <button className="bh-btn bh-btn-primary" disabled={busy || !reply.trim()} onClick={() => void sendReply("prompt")}>{busy ? "Sending…" : entry.status === "running" ? "Steer" : "Send"}</button>
            {active && <button className="bh-btn" disabled={busy || !reply.trim()} onClick={() => void sendReply("message")} title="Delivers as an agent-to-agent message">Send as agent message</button>}
          </div>
        </section>

        {active && (
          <section className="flex flex-col gap-2">
            <h3 className="text-xs font-medium uppercase tracking-wider text-dim">Agent messages</h3>
            <div className="flex flex-wrap items-center gap-2">
              {messaging ? <Badge tone={messaging.paused ? "warning" : "success"}>{messaging.paused ? "paused" : "accepting"}</Badge> : <Spinner />}
              <button className="bh-btn" onClick={() => void messagingAction(messaging?.paused ? "agent_messages_resume" : "agent_messages_pause")}>{messaging?.paused ? "Resume" : "Pause"}</button>
              <ConfirmButton label="Clear queued" confirmLabel="Clear?" onConfirm={() => void messagingAction("agent_messages_clear")} />
            </div>
          </section>
        )}

        {active && <KernelBash activeSessionId={active} />}

        <section className="flex flex-wrap gap-2 border-t border-grid/40 pt-3">
          {active && <button className="bh-btn" onClick={() => { openPanel("workflows", { activeSessionId: active }); onClose(); }}>Workflows / factory</button>}
          {active && <button className="bh-btn" onClick={() => { openPanel("heartbeats", { activeSessionId: active }); onClose(); }}>Heartbeats</button>}
          {destructive && destructiveKind(entry) && (
            <ConfirmButton className="ml-auto" label={destructive} confirmLabel={`${destructive}?`} onConfirm={() => void runDestructive(entry).then(onClose)} />
          )}
        </section>
        {s.firstMessage && <p className="text-xs text-dim">First message: {String(s.firstMessage).slice(0, 300)}</p>}
        {s.durationMs && <p className="text-xs text-dim">Running for {fmtDuration(s.durationMs)}</p>}
      </div>
    </Modal>
  );
}
