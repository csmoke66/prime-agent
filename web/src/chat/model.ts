// Pure session-view logic: wire message shapes, the live-event reducer, and small parsers.
// Nothing here touches React or the socket, so the reducer is unit-testable.

export type Msg = { role: string; [key: string]: any };
export type Block = { type: string; [key: string]: any };

export interface ToolExec {
  id: string;
  name: string;
  args: any;
  status: "running" | "done" | "error";
  partial?: any;
  result?: any;
}

export interface UserBash {
  command: string;
  output: string;
  running: boolean;
  excluded: boolean;
  exitCode?: number | null;
  cancelled?: boolean;
  truncated?: boolean;
  fullOutputPath?: string;
  error?: string;
  transient?: boolean;
  runId?: string;
}

export interface CompactionState {
  active: boolean;
  reason: string;
  summary: string;
  customInstructions?: string;
}

export interface RetryState {
  attempt: number;
  maxAttempts: number;
  delayMs: number;
  errorMessage: string;
  backupModel?: string;
  startedAt: number;
}

export interface SideTurn {
  id: string;
  question: string;
  answer: string;
  status: string;
  errorMessage?: string;
  local?: boolean;
}

/** Everything the transcript and status bar derive from the session's event stream. */
export interface SessionData {
  messages: Msg[];
  live: Msg | null;
  tools: Record<string, ToolExec>;
  state: Record<string, any>;
  userBash: UserBash | null;
  compaction: CompactionState | null;
  retry: RetryState | null;
  lastSequence: number;
  generation: string;
  historyBeforeEntryId: string | null;
  turnStartedAt: number | null;
  /** Counter of locally minted rows, so keys stay unique. */
  localSeq: number;
}

export const emptySession = (): SessionData => ({
  messages: [],
  live: null,
  tools: {},
  state: {},
  userBash: null,
  compaction: null,
  retry: null,
  lastSequence: 0,
  generation: "",
  historyBeforeEntryId: null,
  turnStartedAt: null,
  localSeq: 0,
});

export type StatusKind = "info" | "warning" | "error";

export function statusRow(s: SessionData, text: string, kind: StatusKind = "info"): [SessionData, Msg] {
  const localSeq = s.localSeq + 1;
  return [{ ...s, localSeq }, { role: "_status", kind, text, key: `local-${localSeq}`, timestamp: Date.now() }];
}

export function pushStatus(s: SessionData, text: string, kind: StatusKind = "info"): SessionData {
  const [next, row] = statusRow(s, text, kind);
  return { ...next, messages: [...next.messages, row] };
}

export function textOfContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((block: Block) => (block?.type === "text" || block?.type === undefined ? (block?.text ?? "") : ""))
    .filter(Boolean)
    .join("\n");
}

export function assistantText(message: Msg): string {
  if (!Array.isArray(message.content)) return textOfContent(message.content);
  return message.content
    .filter((block: Block) => block.type === "text")
    .map((block: Block) => block.text ?? "")
    .join("");
}

/** The `toolResult` messages of a transcript keyed by the call they complete. */
export function resultsById(messages: Msg[]): Map<string, Msg> {
  const map = new Map<string, Msg>();
  for (const message of messages) if (message.role === "toolResult" && message.toolCallId) map.set(message.toolCallId, message);
  return map;
}

export interface SkillBlock {
  name: string;
  location: string;
  content: string;
  userMessage?: string;
}

/** Mirror of `bh_types::skill_blocks::parse_skill_block`. */
export function parseSkillBlock(text: string): SkillBlock | null {
  const open = '<skill name="';
  if (!text.startsWith(open)) return null;
  let rest = text.slice(open.length);
  let i = rest.indexOf('"');
  if (i < 0) return null;
  const name = rest.slice(0, i);
  rest = rest.slice(i + 1);
  const locOpen = ' location="';
  if (!rest.startsWith(locOpen)) return null;
  rest = rest.slice(locOpen.length);
  i = rest.indexOf('"');
  if (i < 0) return null;
  const location = rest.slice(0, i);
  rest = rest.slice(i + 1);
  if (!rest.startsWith(">\n")) return null;
  const body = rest.slice(2);
  const close = "\n</skill>";
  let from = 0;
  for (;;) {
    const end = body.indexOf(close, from);
    if (end < 0) return null;
    const tail = body.slice(end + close.length);
    if (tail === "") return { name, location, content: body.slice(0, end) };
    if (tail.startsWith("\n\n") && tail.slice(2) !== "") {
      const trimmed = tail.trim();
      return { name, location, content: body.slice(0, end), userMessage: trimmed || undefined };
    }
    from = end + 1;
  }
}

// ---------- state / queue ----------

export interface QueueView {
  steering: string[];
  followUps: string[];
  starting: string | null;
}

export function queueOf(state: Record<string, any>): QueueView {
  const actions = state.sessionActions ?? {};
  const strings = (value: unknown) => (Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : []);
  const active = actions.active;
  const starting = active?.kind === "turn" && active?.phase === "preparing" ? String(active.label ?? "") : null;
  return { steering: strings(actions.steering), followUps: strings(actions.followUps), starting };
}

export function modelLabel(state: Record<string, any>): string {
  const model = state.model;
  if (!model) return "no model";
  return typeof model === "string" ? model : (model.name ?? model.id ?? "model");
}

export function contextPercent(state: Record<string, any>): number | null {
  const usage = state.contextUsage;
  if (!usage || !usage.contextWindow) return null;
  return (usage.tokens / usage.contextWindow) * 100;
}

export function formatTokens(count: number): string {
  if (count < 1000) return String(count);
  if (count < 10_000) return `${(count / 1000).toFixed(1)}k`;
  if (count < 1_000_000) return `${Math.round(count / 1000)}k`;
  if (count < 10_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  return `${Math.round(count / 1_000_000)}M`;
}

export function formatCost(total: number): string {
  if (total === 0) return "$0";
  if (total < 0.01) return `$${total.toFixed(4)}`;
  return `$${total.toFixed(2)}`;
}

export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** Cost and token totals over the assistant messages of the loaded transcript. */
export function usageTotals(messages: Msg[]): { input: number; output: number; cacheRead: number; cacheWrite: number; cost: number } {
  const total = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 };
  for (const m of messages) {
    if (m.role !== "assistant" || !m.usage) continue;
    total.input += m.usage.input ?? 0;
    total.output += m.usage.output ?? 0;
    total.cacheRead += m.usage.cacheRead ?? 0;
    total.cacheWrite += m.usage.cacheWrite ?? 0;
    total.cost += m.usage.cost?.total ?? 0;
  }
  return total;
}

// ---------- snapshot ----------

/** The slim attach / resync / snapshot payload folded into a fresh session record. */
export function fromSnapshot(snapshot: any, extra?: { generation?: string; lastEventSequence?: number }): SessionData {
  const state = snapshot?.state ?? {};
  const base = emptySession();
  return {
    ...base,
    messages: Array.isArray(snapshot?.messages) ? snapshot.messages : [],
    state,
    lastSequence: snapshot?.lastEventSequence ?? extra?.lastEventSequence ?? 0,
    generation: snapshot?.lastEventCursor?.generation ?? extra?.generation ?? "",
    historyBeforeEntryId: snapshot?.historyBeforeEntryId ?? null,
    compaction: state.isCompacting ? { active: true, reason: "manual", summary: "" } : null,
    userBash: state.isBashRunning ? { command: "", output: "", running: true, excluded: false } : null,
    turnStartedAt: state.isStreaming ? Date.now() : null,
  };
}

// ---------- event reducer ----------

const settleTools = (tools: Record<string, ToolExec>): Record<string, ToolExec> => {
  let changed = false;
  const next: Record<string, ToolExec> = {};
  for (const [id, tool] of Object.entries(tools)) {
    if (tool.status === "running") {
      changed = true;
      next[id] = { ...tool, status: "error" };
    } else next[id] = tool;
  }
  return changed ? next : tools;
};

/** Event types after which the connection state must be re-read (the daemon sends no patch). */
export const REFRESH_STATE_EVENTS = new Set(["agent_end", "compaction_end", "turn_end", "model_changed", "thinking_level_changed", "auto_compaction_changed"]);

export function applySessionEvent(s: SessionData, event: any): SessionData {
  switch (event?.type) {
    case "agent_start":
    case "turn_start":
      return { ...s, state: { ...s.state, isStreaming: true }, turnStartedAt: s.turnStartedAt ?? Date.now(), retry: null };
    case "message_start": {
      const message: Msg = event.message;
      if (!message) return s;
      if (message.role === "assistant") return { ...s, live: message };
      if (message.role === "user") return { ...s, messages: [...s.messages, message], turnStartedAt: s.turnStartedAt ?? Date.now() };
      if (message.role === "custom") return message.display === false ? s : { ...s, messages: [...s.messages, message] };
      if (message.role === "branchSummary" || message.role === "compactionSummary") return { ...s, messages: [...s.messages, message] };
      return s;
    }
    case "message_update": {
      const message: Msg = event.message;
      if (message?.role !== "assistant") return s;
      return { ...s, live: message };
    }
    case "message_end": {
      const message: Msg = event.message;
      if (message?.role === "assistant") return { ...s, live: null, messages: [...s.messages, message] };
      if (message?.role === "toolResult") {
        const known = s.messages.some((m) => m.role === "toolResult" && m.toolCallId === message.toolCallId);
        return known ? s : { ...s, messages: [...s.messages, message] };
      }
      return s;
    }
    case "tool_execution_start": {
      const id = String(event.toolCallId ?? "");
      return { ...s, tools: { ...s.tools, [id]: { id, name: event.toolName ?? "", args: event.args, status: "running" } } };
    }
    case "tool_execution_update": {
      const id = String(event.toolCallId ?? "");
      const prev = s.tools[id] ?? { id, name: event.toolName ?? "", args: event.args, status: "running" as const };
      return { ...s, tools: { ...s.tools, [id]: { ...prev, partial: event.partialResult } } };
    }
    case "tool_execution_end": {
      const id = String(event.toolCallId ?? "");
      const prev = s.tools[id] ?? { id, name: event.toolName ?? "", args: event.args, status: "running" as const };
      return { ...s, tools: { ...s.tools, [id]: { ...prev, status: event.isError ? "error" : "done", result: event.result, partial: undefined } } };
    }
    case "turn_end": {
      const message = event.message;
      if (event.error && !(message?.stopReason === "error" && message?.errorMessage)) return pushStatus(s, String(event.error), "error");
      return s;
    }
    case "agent_end": {
      let next: SessionData = { ...s, state: { ...s.state, isStreaming: false }, live: null, tools: settleTools(s.tools), turnStartedAt: null, retry: null };
      if (s.live) next = { ...next, messages: [...next.messages, s.live] };
      return next;
    }
    case "compaction_start":
      return { ...s, state: { ...s.state, isCompacting: true }, compaction: { active: true, reason: event.reason ?? "manual", summary: "", customInstructions: event.customInstructions } };
    case "compaction_summary_delta":
      return s.compaction ? { ...s, compaction: { ...s.compaction, summary: s.compaction.summary + String(event.delta ?? "") } } : s;
    case "compaction_end": {
      let next: SessionData = { ...s, state: { ...s.state, isCompacting: false }, compaction: null };
      const result = event.result;
      if (event.aborted) return pushStatus(next, "Compaction cancelled", "warning");
      if (event.errorMessage) return pushStatus(next, String(event.errorMessage), event.errorSeverity === "warning" ? "warning" : "error");
      if (result?.summary) {
        next = {
          ...next,
          messages: [
            ...next.messages,
            { role: "compactionSummary", summary: result.summary, tokensBefore: result.tokensBefore, customInstructions: event.customInstructions, timestamp: Date.now(), key: `compact-${next.localSeq + 1}` },
          ],
          localSeq: next.localSeq + 1,
        };
      }
      return next;
    }
    case "auto_retry_start":
      return {
        ...s,
        retry: {
          attempt: event.attempt ?? 0,
          maxAttempts: event.maxAttempts ?? 0,
          delayMs: event.delayMs ?? 0,
          errorMessage: event.errorMessage ?? "Unknown error",
          backupModel: event.reason === "backup" ? (event.backupModel ?? "unknown") : undefined,
          startedAt: Date.now(),
        },
      };
    case "auto_retry_end": {
      const next = { ...s, retry: null };
      if (event.aborted) return pushStatus(next, "Retry cancelled", "warning");
      if (!event.success && event.finalError) return pushStatus(next, `Retry failed after ${event.attempt ?? 0} attempts: ${event.finalError}`, "error");
      return next;
    }
    case "goal_update":
      return { ...s, state: { ...s.state, goal: event.goal } };
    case "session_action_update":
      return { ...s, state: { ...s.state, sessionActions: event.actions } };
    case "session_info_changed":
      return { ...s, state: { ...s.state, sessionName: event.name ?? undefined } };
    case "service_tier_changed":
      return { ...s, state: { ...s.state, serviceTier: event.serviceTier } };
    case "bash_start":
      if (event.transient) return s;
      return {
        ...s,
        state: { ...s.state, isBashRunning: true },
        userBash: { command: event.command ?? "", output: "", running: true, excluded: !!event.excludeFromContext, runId: event.runId },
      };
    case "bash_output":
      return s.userBash ? { ...s, userBash: { ...s.userBash, output: s.userBash.output + String(event.chunk ?? "") } } : s;
    case "bash_end": {
      if (event.transient || !s.userBash) return { ...s, state: { ...s.state, isBashRunning: false } };
      const row: Msg = {
        role: "bashExecution",
        command: s.userBash.command,
        output: s.userBash.output,
        exitCode: event.exitCode,
        cancelled: !!event.cancelled,
        truncated: !!event.truncated,
        fullOutputPath: event.fullOutputPath,
        excludeFromContext: s.userBash.excluded,
        errorMessage: event.errorMessage,
        timestamp: Date.now(),
        key: `bash-${s.localSeq + 1}`,
      };
      return { ...s, state: { ...s.state, isBashRunning: false }, userBash: null, messages: [...s.messages, row], localSeq: s.localSeq + 1 };
    }
    default:
      return s;
  }
}

/** Sequence guard: `ok` folds the event; `replay` drops a duplicate; `gap` demands a resync. */
export function sequenceVerdict(last: number, sequence: number | undefined): "ok" | "replay" | "gap" {
  if (sequence === undefined || sequence === 0) return "ok";
  if (sequence <= last) return "replay";
  if (last > 0 && sequence > last + 1) return "gap";
  return "ok";
}

// ---------- slash commands ----------

export interface SlashCommand {
  name: string;
  description: string;
  argumentHint?: string;
  aliases?: string[];
  /** `session` commands travel to the daemon as prompt text; `chat` run here; `panel` belong to the panels owner. */
  kind: "session" | "chat" | "panel";
  /** Where a skill command comes from (`user`, `project`, `temporary`, `npm:…`), shown after the description. */
  sourceTag?: string;
}

export const BUILTIN_COMMANDS: SlashCommand[] = [
  { name: "settings", description: "Open settings menu", kind: "panel" },
  { name: "export", description: "Export session (HTML default, or specify path: .html/.jsonl)", argumentHint: "[path]", kind: "panel" },
  { name: "import", description: "Import and resume a session from a JSONL file", argumentHint: "<path.jsonl>", kind: "panel" },
  { name: "share", description: "Share session as a secret GitHub gist", kind: "panel" },
  { name: "copy", description: "Copy last agent message to clipboard", kind: "chat" },
  { name: "btw", description: "Ask a side question without adding it to the session", argumentHint: "<question>", aliases: ["side"], kind: "chat" },
  { name: "name", description: "Set or show the session display name", argumentHint: "[name]", aliases: ["rename"], kind: "chat" },
  { name: "session", description: "Show session info", kind: "chat" },
  { name: "cost", description: "Show or reset this session tree's displayed cost", argumentHint: "[reset]", kind: "chat" },
  { name: "system-prompt", description: "Show the exact system prompt sent to the model", kind: "chat" },
  { name: "logs", description: "Show where daemon and client logs are saved", kind: "chat" },
  { name: "context", description: "Show token, cost, and context usage for agent and sub-agents", argumentHint: "[all]", aliases: ["usage"], kind: "chat" },
  { name: "changelog", description: "Show changelog entries", kind: "panel" },
  { name: "hotkeys", description: "Show all keyboard shortcuts", kind: "panel" },
  { name: "fork", description: "Create a new fork from a previous user message", kind: "panel" },
  { name: "clone", description: "Duplicate the current session at the current position", kind: "panel" },
  { name: "tree", description: "Navigate session tree (switch branches)", kind: "panel" },
  { name: "login", description: "Configure provider authentication", kind: "panel" },
  { name: "logout", description: "Remove provider authentication", kind: "panel" },
  { name: "mcp", description: "Open the MCP connections menu", kind: "panel" },
  { name: "plugins", description: "Browse and connect external services", argumentHint: "[search]", kind: "panel" },
  { name: "new", description: "Start a new session, optionally named and/or with an initial prompt", argumentHint: '[--name "session name" --] [prompt]', aliases: ["clear"], kind: "panel" },
  { name: "edit_context", description: "Edit the model-visible context as JSON", kind: "panel" },
  { name: "compact", description: "Compact the session context; optional instructions focus the summary", argumentHint: "[instructions]", kind: "session" },
  { name: "refine", description: "Refine continual harness prompt notes, skills, subagents, and memory", kind: "session" },
  { name: "harness", description: "Browse and delete local and global harness entries", kind: "panel" },
  { name: "run", description: "Run a named workflow with JSON input and output", argumentHint: "[workflow [JSON]]", kind: "panel" },
  { name: "goal", description: "Set or view a persistent goal; supports pause, resume, and clear", argumentHint: "[objective]", kind: "session" },
  { name: "heartbeat", description: "Set or view a persistent heartbeat", argumentHint: "[status|pause|resume|stop|<instruction>]", kind: "panel" },
  { name: "heartbeats", description: "View and manage all user and agent heartbeats", kind: "panel" },
  { name: "resume", description: "Open the agents view, or resume a session by id or path", argumentHint: "[id|path]", kind: "panel" },
  { name: "reload", description: "Reload keybindings, skills, and prompts", kind: "chat" },
];

export function parseSlash(text: string): { name: string; args: string } | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/.exec(text.trim());
  if (!match) return null;
  return { name: match[1], args: (match[2] ?? "").trim() };
}

export function resolveCommand(name: string): SlashCommand | undefined {
  return BUILTIN_COMMANDS.find((c) => c.name === name || c.aliases?.includes(name));
}

/** Rank commands for a typed prefix: prefix matches first, then substring. */
const isSubsequence = (needle: string, hay: string): boolean => {
  let from = 0;
  return [...needle].every((ch) => (from = hay.indexOf(ch, from) + 1) > 0);
};

/** The TUI menu's source label for a skill's `sourceInfo`. */
export function skillSourceTag(info: { scope?: string; source?: string } | undefined): string | undefined {
  if (!info) return undefined;
  const scope = info.scope === "user" || info.scope === "project" ? info.scope : "temporary";
  const source = (info.source ?? "").trim();
  if (source === "builtin") return "builtin";
  return source.startsWith("npm:") ? `${scope}:${source}` : scope;
}

export function filterCommands(commands: SlashCommand[], query: string): SlashCommand[] {
  const q = query.toLowerCase();
  const score = (c: SlashCommand) => {
    const names = [c.name, ...(c.aliases ?? [])].map((n) => n.toLowerCase());
    if (names.some((n) => n.startsWith(q))) return 0;
    if (names.some((n) => n.includes(q))) return 1;
    if (names.some((n) => isSubsequence(q, n))) return 2;
    return c.description.toLowerCase().includes(q) ? 3 : 4;
  };
  return commands.filter((c) => score(c) < 4).sort((a, b) => score(a) - score(b));
}

/** `!cmd` runs a command whose output joins the context, `!!cmd` one excluded from it. */
export function parseBang(text: string): { command: string; excludeFromContext: boolean } | null {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("!")) return null;
  const excluded = trimmed.startsWith("!!");
  const command = trimmed.slice(excluded ? 2 : 1).trim();
  return command ? { command, excludeFromContext: excluded } : null;
}

/** The `@path` token under the caret, if any. */
export function pathTokenAt(text: string, caret: number): { start: number; query: string; quote: string } | null {
  const before = text.slice(0, caret);
  const quoted = /(?:^|\s)@(["'])([^"']*)$/.exec(before);
  if (quoted) return { start: caret - quoted[2].length - 2, query: quoted[2], quote: quoted[1] };
  const match = /(?:^|\s)@([^\s"']*)$/.exec(before);
  if (!match) return null;
  return { start: caret - match[1].length - 1, query: match[1], quote: "" };
}

/** How a completed path is written after `@`: in the quote the user typed, else quoted only when it needs it. */
export function spellPathToken(value: string, typedQuote: string): string {
  const isDir = value.endsWith("/");
  const quote = typedQuote || (/[\s"'=]/.test(value) ? (value.includes('"') ? "'" : '"') : "");
  if (!quote) return `@${value}${isDir ? "" : " "}`;
  return `@${quote}${value}${isDir ? "" : `${quote} `}`;
}
