import { describe, expect, it } from "vitest";
import { getPath, setPath } from "./settingsSchema";

describe("settings paths", () => {
  it("reads and writes nested keys without mutating", () => {
    const base = { compaction: { enabled: true } };
    const next = setPath(base, "compaction.thresholdPercent", 80);
    expect(next).toEqual({ compaction: { enabled: true, thresholdPercent: 80 } });
    expect(base).toEqual({ compaction: { enabled: true } });
    expect(getPath(next, "compaction.thresholdPercent")).toBe(80);
  });
  it("removes keys and prunes empty parents", () => {
    expect(setPath({ retry: { enabled: false } }, "retry.enabled", undefined)).toEqual({});
  });
});
