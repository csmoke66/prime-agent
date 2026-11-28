import { useCallback, useState } from "react";
import { Modal } from "./ui/Modal";
import { Badge, Empty, ErrorLine, SearchInput, Spinner } from "./ui/bits";
import { fuzzyFilter } from "./ui/format";
import { rest, type AuthProvider, type FlowRequest } from "./api";
import { FlowRunner } from "./FlowRunner";
import { useAsync } from "./useAsync";
import { toast } from "./toasts";

type Active = { provider: AuthProvider; request: FlowRequest };

/** Providers (the TUI's `/login` and `/logout` in one list): API keys, subscription (OAuth) sign-in with the
 * sign-in link and code paste, and removal of a configured provider. */
export function AuthPanel({ onboarding, onClose }: { onboarding?: boolean; onClose: () => void }) {
  const providers = useAsync(async () => {
    const [login, logout] = await Promise.all([rest.authProviders("login"), rest.authProviders("logout")]);
    const removable = new Set(logout.providers.filter((p) => p.configured).map((p) => p.id));
    const rows = login.providers.filter((p) => p.available || p.configured);
    for (const p of logout.providers) if (p.configured && !rows.some((r) => r.id === p.id)) rows.push(p);
    return { providers: rows.map((p) => ({ ...p, removable: removable.has(p.id) })) };
  }, []);
  const [search, setSearch] = useState("");
  const [keyFor, setKeyFor] = useState<AuthProvider | null>(null);
  const [key, setKey] = useState("");
  const [active, setActive] = useState<Active | null>(null);
  const [busy, setBusy] = useState(false);

  // Configured providers first, then the rest by name.
  const list = [...(providers.data?.providers ?? [])].sort((a, b) => Number(b.configured) - Number(a.configured) || a.name.localeCompare(b.name));
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
    <Modal title={onboarding ? "Welcome to Beehive" : "Providers"} subtitle={onboarding ? "Connect a model provider to get started. You can change this later from Providers on the home page." : "Sign in with a subscription or add an API key."} size="md" onClose={onClose}>
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
              {p.authType === "oauth" ? (
                <button className="bh-btn" disabled={busy} onClick={() => begin(p, { kind: "providerLogin", provider: p.id })}>{p.configured ? "Sign in again" : "Sign in"}</button>
              ) : (
                <button className="bh-btn" disabled={busy} onClick={() => setKeyFor(keyFor?.id === p.id ? null : p)}>{p.configured ? "Replace key" : "Add key"}</button>
              )}
              {p.removable && (
                <button className="bh-btn hover:text-error" disabled={busy} onClick={() => { if (window.confirm(`Remove the sign-in for ${p.name}?`)) begin(p, { kind: "providerLogout", provider: p.id }); }}>Remove</button>
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
