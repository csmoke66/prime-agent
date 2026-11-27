import { useCallback, useState } from "react";
import { wire } from "../wire";
import { Modal } from "./ui/Modal";
import { Badge, ConfirmButton, Empty, ErrorLine, SearchInput, Spinner, type Tone } from "./ui/bits";
import { fuzzyFilter } from "./ui/format";
import { type FlowRequest } from "./api";
import { FlowRunner } from "./FlowRunner";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";

type Service = {
  serviceId: string; label: string; connectionStatus: string; connectable?: boolean; loginPending?: boolean; usesOAuth?: boolean;
  pasteToken?: boolean; description?: string; category?: string; toolCount?: number; setupHint?: string; docsUrl?: string; source?: string;
};
type Credential = { id: string; label: string; configured: boolean };
type Data = { services?: Service[]; credentials?: Credential[]; connections?: { server: string; label?: string; connected?: boolean }[] };

const STATUS: Record<string, { tone: Tone; text: (s: Service) => string }> = {
  connected: { tone: "success", text: (s) => (s.toolCount !== undefined ? `Connected · ${s.toolCount} tools` : "Connected") },
  pending: { tone: "warning", text: () => "Needs verification" },
  error: { tone: "error", text: (s) => (s.connectable ? "Reconnect" : "Needs attention") },
  setup_required: { tone: "warning", text: () => "Requires setup" },
  disabled: { tone: "neutral", text: () => "Disabled" },
};

/** `/mcp` and `/plugins`: browse the service catalog, connect or disconnect, store API keys. */
export function McpPanel({ activeSessionId, initialSearch, onClose }: { activeSessionId: string; initialSearch: string; onClose: () => void }) {
  const [search, setSearch] = useState(initialSearch);
  const data = useAsync(() => wire.call<Data>({ type: "get_mcp_connections", activeSessionId }), [activeSessionId]);
  const [busy, setBusy] = useState("");
  const [flow, setFlow] = useState<{ request: FlowRequest; label: string } | null>(null);
  const [error, setError] = useState("");

  const services: Service[] =
    data.data?.services ?? (data.data?.connections ?? []).map((c) => ({ serviceId: c.server, label: c.label ?? c.server, connectionStatus: c.connected ? "connected" : "not_connected", connectable: !c.connected }));
  const credentials = data.data?.credentials ?? [];
  const shown = fuzzyFilter(services, search, (s) => `${s.label} ${s.serviceId} ${s.category ?? ""} ${s.description ?? ""}`);

  const run = async (id: string, action: () => Promise<string | void>) => {
    setBusy(id);
    setError("");
    try {
      const status = await action();
      if (status) toast.info(status);
      data.reload();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy("");
    }
  };

  const flowDone = useCallback(
    (outcome: { kind: string; message?: string }) => {
      if (outcome.kind === "status") toast.success(outcome.message ?? "Done");
      data.reload();
    },
    [data.reload],
  );

  return (
    <Modal title="Services & MCP" subtitle="Connect external services for agents to use." size="lg" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search services…" autoFocus />
        <ErrorLine>{data.error || error}</ErrorLine>
        {data.loading && <Spinner label="Loading services" />}
        {flow && (
          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between text-sm font-medium">{flow.label}<button className="bh-btn" onClick={() => setFlow(null)}>Close</button></div>
            <FlowRunner key={JSON.stringify(flow.request)} request={flow.request} onFinished={flowDone} />
          </div>
        )}
        {shown.map((s) => {
          const status = STATUS[s.connectionStatus];
          const connected = s.connectionStatus === "connected";
          return (
            <div key={s.serviceId} className="bh-card flex flex-wrap items-center gap-3 px-3 py-2.5">
              <div className="min-w-48 flex-1">
                <div className="flex items-center gap-2 text-sm font-medium">{s.label || s.serviceId}{s.category && <Badge>{s.category}</Badge>}</div>
                {s.description && <p className="line-clamp-2 text-xs text-muted">{s.description}</p>}
                {s.setupHint && !connected && <p className="line-clamp-1 text-xs text-dim">{s.setupHint}</p>}
              </div>
              <Badge tone={s.loginPending ? "warning" : status?.tone ?? "neutral"}>{s.loginPending ? "Login in progress" : status?.text(s) ?? "Not connected"}</Badge>
              <div className="flex gap-1">
                {connected && (
                  <ConfirmButton label="Disconnect" confirmLabel="Disconnect?" disabled={busy === s.serviceId} onConfirm={() => void run(s.serviceId, async () => void (await wire.call({ type: "remove_mcp_connection", activeSessionId, server: s.serviceId })))} />
                )}
                {!connected && s.pasteToken && <button className="bh-btn" onClick={() => setFlow({ request: { kind: "mcpPaste", server: s.serviceId }, label: s.label || s.serviceId })}>Paste token</button>}
                {!connected && !s.pasteToken && s.connectable !== false && <button className="bh-btn bh-btn-primary" onClick={() => setFlow({ request: { kind: "mcpLogin", server: s.serviceId }, label: s.label || s.serviceId })}>Connect</button>}
                {s.docsUrl && <a className="bh-btn" href={s.docsUrl} target="_blank" rel="noreferrer">Docs</a>}
              </div>
            </div>
          );
        })}
        {!data.loading && shown.length === 0 && <Empty title="No services match" />}
        {credentials.length > 0 && (
          <>
            <h3 className="mt-2 text-xs font-medium uppercase tracking-wider text-dim">API keys</h3>
            {credentials.map((c) => (
              <div key={c.id} className="bh-card flex items-center gap-3 px-3 py-2.5">
                <span className="flex-1 text-sm font-medium">{c.label || c.id}</span>
                <Badge tone={c.configured ? "success" : "neutral"}>{c.configured ? "Configured" : "Not configured"}</Badge>
                <button className="bh-btn" onClick={() => setFlow({ request: { kind: "mcpKey", credential: c.id }, label: c.label || c.id })}>{c.configured ? "Replace key" : "Add key"}</button>
              </div>
            ))}
          </>
        )}
      </div>
    </Modal>
  );
}
