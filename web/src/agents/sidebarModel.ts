// Sidebar list model: one list for live and saved agents, grouped like the TUI agents view
// (`group_rows` + section headings): custom groups first by label, then ungrouped trees by status.
import type { SavedSession } from "../panels/savedSessions";
import type { AgentNode, RosterEntry } from "./roster";
import type { Groups } from "./groups";

export type SidebarSection = { key: string; title: string; roots: AgentNode[] };

const SECTION_TITLE: Record<RosterEntry["status"], string> = { running: "Running", idle: "Idle", inactive: "Inactive" };

/** A saved session that no live roster entry covers, shown as an inactive agent. */
export function savedEntry(saved: SavedSession): RosterEntry {
  const model = saved.model ? { provider: saved.model.provider, id: saved.model.modelId } : undefined;
  return {
    agentId: `saved:${saved.path}`,
    status: "inactive",
    summary: {
      sessionId: saved.id,
      sessionFile: saved.path,
      cwd: saved.cwd,
      sessionName: saved.name,
      firstMessage: saved.firstMessage,
      messageCount: saved.messageCount,
      created: saved.created,
      modified: saved.modified,
      parentSessionPath: saved.parentSessionPath,
      runtimeKind: saved.parentSessionPath ? "subagent" : undefined,
      rlmDepth: saved.rlmDepth,
      cost: saved.usage?.cost,
      model,
    },
  };
}

/** Saved sessions not already present in the live roster; empty unnamed ones are dropped like the TUI does. */
export function inactiveEntries(roster: RosterEntry[], saved: SavedSession[]): RosterEntry[] {
  const known = new Set(roster.flatMap((e) => [e.summary.sessionId, e.summary.sessionFile, e.summary.path].filter(Boolean)));
  return saved.filter((s) => !known.has(s.id) && !known.has(s.path) && (s.name || s.messageCount > 0)).map(savedEntry);
}

export function sidebarSections(forest: AgentNode[], groups: Groups): SidebarSection[] {
  const custom = new Map<string, AgentNode[]>();
  const byStatus = new Map<RosterEntry["status"], AgentNode[]>();
  for (const node of forest) {
    const label = groups[String(node.entry.summary.sessionId)];
    const bucket = label ? custom : byStatus;
    const key = (label ?? node.entry.status) as string;
    (bucket as Map<string, AgentNode[]>).set(key, [...((bucket as Map<string, AgentNode[]>).get(key) ?? []), node]);
  }
  const sections: SidebarSection[] = [...custom].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)).map(([label, roots]) => ({ key: `group:${label}`, title: label, roots }));
  for (const status of ["running", "idle", "inactive"] as const) {
    const roots = byStatus.get(status);
    if (roots) sections.push({ key: `status:${status}`, title: SECTION_TITLE[status], roots });
  }
  return sections;
}
