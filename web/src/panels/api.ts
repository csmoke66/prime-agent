// REST endpoints of `bh-web` that carry client-side state the daemon wire does not. Each helper
// reports a missing endpoint (404/501) as `BackendUnavailable` so panels can degrade with a clear note.
export class BackendUnavailable extends Error {
  constructor(readonly endpoint: string) {
    super(`The web backend does not provide ${endpoint} yet`);
  }
}

async function send<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, { credentials: "same-origin", ...init });
  if (response.status === 404 || response.status === 501) throw new BackendUnavailable(`/api${path.split("?")[0]}`);
  const text = await response.text();
  if (!response.ok) {
    let message = text;
    try {
      const parsed = JSON.parse(text);
      message = parsed.error ?? parsed.message ?? text;
    } catch {
      /* plain text error */
    }
    throw new Error(message || response.statusText);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

const json = (method: string, body: unknown): RequestInit => ({ method, headers: { "content-type": "application/json" }, body: JSON.stringify(body) });

export type Settings = Record<string, any>;
export type AuthProvider = { id: string; name: string; authType: "oauth" | "apiKey"; configured: boolean; available: boolean; status?: string };
export type LogFile = { name: string; size: number };
export type HarnessEntry = { scope: "local" | "global"; kind: string; id: string; title?: string; content?: string; path?: string; [key: string]: any };

export const rest = {
  info: () => send<{ version: string; cwd: string; agentDir: string; socketPath: string }>("/info"),
  settings: () => send<Settings>("/settings"),
  saveSettings: (settings: Settings) => send<Settings>("/settings", json("PUT", settings)),
  files: (cwd: string, prefix: string) => send<{ entries: { path: string; isDir: boolean }[] } | string[]>(`/files?cwd=${encodeURIComponent(cwd)}&prefix=${encodeURIComponent(prefix)}`),
  changelog: () => send<{ markdown: string }>("/changelog"),
  logs: () => send<{ dir: string; files: LogFile[] }>("/logs"),
  logTail: async (name: string, tail = 200) => {
    const response = await fetch(`/api/logs/${encodeURIComponent(name)}?tail=${tail}`, { credentials: "same-origin" });
    if (!response.ok) throw new Error(await response.text());
    return response.text();
  },
  keybindings: () => send<{ bindings: Record<string, string[]> }>("/keybindings"),
  authProviders: () => send<{ providers: AuthProvider[] }>("/auth/providers"),
  authLogin: (provider: string, apiKey?: string) => send<{ status: string; authUrl?: string }>("/auth/login", json("POST", { provider, apiKey })),
  authLogout: (provider: string) => send<{ status: string }>("/auth/logout", json("POST", { provider })),
  mcpAuth: (action: "login" | "key" | "logout", target: string, key?: string) => send<{ status: string; authUrl?: string }>("/mcp/auth", json("POST", { action, target, key })),
  harness: (sessionFile?: string) => send<{ entries: HarnessEntry[] }>(`/harness?sessionFile=${encodeURIComponent(sessionFile ?? "")}`),
  deleteHarness: (entry: HarnessEntry, sessionFile?: string) => send<{ ok: boolean }>("/harness", json("DELETE", { sessionFile, entry })),
  share: (activeSessionId: string) => send<{ previewUrl: string; gistUrl: string }>("/share", json("POST", { activeSessionId })),
  upload: (file: File) => send<{ path: string }>("/upload", { method: "POST", body: file }),
  exportUrl: (activeSessionId: string, format: "html" | "jsonl") => `/api/export?activeSessionId=${encodeURIComponent(activeSessionId)}&format=${format}`,
};
