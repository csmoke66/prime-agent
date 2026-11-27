// Browser side of the daemon wire protocol. The backend (`bh-web`) bridges one
// WebSocket to one daemon socket connection and forwards JSON frames verbatim, so this client
// speaks the same `beehive.daemon` JSONL protocol as the TUI: a `daemon_hello` frame first,
// `type: "command"` envelopes answered by `type: "response"` frames matched on id, and every other
// frame (session_event, roster_update, ...) delivered to subscribers.

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Frame = { type: string; [key: string]: any };
export type DaemonResponse = { type: "response"; id?: string; success: boolean; data?: any; error?: string; [key: string]: any };
export type CommandBody = { type: string; [key: string]: any };

export type ConnectionStatus = "connecting" | "ready" | "closed";

export class WireError extends Error {
  constructor(message: string, readonly response?: DaemonResponse) {
    super(message);
  }
}

const DEFAULT_TIMEOUT_MS = 30_000;
/** Commands bounded by the turn itself (TUI `LONG_RUNNING_REQUEST_TIMEOUT_MS`). */
const LONG_TIMEOUT_MS = 600_000;
const PROTOCOL_NAME = "beehive.daemon";

type Pending = { resolve: (r: DaemonResponse) => void; reject: (e: Error) => void; timer: number };

export class WireClient {
  hello: Frame | null = null;
  status: ConnectionStatus = "connecting";
  private socket: WebSocket | null = null;
  private pending = new Map<string, Pending>();
  private nextId = 0;
  private frameHandlers = new Set<(frame: Frame) => void>();
  private statusHandlers = new Set<(status: ConnectionStatus) => void>();
  private retryMs = 500;
  private closedByUser = false;
  private readonly clientId = `web:${Math.random().toString(36).slice(2, 10)}`;

  connect(): void {
    this.closedByUser = false;
    this.setStatus("connecting");
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${scheme}://${location.host}/ws`);
    this.socket = socket;
    socket.onmessage = (event) => this.onMessage(String(event.data));
    socket.onclose = () => this.onClose(socket);
  }

  close(): void {
    this.closedByUser = true;
    this.socket?.close();
  }

  onFrame(handler: (frame: Frame) => void): () => void {
    this.frameHandlers.add(handler);
    return () => this.frameHandlers.delete(handler);
  }

  onStatus(handler: (status: ConnectionStatus) => void): () => void {
    this.statusHandlers.add(handler);
    return () => this.statusHandlers.delete(handler);
  }

  /** Send a command and resolve with the daemon response (success or refusal). */
  request(command: CommandBody, timeoutMs?: number): Promise<DaemonResponse> {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN || !this.hello) {
      return Promise.reject(new WireError("not connected to the Beehive daemon"));
    }
    const id = `web_${++this.nextId}`;
    const version = Math.min(this.hello.protocol?.version ?? 7, 7);
    const envelope = {
      type: "command",
      id,
      protocol: { name: PROTOCOL_NAME, version },
      clientId: this.clientId,
      command,
    };
    const timeout =
      timeoutMs ?? (command.type === "prompt_and_wait" || command.type === "wait_for_idle" ? LONG_TIMEOUT_MS : DEFAULT_TIMEOUT_MS);
    return new Promise((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.pending.delete(id);
        reject(new WireError(`timed out after ${timeout}ms waiting for ${command.type}`));
      }, timeout);
      this.pending.set(id, { resolve, reject, timer });
      socket.send(JSON.stringify(envelope));
    });
  }

  /** Like `request`, but throws the daemon's error text on `success: false` and returns `data`. */
  async call<T = any>(command: CommandBody, timeoutMs?: number): Promise<T> {
    const response = await this.request(command, timeoutMs);
    if (!response.success) throw new WireError(response.error ?? `${command.type} failed`, response);
    return response.data as T;
  }

  private onMessage(text: string): void {
    let frame: Frame;
    try {
      frame = JSON.parse(text);
    } catch {
      return;
    }
    if (frame.type === "daemon_hello") {
      this.hello = frame;
      this.retryMs = 500;
      this.setStatus("ready");
    } else if (frame.type === "response") {
      const pending = this.pending.get(frame.id);
      if (pending) {
        window.clearTimeout(pending.timer);
        this.pending.delete(frame.id);
        pending.resolve(frame as DaemonResponse);
        return;
      }
    }
    for (const handler of this.frameHandlers) handler(frame);
  }

  private onClose(socket: WebSocket): void {
    if (this.socket !== socket) return;
    this.hello = null;
    for (const [id, pending] of this.pending) {
      window.clearTimeout(pending.timer);
      pending.reject(new WireError("the daemon connection closed"));
      this.pending.delete(id);
    }
    this.setStatus("closed");
    if (this.closedByUser) return;
    const delay = this.retryMs;
    this.retryMs = Math.min(this.retryMs * 2, 8000);
    window.setTimeout(() => this.connect(), delay);
  }

  private setStatus(status: ConnectionStatus): void {
    this.status = status;
    for (const handler of this.statusHandlers) handler(status);
  }
}

/** The one shared connection of the page. */
export const wire = new WireClient();
