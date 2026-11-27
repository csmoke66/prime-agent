// Pure roster model: the daemon's `roster_subscribe` snapshot plus `roster_update` deltas, and the
// parent/child forest the agents view and session picker render.

export type RosterEntry = {
  agentId: string;
  status: "running" | "idle" | "inactive";
  statusLabel?: string;
  queuedChild?: boolean;
  lastHeardFromAt?: string;
  summary: Record<string, any>;
  [key: string]: any;
};

export type RosterUpdate = { changed?: RosterEntry[]; removed?: string[]; resync?: boolean };

export function applyRosterUpdate(roster: RosterEntry[], update: RosterUpdate): RosterEntry[] {
  const changed = update.changed ?? [];
  if (update.resync) return changed.filter((entry) => entry?.agentId);
  const removed = new Set(update.removed ?? []);
  const next = roster.filter((entry) => !removed.has(entry.agentId));
  for (const entry of changed) {
    if (!entry?.agentId) continue;
    const index = next.findIndex((existing) => existing.agentId === entry.agentId);
    if (index >= 0) next[index] = entry;
    else next.push(entry);
  }
  return next;
}

export type AgentNode = { entry: RosterEntry; children: AgentNode[]; depth: number };

const str = (value: unknown): string => (typeof value === "string" ? value : "");

export function isSubagent(entry: RosterEntry): boolean {
  return entry.summary.runtimeKind === "subagent" || (Boolean(entry.summary.rlmChildId) && Boolean(parentKey(entry)));
}

function parentKey(entry: RosterEntry): string {
  return str(entry.summary.parentSessionPath) || str(entry.summary.parentActiveSessionId);
}

/** Keys that identify an entry as someone's parent: session file, durable id, live id. */
function identityKeys(entry: RosterEntry): string[] {
  const s = entry.summary;
  return [str(s.sessionFile), str(s.sessionPath), str(s.sessionId), str(s.activeSessionId), entry.agentId].filter(Boolean);
}

export function sessionTitle(entry: RosterEntry): string {
  const s = entry.summary;
  return str(s.sessionName) || str(s.name) || oneLineFirst(s) || str(s.sessionId).slice(0, 8) || entry.agentId.slice(0, 8);
}

function oneLineFirst(s: Record<string, any>): string {
  const text = str(s.firstMessage).replace(/\s+/g, " ").trim();
  return text.length > 80 ? `${text.slice(0, 79)}…` : text;
}

/** Build the parent/child forest; running agents first, then most recent activity. */
export function buildForest(roster: RosterEntry[]): AgentNode[] {
  const byKey = new Map<string, RosterEntry>();
  for (const entry of roster) for (const key of identityKeys(entry)) if (!byKey.has(key)) byKey.set(key, entry);
  const nodes = new Map<string, AgentNode>(roster.map((entry) => [entry.agentId, { entry, children: [], depth: 0 }]));
  const roots: AgentNode[] = [];
  for (const entry of roster) {
    const node = nodes.get(entry.agentId)!;
    const key = parentKey(entry);
    const parent = key ? byKey.get(key) : undefined;
    if (parent && parent.agentId !== entry.agentId && isSubagent(entry)) nodes.get(parent.agentId)!.children.push(node);
    else roots.push(node);
  }
  const rank = (node: AgentNode) => (node.entry.status === "running" ? 0 : node.entry.status === "idle" ? 1 : 2);
  const recency = (node: AgentNode) => Date.parse(str(node.entry.summary.lastActivityAt) || str(node.entry.summary.modified) || str(node.entry.summary.created)) || 0;
  const sort = (list: AgentNode[], depth: number) => {
    list.sort((a, b) => rank(a) - rank(b) || recency(b) - recency(a));
    for (const node of list) {
      node.depth = depth;
      sort(node.children, depth + 1);
    }
  };
  sort(roots, 0);
  return roots;
}

export function flatten(nodes: AgentNode[], collapsed: ReadonlySet<string>): AgentNode[] {
  const out: AgentNode[] = [];
  const walk = (list: AgentNode[]) => {
    for (const node of list) {
      out.push(node);
      if (!collapsed.has(node.entry.agentId)) walk(node.children);
    }
  };
  walk(nodes);
  return out;
}

export function descendantCount(node: AgentNode): number {
  return node.children.reduce((sum, child) => sum + 1 + descendantCount(child), 0);
}

export function activityLabel(entry: RosterEntry): string {
  const s = entry.summary;
  if (entry.statusLabel) return entry.statusLabel;
  if (s.workerState && s.workerState !== "ready") return String(s.workerState);
  if (s.isCompacting) return "compacting";
  if (s.isStreaming) return s.isRunningTools ? "running tools" : "thinking";
  if (s.isRunningTools) return "running tools";
  if (s.isBashRunning) return "running bash";
  if (s.hasRunningSubagents) return "subagents running";
  if (s.lifecycle === "archived") return "archived";
  if (s.hasActiveHeartbeat) return "heartbeat";
  if (s.runtimeKind === "subagent" && s.repliedSinceTask) return "replied";
  if (s.activity === "working") return "working";
  if (s.taskState === "error") return "error";
  if (s.taskState === "completed") return "completed";
  return entry.status === "inactive" ? "saved" : "needs input";
}

/** Draft sessions that never got a name or a message (the TUI hides these too). */
export function isUnnamedEmpty(entry: RosterEntry): boolean {
  const s = entry.summary;
  return s.lifecycle === "draft" && !s.sessionName && !s.name && !s.messageCount && entry.status !== "running";
}

/** Model of a summary: a plain id string or a `{provider, id|modelId}` object. */
export function modelLabel(summary: Record<string, any>): string {
  const m = summary.model;
  if (!m) return [summary.provider, summary.modelId].filter(Boolean).join("/");
  if (typeof m === "string") return m;
  const id = m.id ?? m.modelId ?? "";
  return [m.provider, id].filter(Boolean).join("/");
}
