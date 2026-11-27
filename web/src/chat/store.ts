import { create } from "zustand";
import { wire, WireError, type Frame } from "../wire";
import {
  applySessionEvent,
  emptySession,
  fromSnapshot,
  pushStatus,
  sequenceVerdict,
  REFRESH_STATE_EVENTS,
  type Msg,
  type SessionData,
  type SideTurn,
  type StatusKind,
} from "./model";

/** Capabilities the TUI sends on attach: slim payload, no image bytes, a windowed tail. */
const ATTACH_CAPABILITIES = ["attach_snapshot", "event_sequence", "slim_attach", "elide_snapshot_images", "windowed_snapshot"];
const PAGE_LIMIT = 100;

export type AttachStatus = "idle" | "attaching" | "ready" | "failed" | "closed";
/** Conversation detail, cycled by Ctrl+O: 0 overview, 1 thinking + diffs, 2 everything expanded. */
export type DetailLevel = 0 | 1 | 2;

export interface InfoPanel {
  title: string;
  /** Markdown body. */
  markdown?: string;
  /** Preformatted text body. */
  text?: string;
}

export interface Toast {
  id: number;
  text: string;
  kind: StatusKind;
}

interface ChatState {
  activeSessionId: string | null;
  attach: AttachStatus;
  attachError: string | null;
  data: SessionData;
  detail: DetailLevel;
  loadingOlder: boolean;
  models: any[] | null;
  configuredProviders: string[];
  commands: any[];
  side: SideTurn[];
  sideOpen: boolean;
  info: InfoPanel | null;
  toasts: Toast[];
  /** Text a queue action wants put back into the composer. */
  composerInsert: { text: string; nonce: number } | null;
}

export const useChat = create<ChatState>(() => ({
  activeSessionId: null,
  attach: "idle",
  attachError: null,
  data: emptySession(),
  detail: 0,
  loadingOlder: false,
  models: null,
  configuredProviders: [],
  commands: [],
  side: [],
  sideOpen: false,
  info: null,
  toasts: [],
  composerInsert: null,
}));

const set = useChat.setState;
const get = useChat.getState;

let generationToken = 0;
let toastId = 0;
let pendingFrames: Frame[] = [];
let snapshotBuffer: { id: string; head: any; chunks: Msg[][] } | null = null;
let stateRefreshTimer: number | null = null;

const patchData = (patch: (data: SessionData) => SessionData) => set((s) => ({ data: patch(s.data) }));

export function toast(text: string, kind: StatusKind = "info"): void {
  const id = ++toastId;
  set((s) => ({ toasts: [...s.toasts, { id, text, kind }] }));
  window.setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), kind === "error" ? 6000 : 2800);
}

/** A transcript status row (also used for command errors, like the TUI's `error_row`). */
export function note(text: string, kind: StatusKind = "info"): void {
  patchData((d) => pushStatus(d, text, kind));
}

export const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

function sid(): string {
  const id = get().activeSessionId;
  if (!id) throw new WireError("no session attached");
  return id;
}

/** Run a session-addressed command; failures become an error row. Returns the data or undefined. */
export async function command<T = any>(type: string, fields: Record<string, any> = {}, timeoutMs?: number): Promise<T | undefined> {
  try {
    return await wire.call<T>({ type, activeSessionId: sid(), ...fields }, timeoutMs);
  } catch (error) {
    note(errorText(error), "error");
    return undefined;
  }
}

// ---------- attach ----------

export async function attachSession(activeSessionId: string): Promise<void> {
  const token = ++generationToken;
  pendingFrames = [];
  snapshotBuffer = null;
  set({ activeSessionId, attach: "attaching", attachError: null, data: emptySession(), loadingOlder: false, side: [], sideOpen: false, info: null });
  try {
    const result = await wire.call({ type: "attach", activeSessionId, capabilities: ATTACH_CAPABILITIES });
    if (token !== generationToken) return;
    const snapshot = result?.snapshot ?? { state: result?.state, messages: result?.messages };
    const id = result?.activeSessionId ?? activeSessionId;
    const data = fromSnapshot(snapshot, { generation: result?.lastEventCursor?.generation, lastEventSequence: result?.lastEventSequence });
    set({ activeSessionId: id, attach: "ready", data });
    const queued = pendingFrames;
    pendingFrames = [];
    for (const frame of queued) handleFrame(frame);
    void refreshCommands();
  } catch (error) {
    if (token !== generationToken) return;
    set({ attach: "failed", attachError: errorText(error) });
  }
}

export function detachSession(): void {
  generationToken++;
  const id = get().activeSessionId;
  set({ attach: "idle", activeSessionId: null });
  if (id && wire.status === "ready") void wire.request({ type: "detach", activeSessionId: id }).catch(() => undefined);
}

function resync(): void {
  const id = get().activeSessionId;
  if (id) void attachSession(id);
}

export function refreshState(): void {
  if (stateRefreshTimer !== null) return;
  stateRefreshTimer = window.setTimeout(async () => {
    stateRefreshTimer = null;
    const id = get().activeSessionId;
    if (!id || get().attach !== "ready") return;
    try {
      // Context usage is not part of the connection state; like the TUI tray, re-read it from the stats.
      const [state, stats] = await Promise.all([
        wire.call({ type: "get_connection_state", activeSessionId: id }),
        wire.call<{ contextUsage?: unknown }>({ type: "get_session_stats", activeSessionId: id }).catch(() => null),
      ]);
      if (get().activeSessionId !== id) return;
      patchData((d) => ({ ...d, state: { ...d.state, ...(state ?? {}), ...(stats ? { contextUsage: stats.contextUsage ?? null } : {}) } }));
    } catch {
      /* the next event refreshes again */
    }
  }, 120);
}

export async function refreshCommands(): Promise<void> {
  try {
    const data = await wire.call({ type: "get_commands", activeSessionId: sid() });
    set({ commands: Array.isArray(data?.commands) ? data.commands : [] });
  } catch {
    set({ commands: [] });
  }
}

export async function loadOlder(): Promise<void> {
  const { data, loadingOlder } = get();
  const before = data.historyBeforeEntryId;
  if (!before || loadingOlder) return;
  set({ loadingOlder: true });
  try {
    const page = await wire.call({
      type: "get_messages",
      activeSessionId: sid(),
      page: { from: "end", before, limit: PAGE_LIMIT },
      capabilities: ATTACH_CAPABILITIES,
    });
    const older: Msg[] = page?.messages ?? [];
    patchData((d) => ({ ...d, messages: [...older, ...d.messages], historyBeforeEntryId: page?.more ? (page.firstEntryId ?? null) : null }));
  } catch (error) {
    note(errorText(error), "error");
  } finally {
    set({ loadingOlder: false });
  }
}

// ---------- frames ----------

function applySnapshot(head: any, messages: Msg[]): void {
  const data = fromSnapshot({ ...head, messages });
  set((s) => ({ data: { ...data, localSeq: s.data.localSeq }, attach: "ready" }));
}

export function handleFrame(frame: Frame): void {
  const { activeSessionId, attach } = get();
  if (!activeSessionId) return;
  const frameSession: string | undefined = frame.activeSessionId;
  if (frame.type === "session_binding" && frame.previousActiveSessionId === activeSessionId) {
    set({ activeSessionId: frame.activeSessionId });
    history.replaceState(null, "", `#/session/${encodeURIComponent(frame.activeSessionId)}`);
    return;
  }
  if (frameSession && frameSession !== activeSessionId) return;
  if (attach === "attaching") {
    if (frame.type === "session_event" || frame.type === "side_question_event") pendingFrames.push(frame);
    return;
  }
  switch (frame.type) {
    case "session_event": {
      const verdict = sequenceVerdict(get().data.lastSequence, frame.meta?.sequence);
      if (verdict === "replay") return;
      if (verdict === "gap") return resync();
      patchData((d) => {
        const next = applySessionEvent(d, frame.event);
        const sequence = frame.meta?.sequence;
        return sequence && sequence > next.lastSequence ? { ...next, lastSequence: sequence } : next;
      });
      if (REFRESH_STATE_EVENTS.has(frame.event?.type)) refreshState();
      if (frame.event?.type === "compaction_end" && !frame.event?.aborted) void refreshCommands();
      return;
    }
    case "side_question_event":
      return applySideEvent(frame.event);
    case "session_snapshot_begin":
      snapshotBuffer = { id: frame.snapshotId, head: frame, chunks: [] };
      return;
    case "session_snapshot_chunk":
    {
      const buffer = snapshotBuffer as { id: string; chunks: Msg[][] } | null;
      if (buffer && buffer.id === frame.snapshotId) buffer.chunks[frame.index] = frame.messages ?? [];
      return;
    }
    case "session_snapshot_end":
    {
      const buffer = snapshotBuffer as { id: string; head: any; chunks: Msg[][] } | null;
      if (buffer && buffer.id === frame.snapshotId) {
        const { head, chunks } = buffer;
        snapshotBuffer = null;
        applySnapshot({ ...head, lastEventSequence: frame.lastEventSequence, lastEventCursor: frame.lastEventCursor }, chunks.flat());
      }
      return;
    }
    case "session_snapshot_failed":
      snapshotBuffer = null;
      note(`Transcript transfer failed: ${frame.error}`, "error");
      return;
    case "session_resynced":
      return applySnapshot(frame.snapshot, frame.snapshot?.messages ?? []);
    case "session_replaced":
      if (frame.snapshotFollows) return;
      return applySnapshot({ state: frame.state, lastEventSequence: frame.meta?.sequence }, frame.messages ?? []);
    case "session_closed":
      note(`Session closed (${frame.reason})`, "warning");
      set({ attach: "closed" });
      return;
    case "session_detached":
      set({ attach: "closed" });
      return;
    default:
      return;
  }
}

// ---------- side question (/btw) ----------

function applySideEvent(event: any): void {
  const id = String(event?.id ?? "");
  set((s) => {
    const index = s.side.findIndex((t) => t.id === id);
    if (index < 0) return s;
    const turn: SideTurn = {
      id,
      question: event.question ?? s.side[index].question,
      answer: event.answer ?? s.side[index].answer,
      status: event.status ?? "running",
      errorMessage: event.errorMessage ?? undefined,
    };
    return { side: s.side.map((t, i) => (i === index ? turn : t)) };
  });
}

export async function startSideQuestion(question: string): Promise<void> {
  const { side } = get();
  if (side.some((t) => t.status === "running" && !t.local)) {
    note("Wait for the current side question to finish or cancel it first.", "warning");
    return;
  }
  const id = `side-${Date.now()}-${side.length + 1}`;
  const previous = side.filter((t) => !t.local && t.answer).map((t) => ({ question: t.question, answer: t.answer }));
  set({ side: [...side, { id, question, answer: "", status: "running" }], sideOpen: true });
  try {
    await wire.call({ type: "start_side_question", activeSessionId: sid(), sideQuestionId: id, question, ...(previous.length ? { previousTurns: previous } : {}) });
  } catch (error) {
    set((s) => ({ side: s.side.map((t) => (t.id === id ? { ...t, status: "error", errorMessage: errorText(error) } : t)) }));
  }
}

export async function closeSidePane(): Promise<void> {
  const running = get().side.find((t) => t.status === "running" && !t.local);
  set({ side: [], sideOpen: false });
  if (running) await wire.request({ type: "abort_side_question", activeSessionId: sid(), sideQuestionId: running.id }).catch(() => undefined);
}

// ---------- user actions ----------

export interface ImageAttachment {
  data: string;
  mimeType: string;
  name?: string;
}

export type SendMode = "steer" | "followUp";

export async function sendPrompt(message: string, mode: SendMode, images: ImageAttachment[] = []): Promise<boolean> {
  try {
    await wire.call({
      type: "prompt",
      activeSessionId: sid(),
      message,
      streamingBehavior: mode,
      queueIfBusy: true,
      ...(images.length ? { images: images.map((i) => ({ type: "image", data: i.data, mimeType: i.mimeType })) } : {}),
    });
    return true;
  } catch (error) {
    note(errorText(error), "error");
    return false;
  }
}

export const abort = () => command("abort");
export const abortAndSendQueued = () => command("abort_and_send_queued");
export const abortCompaction = () => command("abort_compaction");
export const abortRetry = () => command("abort_retry");
export const abortBash = () => command("abort_bash");
export const clearQueue = () => command("clear_queue");

export async function runBash(commandText: string, excludeFromContext: boolean): Promise<void> {
  await command("execute_bash", { command: commandText, excludeFromContext }, 600_000);
}

export async function mutateQueued(lane: "steering" | "followUp", index: number, expectedText: string, mutation: Record<string, any>): Promise<string | undefined> {
  const data = await command("mutate_queued_message", { lane, index, expectedText, mutation });
  const status = data?.status as string | undefined;
  if (status && status !== "applied") {
    toast(status === "invalid" ? "Edited text is not a valid session command" : status === "unsupported" ? "Queue editing requires a newer daemon" : "Queue changed; try again", "warning");
  }
  return status;
}

export function insertIntoComposer(text: string): void {
  set((s) => ({ composerInsert: { text, nonce: (s.composerInsert?.nonce ?? 0) + 1 } }));
}

export async function setModel(provider: string, modelId: string): Promise<void> {
  const ok = await command("set_model", { provider, modelId });
  if (ok !== undefined) refreshState();
}

export async function cycleModel(direction: "forward" | "backward" = "forward"): Promise<void> {
  const data = await command("cycle_model", { direction });
  if (data === null) toast("No other models available to cycle");
  refreshState();
}

export async function setThinking(level: string): Promise<void> {
  await command("set_thinking_level", { level });
  refreshState();
}

export async function cycleThinking(): Promise<void> {
  await command("cycle_thinking_level");
  refreshState();
}

export async function setServiceTier(tier: string | null): Promise<void> {
  await command("set_service_tier", { serviceTier: tier });
  refreshState();
}

export async function setSessionOption(type: "set_transport" | "set_steering_mode" | "set_follow_up_mode" | "set_auto_compaction" | "set_auto_retry", fields: Record<string, any>): Promise<void> {
  await command(type, fields);
  refreshState();
}

export async function loadModels(force = false): Promise<void> {
  if (get().models && !force) return;
  try {
    const data = await wire.call({ type: "get_model_catalog", activeSessionId: sid() });
    set({ models: data?.models ?? [], configuredProviders: data?.configuredProviders ?? [] });
  } catch (error) {
    note(errorText(error), "error");
  }
}

export const cycleDetail = () => set((s) => ({ detail: ((s.detail + 1) % 3) as DetailLevel }));
export const openInfo = (info: InfoPanel | null) => set({ info });
