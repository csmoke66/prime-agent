import { Markdown } from "./markdown";

export type MailDirection = "received" | "sent" | "queued";

const ARROW: Record<MailDirection, string> = { received: "↓", sent: "↑", queued: "◇" };

/** One agent-message row: `✉ Agent message · ↓ name`, with the body underneath (as in the TUI). */
export function AgentMail({ direction, who, message }: { direction: MailDirection; who: string; message: string }) {
  return (
    <div className="my-1 ml-[1.15rem] rounded border-l-2 border-info/60 bg-panel px-2 py-1 text-[13px]">
      <div className="mb-0.5 text-[10px] uppercase tracking-wider text-info">✉ Agent message · {ARROW[direction]} {who}</div>
      <Markdown text={message} />
    </div>
  );
}
