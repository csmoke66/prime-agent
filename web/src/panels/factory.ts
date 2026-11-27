// Pure parsing of the `factory_activity` graph reply (`{runs: [...]}`), after crates/bh-tui/src/factory_view.rs.
export type FactoryNode = { status: string; entriesUsed: number; maxEntries: number; running: number; queued: number; error?: string };
export type FactoryRun = {
  runId: string; specId: string; name: string; state: string; elapsedMs: number; running: number; settled: number;
  states: { id: string; entry: boolean; lifecycle?: string; subagent?: string }[];
  transitions: { from: string[]; to: string; when?: string }[];
  nodes: Record<string, FactoryNode>;
  milestones: string[];
};

const pick = (o: any, a: string, b: string) => o?.[a] ?? o?.[b];

export function parseFactoryRuns(data: any): FactoryRun[] {
  if (!Array.isArray(data?.runs)) return [];
  const runs: FactoryRun[] = [];
  for (const run of data.runs) {
    const runId = String(pick(run, "run_id", "runId") ?? "");
    const specId = String(pick(run, "spec_id", "specId") ?? "");
    if ((!runId && !specId) || !run.machine) continue;
    const nodes: Record<string, FactoryNode> = {};
    for (const node of run.nodes ?? []) {
      if (typeof node?.id !== "string") continue;
      nodes[node.id] = {
        status: String(node.status ?? "pending"),
        entriesUsed: Number(pick(node, "entries_used", "entriesUsed") ?? 0),
        maxEntries: Number(pick(node, "max_entries", "maxEntries") ?? 0),
        running: Number(node.running ?? 0),
        queued: Number(node.queued ?? 0),
        error: node.error ? String(node.error) : undefined,
      };
    }
    runs.push({
      runId, specId,
      name: String(run.name || specId || runId),
      state: String(run.state ?? "unknown"),
      elapsedMs: Number(pick(run, "elapsed_ms", "elapsedMs") ?? 0),
      running: Number(run.usage?.running ?? 0),
      settled: Number(run.usage?.settled ?? 0),
      states: (run.machine.states ?? []).filter((s: any) => typeof s?.id === "string").map((s: any) => ({ id: s.id, entry: Boolean(s.entry), lifecycle: s.lifecycle, subagent: s.subagent })),
      transitions: (run.machine.transitions ?? []).map((t: any) => ({ from: Array.isArray(t.from) ? t.from : [t.from], to: String(t.to), when: t.when })),
      nodes,
      milestones: (run.events ?? []).filter((e: any) => e?.kind === "milestone").map((e: any) => String(e.milestone)),
    });
  }
  return runs.reverse();
}

export const isLive = (run: FactoryRun): boolean => ["running", "stopping", "paused"].includes(run.state) || run.running > 0;
