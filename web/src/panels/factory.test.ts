import { describe, expect, it } from "vitest";
import { isLive, parseFactoryRuns } from "./factory";

describe("parseFactoryRuns", () => {
  it("parses runs newest first and drops malformed rows", () => {
    const runs = parseFactoryRuns({
      runs: [
        { run_id: "a", spec_id: "s", state: "done", machine: { states: [{ id: "x", entry: true }], transitions: [] }, usage: { running: 0 } },
        { run_id: "b", spec_id: "s", state: "running", machine: { states: [], transitions: [{ from: "x", to: "y" }] }, nodes: [{ id: "x", status: "running", running: 1 }] },
        { run_id: "bad" },
      ],
    });
    expect(runs.map((r) => r.runId)).toEqual(["b", "a"]);
    expect(isLive(runs[0])).toBe(true);
    expect(isLive(runs[1])).toBe(false);
    expect(runs[0].nodes.x.running).toBe(1);
  });
});
