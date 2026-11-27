import { describe, expect, it } from "vitest";
import { initialSaved, savedReducer, type SavedSession } from "./savedSessions";

const session = (path: string, modified: string): SavedSession => ({ path, id: path, cwd: "/p", created: modified, modified, messageCount: 1, firstMessage: "hi" });

describe("savedReducer", () => {
  it("streams rows newest first without duplicating the final page", () => {
    let state = savedReducer(initialSaved, { type: "start", reset: true });
    state = savedReducer(state, { type: "item", session: session("a", "2026-01-01") });
    state = savedReducer(state, { type: "item", session: session("b", "2026-02-01") });
    state = savedReducer(state, { type: "done", sessions: [session("a", "2026-01-01")], cursor: "c", remaining: 5 });
    expect(state.sessions.map((s) => s.path)).toEqual(["b", "a"]);
    expect(state).toMatchObject({ loading: false, cursor: "c", remaining: 5 });
  });
  it("renames and removes rows", () => {
    let state = savedReducer(initialSaved, { type: "item", session: session("a", "x") });
    state = savedReducer(state, { type: "rename", path: "a", name: "n" });
    expect(state.sessions[0].name).toBe("n");
    expect(savedReducer(state, { type: "remove", path: "a" }).sessions).toEqual([]);
  });
});
