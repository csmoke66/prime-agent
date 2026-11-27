import { useCallback, useState } from "react";
import { Modal } from "./ui/Modal";
import { Badge, Empty, ErrorLine, SearchInput, Spinner } from "./ui/bits";
import { fuzzyFilter } from "./ui/format";
import { rest, type AuthProvider, type FlowRequest } from "./api";
import { FlowRunner } from "./FlowRunner";
import { useAsync } from "./useAsync";
import { toast } from "./toasts";

type Active = { provider: AuthProvider; request: FlowRequest };

/** `/login` and `/logout`: API keys, subscription (OAuth) sign-in with the sign-in link and code paste, and removal. */
export function AuthPanel({ mode, onboarding, onClose }: { mode: "login" | "logout"; onboarding?: boolean; onClose: () => void }) {
  const providers = useAsync(() => rest.authProviders(mode), [mode]);
  const [search, setSearch] = useState("");
  const [keyFor, setKeyFor] = useState<AuthProvider | null>(null);
  const [key, setKey] = useState("");
  const [active, setActive] = useState<Active | null>(null);
  const [busy, setBusy] = useState(false);

  const list = (providers.data?.providers ?? []).filter((p) => (mode === "login" ? p.available : p.configured));
  const shown = fuzzyFilter(list, search, (p) => `${p.name} ${p.id}`);

  const begin = (provider: AuthProvider, request: FlowRequest) => {
    setKeyFor(null);
    setKey("");
    setBusy(true);
    setActive({ provider, request });
  };
  const finished = useCallback(
    (outcome: { kind: string; message?: string }) => {
      if (outcome.kind === "status") toast.success(outcome.message || "Done");
      setBusy(false);
      providers.reload();
    },
    [providers.reload],
  );

  return (
    <Modal title={onboarding ? "Welcome to Beehive" : mode === "login" ? "Sign in to a provider" : "Remove provider authentication"} subtitle={onboarding ? "Connect a model provider to get started. You can change this later with /login." : undefined} size="md" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search providers…" autoFocus />
        <ErrorLine>{providers.error}</ErrorLine>
        {providers.loading && !providers.data && <Spinner />}
        {!providers.loading && shown.length === 0 && !providers.error && <Empty title="No providers" />}
        {shown.map((p) => (
          <div key={p.id} className="bh-card flex flex-col gap-2 px-3 py-2.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="min-w-0 flex-1 text-sm font-medium">{p.name}</span>
              <Badge>{p.authType === "oauth" ? "subscription" : "api key"}</Badge>
              {p.configured && <Badge tone="success">{p.status ?? "configured"}</Badge>}
              {mode === "logout" ? (
                <button className="bh-btn" disabled={busy} onClick={() => begin(p, { kind: "providerLogout", provider: p.id })}>Remove</button>
              ) : p.authType === "oauth" ? (
                <button className="bh-btn" disabled={busy} onClick={() => begin(p, { kind: "providerLogin", provider: p.id })}>Sign in</button>
              ) : (
                <button className="bh-btn" disabled={busy} onClick={() => setKeyFor(keyFor?.id === p.id ? null : p)}>{p.configured ? "Replace key" : "Add key"}</button>
              )}
            </div>
            {keyFor?.id === p.id && (
              <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (key.trim()) begin(p, { kind: "providerKey", provider: p.id, key: key.trim() }); }}>
                <input className="bh-input font-mono" type="password" autoComplete="off" autoFocus placeholder="API key" value={key} onChange={(e) => setKey(e.target.value)} />
                <button className="bh-btn bh-btn-primary" disabled={!key.trim()}>Save</button>
              </form>
            )}
            {active?.provider.id === p.id && <FlowRunner request={active.request} onFinished={finished} />}
          </div>
        ))}
      </div>
    </Modal>
  );
}
