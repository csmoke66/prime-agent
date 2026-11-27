import { useState } from "react";
import { Modal } from "./ui/Modal";
import { Badge, Empty, ErrorLine, SearchInput, Spinner } from "./ui/bits";
import { fuzzyFilter } from "./ui/format";
import { rest, type AuthProvider } from "./api";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";

/** `/login` and `/logout`: provider credentials. API keys are entered here; browser OAuth is limited. */
export function AuthPanel({ mode, onboarding, onClose }: { mode: "login" | "logout"; onboarding?: boolean; onClose: () => void }) {
  const providers = useAsync(() => rest.authProviders(), []);
  const [search, setSearch] = useState("");
  const [picked, setPicked] = useState<AuthProvider | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");

  const list = (providers.data?.providers ?? []).filter((p) => (mode === "login" ? p.available : p.configured));
  const shown = fuzzyFilter(list, search, (p) => `${p.name} ${p.id}`);

  const run = async (provider: AuthProvider, apiKey?: string) => {
    setBusy(true);
    setError("");
    try {
      const result = mode === "login" ? await rest.authLogin(provider.id, apiKey) : await rest.authLogout(provider.id);
      const authUrl = "authUrl" in result ? (result.authUrl as string | undefined) : undefined;
      if (authUrl) {
        window.open(authUrl, "_blank", "noopener");
        setNote("Finish signing in in the opened tab, then refresh this list.");
      } else {
        toast.success(result.status || (mode === "login" ? "Signed in" : "Signed out"));
        setPicked(null);
        setKey("");
        providers.reload();
      }
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title={onboarding ? "Welcome to Beehive" : mode === "login" ? "Sign in to a provider" : "Remove provider authentication"} subtitle={onboarding ? "Connect a model provider to get started. You can change this later with /login." : undefined} size="md" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search providers…" autoFocus />
        <ErrorLine>{providers.error || error}</ErrorLine>
        {note && <p className="text-sm text-info">{note}</p>}
        {providers.loading && <Spinner />}
        {!providers.loading && shown.length === 0 && !providers.error && <Empty title="No providers" />}
        {shown.map((p) => (
          <div key={p.id} className="bh-card flex flex-col gap-2 px-3 py-2.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="min-w-0 flex-1 text-sm font-medium">{p.name}</span>
              <Badge>{p.authType === "oauth" ? "subscription" : "api key"}</Badge>
              {p.configured && <Badge tone="success">{p.status ?? "configured"}</Badge>}
              {mode === "logout" || p.authType === "oauth" ? (
                <button className="bh-btn" disabled={busy} onClick={() => void run(p)}>{mode === "logout" ? "Remove" : "Sign in"}</button>
              ) : (
                <button className="bh-btn" onClick={() => setPicked(picked?.id === p.id ? null : p)}>{p.configured ? "Replace key" : "Add key"}</button>
              )}
            </div>
            {picked?.id === p.id && (
              <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (key.trim()) void run(p, key.trim()); }}>
                <input className="bh-input font-mono" type="password" autoComplete="off" autoFocus placeholder="API key" value={key} onChange={(e) => setKey(e.target.value)} />
                <button className="bh-btn bh-btn-primary" disabled={busy || !key.trim()}>Save</button>
              </form>
            )}
          </div>
        ))}
        {mode === "login" && <p className="text-xs text-dim">Subscription logins that need a local browser are limited in the web UI; use the terminal for those.</p>}
      </div>
    </Modal>
  );
}
