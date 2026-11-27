import { describe, expect, it } from "vitest";
import { fmtDuration, fuzzyFilter, fuzzyScore, relativeTime } from "./format";

describe("fuzzyScore", () => {
  it("matches subsequences and rejects others", () => {
    expect(fuzzyScore("stg", "settings")).not.toBeNull();
    expect(fuzzyScore("xyz", "settings")).toBeNull();
  });
  it("ranks word-start and consecutive matches first", () => {
    expect(fuzzyFilter(["a-session-tree", "tree", "treetop"], "tree", (s) => s)[0]).toBe("tree");
  });
});

describe("relativeTime / fmtDuration", () => {
  it("formats ages", () => {
    const now = Date.parse("2026-01-01T12:00:00Z");
    expect(relativeTime("2026-01-01T11:00:00Z", now)).toBe("1h ago");
    expect(relativeTime("2025-12-30T12:00:00Z", now)).toBe("2d ago");
  });
  it("formats durations", () => {
    expect(fmtDuration(65_000)).toBe("1m 5s");
  });
});
