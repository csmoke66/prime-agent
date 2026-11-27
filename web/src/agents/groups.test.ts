import { describe, expect, it } from "vitest";
import { withGroup } from "./groups";

describe("withGroup", () => {
  it("sets and clears a label", () => {
    const set = withGroup({}, "s1", " work ");
    expect(set).toEqual({ s1: "work" });
    expect(withGroup(set, "s1", "")).toEqual({});
  });
});
