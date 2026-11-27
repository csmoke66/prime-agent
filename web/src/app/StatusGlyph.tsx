import type { RosterEntry } from "../agents/roster";

/** The one agent-status marker: spinner when running, green dot when idle, hollow ring when inactive. */
export function StatusGlyph({ status }: { status: RosterEntry["status"] }) {
  if (status === "running") return <span className="grid h-4 w-3 shrink-0 place-items-center" role="img" aria-label="running"><span className="bh-spin" /></span>;
  const dot = status === "idle" ? "bg-success/80" : "border border-grid";
  return <span className="grid h-4 w-3 shrink-0 place-items-center" role="img" aria-label={status}><span className={`h-1.5 w-1.5 rounded-full ${dot}`} /></span>;
}
