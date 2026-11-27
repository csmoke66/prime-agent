import { describe, expect, it } from "vitest";
import { buildTree, visibleRows, type FlatNode } from "./treeModel";

const msg = (id: string, parentId: string | null, role: string, text: string, ts: string): FlatNode => ({
  entry: { id, parentId, timestamp: ts, type: "message", message: { role, content: text } },
});

describe("visibleRows", () => {
  const flat: FlatNode[] = [
    msg("1", null, "user", "hello", "1"),
    msg("2", "1", "assistant", "hi", "2"),
    msg("3", "2", "user", "alpha", "3"),
    msg("4", "2", "user", "beta", "4"),
    { entry: { id: "5", parentId: "4", timestamp: "5", type: "model_change", provider: "x", modelId: "y" } },
  ];
  const roots = buildTree(flat);
  it("indents only below branch points and marks the active path", () => {
    const rows = visibleRows(roots, { mode: "default", leafId: "4", search: "", folded: new Set() });
    expect(rows.map((r) => [r.node.id, r.indent, r.onPath])).toEqual([["1", 0, true], ["2", 0, true], ["3", 1, false], ["4", 1, true]]);
  });
  it("filters user-only, searches and folds", () => {
    expect(visibleRows(roots, { mode: "user-only", leafId: "4", search: "", folded: new Set() }).map((r) => r.node.id)).toEqual(["1", "3", "4"]);
    expect(visibleRows(roots, { mode: "all", leafId: "4", search: "alpha", folded: new Set() }).map((r) => r.node.id)).toEqual(["3"]);
    expect(visibleRows(roots, { mode: "default", leafId: "4", search: "", folded: new Set(["2"]) }).map((r) => r.node.id)).toEqual(["1", "2"]);
  });
});
