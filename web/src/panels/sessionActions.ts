import { wire, WireError } from "../wire";
import { navigate } from "../app/router";

export type CreateOptions = {
  cwd: string;
  name?: string;
  provider?: string;
  model?: string;
  thinking?: string;
  sessionPath?: string;
  parentSessionPath?: string;
  rlmDepth?: number;
};

/** Create (or resume with `sessionPath`) a session and return its active session id. */
export async function createSession(options: CreateOptions): Promise<string> {
  const config: Record<string, unknown> = { cwd: options.cwd, executionMode: "interactive" };
  if (options.provider) config.provider = options.provider;
  if (options.model) config.model = options.model;
  if (options.thinking) config.thinking = options.thinking;
  if (options.parentSessionPath) {
    config.parentSessionPath = options.parentSessionPath;
    config.rlmDepth = options.rlmDepth ?? 1;
  }
  try {
    const data = await wire.call({ type: "create", sessionPath: options.sessionPath, name: options.name || undefined, config });
    const id = data?.activeSessionId ?? data?.id;
    if (!id) throw new Error("the daemon did not report a session id for the new session");
    return id;
  } catch (error) {
    // A session already attached elsewhere: open that live session instead of failing.
    const info = error instanceof WireError ? error.response?.errorInfo : undefined;
    if (info?.code === "session_already_active" && info.activeSessionId) return info.activeSessionId as string;
    throw error;
  }
}

export async function openSession(activeSessionId: string): Promise<void> {
  navigate({ name: "session", activeSessionId });
}

/** Resume a saved session file (its recorded cwd) and open it. */
export async function resumeSaved(saved: { path: string; cwd: string }): Promise<void> {
  const id = await createSession({ cwd: saved.cwd, sessionPath: saved.path });
  await openSession(id);
}

export async function startNewSession(options: CreateOptions & { prompt?: string }): Promise<void> {
  const id = await createSession(options);
  if (options.prompt?.trim()) await wire.call({ type: "prompt", activeSessionId: id, message: options.prompt, input: {} });
  await openSession(id);
}

export const renameLive = (activeSessionId: string, name: string) => wire.call({ type: "rename", activeSessionId, name });
export const renameSaved = (sessionPath: string, name: string) => wire.call({ type: "rename_saved_session", sessionPath, name });
export const deleteSaved = (sessionPath: string) => wire.call({ type: "delete_saved_session", sessionPath });
export const killSession = (activeSessionId: string) => wire.call({ type: "kill", activeSessionId });
