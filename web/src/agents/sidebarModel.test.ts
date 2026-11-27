import { describe, expect, it } from "vitest";
import { buildForest } from "./roster";
import { inactiveEntries, sidebarSections } from "./sidebarModel";

const entry = (id: string, status: "running" | "idle" | "inactive", extra: Record<string, unknown> = {}) => ({ agentId: id, status, summary: { sessionId: id, ...extra } });

describe("sidebarSections", () => {
  it("lists custom groups by label first, then ungrouped trees by status, subagents under their parent", () => {
    const roster = [
      entry("a", "inactive"),
      entry("b", "running"),
      entry("c", "idle"),
      entry("d", "idle"),
      entry("kid", "running", { runtimeKind: "subagent", rlmChildId: "k", parentSessionPath: "b" }),
    ];
    const sections = sidebarSections(buildForest(roster), { c: "zeta", d: "alpha" });
    expect(sections.map((s) => [s.title, s.roots.map((r) => [r.entry.agentId, r.children.map((k) => k.entry.agentId)])])).toEqual([
      ["alpha", [["d", []]]],
      ["zeta", [["c", []]]],
      ["Running", [["b", ["kid"]]]],
      ["Inactive", [["a", []]]],
    ]);
  });

  it("adds saved sessions that the roster does not cover as inactive entries", () => {
    const saved = [
      { id: "s1", path: "/p/s1.jsonl", cwd: "/w", name: "old", created: "", modified: "", messageCount: 3, firstMessage: "hi" },
      { id: "live", path: "/p/live.jsonl", cwd: "/w", created: "", modified: "", messageCount: 3, firstMessage: "hi" },
      { id: "empty", path: "/p/e.jsonl", cwd: "/w", created: "", modified: "", messageCount: 0, firstMessage: "" },
    ];
    expect(inactiveEntries([entry("live", "idle")], saved).map((e) => [e.agentId, e.status])).toEqual([["saved:/p/s1.jsonl", "inactive"]]);
  });
});
