import { describe, expect, it } from "vitest";
import { applyRosterUpdate, buildForest, flatten, type RosterEntry } from "./roster";

const entry = (agentId: string, summary: Record<string, any> = {}, status: RosterEntry["status"] = "idle"): RosterEntry => ({ agentId, status, summary });

describe("applyRosterUpdate", () => {
  it("upserts, removes and resyncs", () => {
    let roster = applyRosterUpdate([], { changed: [entry("a"), entry("b")] });
    roster = applyRosterUpdate(roster, { changed: [entry("a", { x: 1 }, "running")], removed: ["b"] });
    expect(roster).toEqual([entry("a", { x: 1 }, "running")]);
    expect(applyRosterUpdate(roster, { resync: true, changed: [entry("z")] })).toEqual([entry("z")]);
  });
});

describe("buildForest", () => {
  it("nests subagents under their parent and sorts running first", () => {
    const roster = [
      entry("p", { sessionId: "p", sessionFile: "/s/p.jsonl" }),
      entry("p#c1", { runtimeKind: "subagent", rlmChildId: "c1", parentSessionPath: "/s/p.jsonl" }),
      entry("q", { sessionId: "q" }, "running"),
    ];
    const forest = buildForest(roster);
    expect(forest.map((n) => n.entry.agentId)).toEqual(["q", "p"]);
    expect(forest[1].children.map((n) => n.entry.agentId)).toEqual(["p#c1"]);
    expect(flatten(forest, new Set(["p"])).map((n) => n.entry.agentId)).toEqual(["q", "p"]);
  });
});
