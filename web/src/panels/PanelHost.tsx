import { useEffect } from "react";
import { wire } from "../wire";
import { navigate, parseRoute } from "../app/router";
import { usePanel, openPanel, type PanelName } from "./store";
import { useToasts, toast, errorText } from "./toasts";
import { SLASH_COMMANDS, resolveSlash } from "./slashCommands";
import { NewSessionDialog } from "./NewSessionDialog";
import { TreePanel, ForkPanel, cloneSession } from "./SessionTree";
import { SettingsPanel } from "./SettingsPanel";
import { McpPanel } from "./McpPanel";
import { HarnessPanel } from "./HarnessPanel";
import { AuthPanel } from "./AuthPanel";
import { HotkeysPanel, ChangelogPanel, LogsPanel } from "./InfoPanels";
import { ExportPanel, ImportPanel, SharePanel, exportToPath, importFromPath } from "./SharePanels";
import { resetKeybindings } from "../app/keybindings";
import { ContextEditPanel } from "./ContextEditPanel";
import { HeartbeatsPanel } from "./HeartbeatsPanel";
import { WorkflowsPanel } from "./WorkflowsPanel";
import { InlineRename } from "./InlineRename";
import { Modal } from "./ui/Modal";
import { createSession, renameLive } from "./sessionActions";
import { rest } from "./api";
import { useAsync } from "./useAsync";

const NEEDS_SESSION = new Set<PanelName>(["tree", "fork", "edit_context", "export", "share", "workflows", "rename"]);
const currentSession = (): string | undefined => {
  const route = parseRoute(location.hash);
  return route.name === "session" ? route.activeSessionId : undefined;
};

/** Run one slash command that belongs to the panels (called for `bh:slash` and by the palette). */
function runPanelSlash(name: string, args: string, activeSessionId?: string): void {
  const session = activeSessionId ?? currentSession();
  switch (name) {
    case "clone":
      if (!session) return void toast.info("Open a session first");
      void cloneSession(session);
      return;
    case "resume":
      if (!args.trim()) return navigate({ name: "sessions" });
      void resumeBySelector(args.trim());
      return;
    case "export":
      if (!session) return void toast.info("Open a session first");
      if (!args.trim()) return openPanel("export", { args, activeSessionId: session });
      void exportToPath(session, args.trim()).catch((e) => toast.error(errorText(e)));
      return;
    case "import":
      if (!args.trim()) return openPanel("import", { args });
      void importFromPath(args.trim()).catch((e) => toast.error(errorText(e)));
      return;
    case "reload":
      if (!session) return void toast.info("Open a session first");
      resetKeybindings();
      wire.call({ type: "reload", activeSessionId: session }).then(() => toast.success("Reloaded keybindings, skills, prompts"), (e) => toast.error(`Reload failed: ${errorText(e)}`));
      return;
    case "name":
      if (!session) return void toast.info("Open a session first");
      if (args.trim()) void renameLive(session, args.trim()).then(() => toast.success("Session renamed"), (e) => toast.error(errorText(e)));
      else openPanel("rename", { activeSessionId: session });
      return;
    case "heartbeat":
      return openPanel("heartbeats", { args, activeSessionId: session });
    case "run":
      if (!session) return void toast.info("Open a session first");
      return openPanel("workflows", { args, activeSessionId: session });
    default: {
      const panel = name as PanelName;
      if (NEEDS_SESSION.has(panel) && !session) return void toast.info("Open a session first");
      openPanel(panel, { args, activeSessionId: session });
    }
  }
}

async function resumeBySelector(selector: string): Promise<void> {
  try {
    if (selector.includes("/") || selector.endsWith(".jsonl")) {
      const info = await rest.info().catch(() => undefined);
      navigate({ name: "session", activeSessionId: await createSession({ cwd: info?.cwd ?? "/", sessionPath: selector }) });
    } else navigate({ name: "session", activeSessionId: selector });
  } catch (error) {
    toast.error(errorText(error));
  }
}

/** Entry point for the palette: panel commands open here, the rest go to the chat view. */
export function dispatchSlash(name: string, args: string, activeSessionId?: string): void {
  const command = resolveSlash(name);
  if (command?.owner === "panel") runPanelSlash(command.name, args, activeSessionId);
  else window.dispatchEvent(new CustomEvent("bh:run-slash", { detail: { name: command?.name ?? name, args, activeSessionId: activeSessionId ?? currentSession() } }));
}

const PANEL_SLASH = new Set(SLASH_COMMANDS.filter((c) => c.owner === "panel").flatMap((c) => [c.name, ...(c.aliases ?? [])]));

export function PanelHost() {
  const request = usePanel((s) => s.open);
  const close = usePanel((s) => s.close);

  useEffect(() => {
    const onSlash = (event: Event) => {
      const { name, args, activeSessionId } = (event as CustomEvent).detail ?? {};
      if (!PANEL_SLASH.has(name)) return;
      runPanelSlash(resolveSlash(name)!.name, args ?? "", activeSessionId);
    };
    window.addEventListener("bh:slash", onSlash);
    return () => window.removeEventListener("bh:slash", onSlash);
  }, []);

  return (
    <>
      {request && <ActivePanel name={request.name} args={request.args} activeSessionId={request.activeSessionId} onClose={close} />}
      <Onboarding />
      <Toaster />
    </>
  );
}

/** Panels that read daemon state need some session to address; borrow any live one when none is open. */
function useAnySession(preferred?: string): string | undefined {
  const found = useAsync(async () => {
    if (preferred) return preferred;
    const data = await wire.call<{ sessions?: { id: string }[] }>({ type: "list" });
    return data?.sessions?.[0]?.id;
  }, [preferred]);
  return found.data;
}

function ActivePanel({ name, args, activeSessionId, onClose }: { name: PanelName; args: string; activeSessionId?: string; onClose: () => void }) {
  const session = useAnySession(activeSessionId);
  switch (name) {
    case "new": return <NewSessionDialog args={args} onClose={onClose} />;
    case "tree": return <TreePanel activeSessionId={activeSessionId!} onClose={onClose} />;
    case "fork": return <ForkPanel activeSessionId={activeSessionId!} onClose={onClose} />;
    case "clone": return null;
    case "settings": return <SettingsPanel activeSessionId={activeSessionId} onClose={onClose} />;
    case "mcp":
    case "plugins":
      return session ? <McpPanel activeSessionId={session} initialSearch={name === "plugins" ? args : ""} onClose={onClose} /> : <NeedsSession onClose={onClose} />;
    case "harness": return <HarnessHost activeSessionId={activeSessionId} onClose={onClose} />;
    case "login":
    case "logout": return <AuthPanel mode={name} onClose={onClose} />;
    case "hotkeys": return <HotkeysPanel onClose={onClose} />;
    case "changelog": return <ChangelogPanel onClose={onClose} />;
    case "logs": return <LogsPanel onClose={onClose} />;
    case "export": return <ExportPanel activeSessionId={activeSessionId!} onClose={onClose} />;
    case "edit_context": return <ContextEditPanel activeSessionId={activeSessionId!} onClose={onClose} />;
    case "import": return <ImportPanel onClose={onClose} />;
    case "share": return <SharePanel activeSessionId={activeSessionId!} onClose={onClose} />;
    case "heartbeats": return <HeartbeatsPanel activeSessionId={activeSessionId} onClose={onClose} />;
    case "workflows": return <WorkflowsPanel activeSessionId={activeSessionId!} initial={args} onClose={onClose} />;
    case "rename": return <RenameDialog activeSessionId={activeSessionId!} onClose={onClose} />;
    case "onboarding": return <AuthPanel mode="login" onboarding onClose={() => { void markOnboardingDone(); onClose(); }} />;
    case "messages": return null;
  }
}

function NeedsSession({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="No session available" size="sm" onClose={onClose}>
      <p className="text-sm text-muted">This panel reads state through a running session. Start or open one first.</p>
    </Modal>
  );
}

function HarnessHost({ activeSessionId, onClose }: { activeSessionId?: string; onClose: () => void }) {
  const file = useAsync(async () => {
    if (!activeSessionId) return "";
    const state = await wire.call<{ sessionFile?: string }>({ type: "get_state", activeSessionId });
    return state?.sessionFile ?? "";
  }, [activeSessionId]);
  if (file.loading) return null;
  return <HarnessPanel sessionFile={file.data || undefined} onClose={onClose} />;
}

function RenameDialog({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  return (
    <Modal title="Rename session" size="sm" onClose={onClose}>
      <InlineRename initial="" onCancel={onClose} onSave={(name) => void renameLive(activeSessionId, name).then(onClose, (e) => toast.error(errorText(e)))} />
    </Modal>
  );
}

/** Like the TUI, a finished onboarding (a provider is now configured) is remembered in `settings.json`; leaving early is not. */
async function markOnboardingDone(): Promise<void> {
  const { providers } = await rest.authProviders("login");
  if (providers.some((p) => p.configured)) await rest.saveSettings({ onboardingShown: true });
}

/** First-run guide: shown when onboarding never finished and no provider is configured; once per tab. */
function Onboarding() {
  useEffect(() => {
    if (sessionStorage.getItem("bh.onboardingShown") === "1") return;
    let live = true;
    Promise.all([rest.settings(), rest.authProviders("login")]).then(
      ([settings, { providers }]) => {
        if (!live || settings.onboardingShown === true || providers.some((p) => p.configured)) return;
        sessionStorage.setItem("bh.onboardingShown", "1");
        openPanel("onboarding");
      },
      () => undefined,
    );
    return () => { live = false; };
  }, []);
  return null;
}

function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[70] flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => (
        <button key={t.id} onClick={() => dismiss(t.id)} className={`bh-fade-in pointer-events-auto rounded-xl border px-3 py-2 text-left text-sm shadow-lg ${t.tone === "error" ? "border-error/50 bg-tool-error text-error" : t.tone === "success" ? "border-success/50 bg-tool-success" : "border-line bg-panel"}`}>
          {t.text}
        </button>
      ))}
    </div>
  );
}
