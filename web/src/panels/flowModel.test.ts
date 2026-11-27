import { describe, expect, it } from "vitest";
import { applyEvents, emptyFlow } from "./flowModel";

describe("applyEvents", () => {
  it("keeps the sign-in link, shows a prompt, and clears it once later events arrive", () => {
    const asked = applyEvents(emptyFlow, [
      { type: "authUrl", url: "https://login.example", instructions: null },
      { type: "prompt", prompt: "Paste code", masked: false, allowEmpty: false },
    ]);
    expect(asked).toEqual({
      progress: [],
      authUrl: { url: "https://login.example", instructions: null },
      prompt: { prompt: "Paste code", masked: false, allowEmpty: false },
    });
    const done = applyEvents(asked, [{ type: "progress", message: "Exchanging" }, { type: "done", outcome: { kind: "status", message: "Logged in" } }]);
    expect(done).toEqual({
      progress: ["Exchanging"],
      authUrl: asked.authUrl,
      prompt: undefined,
      waiting: undefined,
      outcome: { kind: "status", message: "Logged in" },
    });
  });
});
