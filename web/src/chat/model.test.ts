import { describe, expect, it } from "vitest";
import { contextPercent, formatCount, spellPathToken, applySessionEvent, emptySession, filterCommands, fromSnapshot, parseBang, parseSkillBlock, pathTokenAt, queueOf, sequenceVerdict, BUILTIN_COMMANDS } from "./model";

const assistant = (text: string) => ({ role: "assistant", content: [{ type: "text", text }] });

describe("applySessionEvent", () => {
  it("streams an assistant message and commits it on message_end", () => {
    let s = emptySession();
    s = applySessionEvent(s, { type: "message_start", message: assistant("") });
    s = applySessionEvent(s, { type: "message_update", message: assistant("hel") });
    expect(s.live).toEqual(assistant("hel"));
    s = applySessionEvent(s, { type: "message_end", message: assistant("hello") });
    expect(s.live).toBeNull();
    expect(s.messages).toEqual([assistant("hello")]);
  });

  it("folds a tool execution and settles running tools when the agent ends", () => {
    let s = emptySession();
    s = applySessionEvent(s, { type: "tool_execution_start", toolCallId: "c1", toolName: "bash", args: { command: "ls" } });
    s = applySessionEvent(s, { type: "tool_execution_start", toolCallId: "c2", toolName: "bash", args: {} });
    s = applySessionEvent(s, { type: "tool_execution_end", toolCallId: "c1", result: { content: [] }, isError: false });
    s = applySessionEvent(s, { type: "agent_end" });
    expect(s.tools.c1.status).toBe("done");
    expect(s.tools.c2.status).toBe("error");
  });

  it("turns compaction events into a summary row", () => {
    let s = applySessionEvent(emptySession(), { type: "compaction_start", reason: "manual" });
    s = applySessionEvent(s, { type: "compaction_summary_delta", delta: "abc" });
    expect(s.compaction?.summary).toBe("abc");
    s = applySessionEvent(s, { type: "compaction_end", result: { summary: "done", tokensBefore: 9 } });
    expect(s.compaction).toBeNull();
    expect(s.messages[0]).toMatchObject({ role: "compactionSummary", summary: "done", tokensBefore: 9 });
  });

  it("collects user bash output into a transcript row", () => {
    let s = applySessionEvent(emptySession(), { type: "bash_start", command: "echo hi", excludeFromContext: true });
    s = applySessionEvent(s, { type: "bash_output", chunk: "hi\n" });
    s = applySessionEvent(s, { type: "bash_end", exitCode: 0 });
    expect(s.userBash).toBeNull();
    expect(s.messages[0]).toMatchObject({ role: "bashExecution", command: "echo hi", output: "hi\n", exitCode: 0, excludeFromContext: true });
  });
});

describe("sequenceVerdict", () => {
  it("drops replays and accepts skipped numbers without re-attaching", () => {
    expect(sequenceVerdict(5, 5)).toBe("replay");
    expect(sequenceVerdict(5, 6)).toBe("ok");
    expect(sequenceVerdict(5, 8)).toBe("ok");
    expect(sequenceVerdict(0, 3)).toBe("ok");
  });
});

describe("parsers", () => {
  it("parses skill blocks with a trailing request", () => {
    const text = '<skill name="x" location="/p">\nbody\n</skill>\n\nplease go';
    expect(parseSkillBlock(text)).toEqual({ name: "x", location: "/p", content: "body", userMessage: "please go" });
    expect(parseSkillBlock("plain")).toBeNull();
  });
  it("parses bash prefixes and @path tokens", () => {
    expect(parseBang("!!ls -l")).toEqual({ command: "ls -l", excludeFromContext: true });
    expect(parseBang("! pwd")).toEqual({ command: "pwd", excludeFromContext: false });
    expect(pathTokenAt("see @src/ma", 11)).toEqual({ start: 4, query: "src/ma", quote: "" });
    expect(pathTokenAt('see @"my di', 11)).toEqual({ start: 4, query: "my di", quote: '"' });
    expect(spellPathToken("my dir/", "")).toBe('@"my dir/');
    expect(spellPathToken("a b.txt", "'")).toBe("@'a b.txt' ");
    expect(pathTokenAt("mail a@b", 8)).toBeNull();
  });
  it("ranks slash commands by prefix", () => {
    expect(filterCommands(BUILTIN_COMMANDS, "co")[0].name).toBe("copy");
  });
  it("reads the queue and snapshot state", () => {
    const state = { sessionActions: { steering: ["a"], followUps: ["b", "c"], active: { kind: "turn", phase: "preparing", label: "x" } } };
    expect(queueOf(state)).toEqual({ steering: ["a"], followUps: ["b", "c"], starting: "x" });
    expect(fromSnapshot({ state: { isStreaming: true }, messages: [assistant("a")], lastEventSequence: 4 }).lastSequence).toBe(4);
  });
});

import { counterpartName as _counterpartName, sentMessages as _sentMessages } from "./model";
describe("agent message rows", () => {
  it("names the counterpart like the TUI", () => {
    expect(_counterpartName({ sessionName: " ", activeSessionId: "a1" })).toBe("a1");
    expect(_counterpartName(undefined)).toBe("unknown");
  });
  it("turns python receipts into sent and queued rows", () => {
    const rows = _sentMessages({ sentAgentMessages: [{ id: "m1", message: "Ping", deliveryStatus: "delivered", target: { sessionName: "Worker" } }, { message: "Later", deliveryStatus: "queued", target: { sessionId: "s" } }, { id: "bad" }] });
    expect(rows).toEqual([{ direction: "sent", who: "Worker", message: "Ping", id: "m1" }, { direction: "queued", who: "s", message: "Later", id: undefined }]);
  });
});

describe("formatCount", () => {
  it("shows an unknown count (null after compaction) instead of throwing", () => {
    expect(formatCount(null)).toBe("?");
    expect(formatCount(1234)).toBe((1234).toLocaleString());
  });
});

describe("contextPercent", () => {
  it("is unknown (not 0%) when the tokens are null after a compaction", () => {
    expect(contextPercent({ contextUsage: { tokens: null, contextWindow: 1_000_000 } })).toBeNull();
    expect(contextPercent({ contextUsage: { tokens: 250_000, contextWindow: 1_000_000 } })).toBe(25);
  });
});
