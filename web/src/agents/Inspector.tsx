import { useEffect, useMemo, useState, type ReactNode } from "react";
import { wire } from "../wire";
import { navigate } from "../app/router";
import { Badge, ConfirmButton, ErrorLine, Spinner } from "../panels/ui/bits";
import { fmtCost, fmtDuration, relativeTime } from "../panels/ui/format";
import { openPanel } from "../panels/store";
import { toast, errorText } from "../panels/toasts";
import { formatCount, contextPercent, formatTokens, queueOf } from "../chat/model";
import { useChat } from "../chat/store";
import { retainRoster, useRoster } from "./rosterStore";
import { activityLabel, buildForest, flatten, modelLabel, sessionTitle, type AgentNode, type RosterEntry } from "./roster";
import { destructiveKind, destructiveLabel, openEntry, runDestructive } from "./useAgentActions";
import { KernelBash } from "./Shells";
import { StatusGlyph } from "../app/StatusGlyph";

const Section = ({ title, children, right }: { title: string; children: ReactNode; right?: ReactNode }) => (
  <section className="border-b border-line px-3 py-2.5">
    <h3 className="bh-section-title mb-1.5 flex items-center justify-between">{title}{right}</h3>
    {children}
  </section>
);

const STATUS_COLOR = { running: "text-success", idle: "text-success", inactive: "text-dim" } as const;

/** Right-hand inspector for the open session: context, goal, queue, subagents, messaging, shells, reply. */
export function Inspector({ activeSessionId }: { activeSessionId: string }) {
  const roster = useRoster((s) => s.roster);
  const state = useChat((s) => s.data.state);
  const entry = roster.find((e) => e.summary.activeSessionId === activeSessionId);
  const node = useMemo(() => {
    const forest = buildForest(roster);
    const find = (list: AgentNode[]): AgentNode | undefined => {
      for (const n of list) {
        if (n.entry.agentId === entry?.agentId) return n;
        const hit = find(n.children);
        if (hit) return hit;
      }
    };
    return find(forest);
  }, [roster, entry?.agentId]);
  useEffect(() => retainRoster(), []);
  const [reply, setReply] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [messaging, setMessaging] = useState<{ paused?: boolean } | null>(null);

  useEffect(() => {
    setMessaging(null);
    wire.call({ type: "agent_messages_status", activeSessionId }).then(setMessaging, () => undefined);
  }, [activeSessionId]);

  const messagingAction = async (type: "agent_messages_pause" | "agent_messages_resume" | "agent_messages_clear") => {
    try {
      const data = await wire.call({ type, activeSessionId });
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
      if (kind === "message") await wire.call({ type: "send_message", targetActiveSessionId: activeSessionId, message: reply });
      else await wire.call({ type: "prompt", activeSessionId, message: reply, input: { streamingBehavior: state.isStreaming ? "steer" : undefined } });
      toast.success("Reply sent");
      setReply("");
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const pct = contextPercent(state);
  const usage = state.contextUsage;
  const queue = queueOf(state);
  const goal = state.goal;
  const s = entry?.summary ?? {};
  const kids = node ? flatten(node.children, new Set()) : [];
  const destructive = entry ? destructiveLabel(entry) : "";

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-surface text-[12px]" aria-label="Inspector">
      <Section title="Session">
        <div className="truncate text-[13px] font-medium">{entry ? sessionTitle(entry) : state.sessionName ?? "Session"}</div>
        <div className="mb-1.5 truncate font-mono text-[11px] text-dim" title={s.cwd}>{s.cwd}</div>
        <dl className="grid grid-cols-[5.5rem_1fr] gap-y-0.5">
          {([
            ["Status", entry ? <span className={STATUS_COLOR[entry.status]}>{activityLabel(entry)}</span> : state.isStreaming ? "working" : "idle"],
            ["Model", entry ? modelLabel(s) : undefined],
            ["Thinking", state.thinkingLevel ?? s.thinkingLevel],
            ["Messages", s.messageCount],
            ["Cost", fmtCost(s.cost ?? s.usage?.cost)],
            ["Created", relativeTime(s.created)],
            ["Last active", relativeTime(s.lastActivityAt ?? s.modified)],
            ["Depth", s.rlmDepth],
            ["Running for", s.durationMs ? fmtDuration(s.durationMs) : ""],
          ] as [string, ReactNode][]).filter(([, v]) => v !== undefined && v !== "" && v !== null).map(([k, v]) => (
            <div key={k} className="contents"><dt className="text-dim">{k}</dt><dd className="truncate">{v}</dd></div>
          ))}
        </dl>
        {s.progressNote ? <p className="mt-1.5 rounded bg-panel px-2 py-1 text-muted">{String(s.progressNote)}</p> : null}
      </Section>

      <Section title="Context" right={<button className="bh-ghost" title="Edit the raw model-visible context (/edit_context)" onClick={() => openPanel("edit_context", { activeSessionId })}>Edit</button>}>
        <div className="h-1.5 overflow-hidden rounded-full bg-panel" role="meter" aria-valuenow={pct ?? 0} aria-valuemin={0} aria-valuemax={100} aria-label="Context used">
          <div className={`h-full ${pct !== null && pct > 85 ? "bg-error" : pct !== null && pct > 65 ? "bg-warning" : "bg-primary"}`} style={{ width: `${Math.min(100, pct ?? 0)}%` }} />
        </div>
        <div className="mt-1 flex justify-between text-dim"><span>{pct === null ? "unknown until the next reply" : `${pct.toFixed(0)}% used`}</span>{usage?.contextWindow ? <span>{usage.tokens == null ? "?" : formatTokens(usage.tokens)} / {formatTokens(usage.contextWindow)}</span> : null}</div>
      </Section>

      {goal && (goal.objective || goal.active) ? (
        <Section title="Goal" right={<Badge tone={goal.status === "active" ? "success" : goal.status === "paused" ? "warning" : "neutral"}>{String(goal.status).replace("_", " ")}</Badge>}>
          <p className="text-muted">{goal.objective}</p>
          <div className="mt-1 text-dim">{formatCount(goal.tokensUsed ?? 0)}{goal.tokenBudget ? ` / ${formatCount(goal.tokenBudget)}` : ""} tokens</div>
        </Section>
      ) : null}

      <Section title="Queue" right={<span className="font-normal normal-case tracking-normal">{queue.steering.length + queue.followUps.length}</span>}>
        {queue.steering.length + queue.followUps.length === 0 ? <p className="text-dim">Nothing queued</p> : (
          <ul className="space-y-0.5">
            {queue.steering.map((q, i) => <li key={`s${i}`} className="truncate"><span className="text-warning">steer</span> {q}</li>)}
            {queue.followUps.map((q, i) => <li key={`f${i}`} className="truncate"><span className="text-info">next</span> {q}</li>)}
          </ul>
        )}
      </Section>

      {kids.length ? (
        <Section title="Subagents" right={<span className="font-normal normal-case tracking-normal">{kids.length}</span>}>
          <ul className="space-y-0.5" data-activity-focus>
            {kids.map((k) => (
              <li key={k.entry.agentId} style={{ paddingLeft: (k.depth - (node?.depth ?? 0) - 1) * 10 }}>
                <button className="flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left hover:bg-selected" onClick={() => void openEntry(k.entry, (id) => navigate({ name: "session", activeSessionId: id }))}>
                  <StatusGlyph status={k.entry.status} />
                  <span className="min-w-0 flex-1 truncate">{sessionTitle(k.entry)}</span>
                  <span className="shrink-0 text-dim">{activityLabel(k.entry)}</span>
                </button>
                {typeof k.entry.summary.spawnCode === "string" && k.entry.summary.spawnCode.trim() ? (
                  <details className="pl-4 text-dim">
                    <summary className="cursor-pointer text-[11px]">Spawn program</summary>
                    <pre className="max-h-48 overflow-auto whitespace-pre-wrap text-[11px]">{k.entry.summary.spawnCode}</pre>
                  </details>
                ) : null}
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      <Section title="Agent messages" right={messaging ? <Badge tone={messaging.paused ? "warning" : "success"}>{messaging.paused ? "paused" : "accepting"}</Badge> : <Spinner />}>
        <div className="flex flex-wrap gap-1.5">
          <button className="bh-btn" onClick={() => void messagingAction(messaging?.paused ? "agent_messages_resume" : "agent_messages_pause")}>{messaging?.paused ? "Resume" : "Pause"}</button>
          <ConfirmButton label="Clear queued" confirmLabel="Clear?" onConfirm={() => void messagingAction("agent_messages_clear")} />
        </div>
      </Section>

      <Section title="Kernel bash"><KernelBash activeSessionId={activeSessionId} /></Section>

      <Section title="Reply">
        <textarea className="bh-input min-h-16" value={reply} onChange={(e) => setReply(e.target.value)} placeholder="Send a prompt to this agent…" aria-label="Reply" />
        <ErrorLine>{error}</ErrorLine>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          <button className="bh-btn bh-btn-primary" disabled={busy || !reply.trim()} onClick={() => void sendReply("prompt")}>{busy ? "Sending…" : state.isStreaming ? "Steer" : "Send"}</button>
          <button className="bh-btn" disabled={busy || !reply.trim()} onClick={() => void sendReply("message")} title="Delivers as an agent-to-agent message">As agent message</button>
        </div>
      </Section>

      <div className="flex flex-wrap gap-1.5 px-3 py-2.5">
        <button className="bh-btn" onClick={() => openPanel("workflows", { activeSessionId })}>Workflows</button>
        <button className="bh-btn" onClick={() => openPanel("heartbeats", { activeSessionId })}>Heartbeats</button>
        {entry && destructive && destructiveKind(entry) ? <ConfirmButton className="ml-auto" label={destructive} confirmLabel={`${destructive}?`} onConfirm={() => void runDestructive(entry as RosterEntry)} /> : null}
      </div>
      {s.firstMessage ? <p className="px-3 pb-3 text-[11px] text-dim">First message: {String(s.firstMessage).slice(0, 240)}</p> : null}
    </div>
  );
}
