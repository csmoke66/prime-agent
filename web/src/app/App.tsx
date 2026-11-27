import { useEffect } from "react";
import { wire } from "../wire";
import { useConnectionStatus } from "../wire/useWire";
import { useRoute, navigate } from "./router";
import { SessionPicker } from "../panels/SessionPicker";
import { SessionView } from "../chat/SessionView";
import { AgentsView } from "../agents/AgentsView";

export function App() {
  const status = useConnectionStatus();
  const route = useRoute();
  useEffect(() => {
    wire.connect();
    return () => wire.close();
  }, []);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-4 border-b border-grid/40 bg-surface px-4 py-2">
        <button className="flex items-center gap-2 font-semibold tracking-tight" onClick={() => navigate({ name: "sessions" })}>
          <span className="text-primary-soft">&#x2B22;</span> Beehive
        </button>
        <nav className="flex gap-1 text-sm text-muted">
          <button className="rounded px-2 py-1 hover:bg-selected" onClick={() => navigate({ name: "sessions" })}>Sessions</button>
          <button className="rounded px-2 py-1 hover:bg-selected" onClick={() => navigate({ name: "agents" })}>Agents</button>
        </nav>
        <span className="ml-auto text-xs text-dim">
          <span className={status === "ready" ? "text-success" : "text-warning bh-pulse"}>&#x25CF;</span> {status}
        </span>
      </header>
      <main className="min-h-0 flex-1">
        {status !== "ready" ? (
          <div className="grid h-full place-items-center text-muted">Connecting to the Beehive daemon&hellip;</div>
        ) : route.name === "session" ? (
          <SessionView activeSessionId={route.activeSessionId} />
        ) : route.name === "agents" ? (
          <AgentsView />
        ) : (
          <SessionPicker />
        )}
      </main>
    </div>
  );
}
