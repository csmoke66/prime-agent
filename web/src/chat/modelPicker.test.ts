import { describe, expect, it } from "vitest";
import { clampLevel, orderModels, supportedLevels } from "./modelPicker";

describe("model picker helpers", () => {
  it("lists the levels a model supports", () => {
    expect(supportedLevels({ provider: "p", id: "a" })).toEqual(["off"]);
    expect(supportedLevels({ provider: "p", id: "b", reasoning: true })).toEqual(["off", "minimal", "low", "medium", "high"]);
    expect(supportedLevels({ provider: "p", id: "c", reasoning: true, thinkingLevelMap: { minimal: null, xhigh: "x" } })).toEqual(["off", "low", "medium", "high", "xhigh"]);
  });
  it("clamps to the nearest higher level first", () => {
    expect(clampLevel(["off", "low", "high"], "medium")).toBe("high");
    expect(clampLevel(["off", "low"], "high")).toBe("low");
  });
  it("orders current, recent, then the rest", () => {
    const models = ["z/1", "a/1", "m/1", "b/1"].map((k) => ({ provider: k.split("/")[0], id: "1" }));
    expect(orderModels(models, { provider: "m", id: "1" }, ["b/1", "z/1"]).map((m) => m.provider)).toEqual(["m", "b", "z", "a"]);
  });
});
