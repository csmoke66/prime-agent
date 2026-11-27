import { useEffect, useState } from "react";
import { wire } from "../wire";
import { useConnectionStatus } from "../wire/useWire";
import { useRoute, navigate } from "./router";
import { SessionView } from "../chat/SessionView";
import { Inspector } from "../agents/Inspector";
import { PanelHost } from "../panels/PanelHost";
import { CommandPalette } from "./CommandPalette";
import { Home } from "./Home";
import { ResizeHandle } from "./ResizeHandle";
import { Sidebar } from "./Sidebar";
import { clampWidth, setLayout, useLayout } from "./layout";
import { usePalette, useGlobalShortcuts } from "./shortcuts";
import { IconInspector, IconSearch, IconSidebar, Logo } from "./icons";
import { useChat } from "../chat/store";
import { loadKeybindings, matches } from "./keybindings";

const useNarrow = () => {
  const [narrow, setNarrow] = useState(() => window.matchMedia("(max-width: 767px)").matches);
  useEffect(() => {
    const query = window.matchMedia("(max-width: 767px)");
    const onChange = () => setNarrow(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return narrow;
};

export function App() {
  const status = useConnectionStatus();
  const route = useRoute();
  const layout = useLayout();
  const narrow = useNarrow();
  const sessionName = useChat((s) => s.data.state.sessionName);
  const [drawer, setDrawer] = useState<"sidebar" | "inspector" | null>(null);
  useGlobalShortcuts();
  useEffect(() => { void loadKeybindings(); }, []);
  useEffect(() => {
    wire.connect();
    return () => wire.close();
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (route.name === "session" && matches(e, "app.subagents.focus")) {
        e.preventDefault();
        if (narrow) setDrawer("inspector");
        else setLayout({ inspectorOpen: true });
        window.setTimeout(() => document.querySelector<HTMLElement>("[data-activity-focus] button")?.focus(), 80);
        return;
      }
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      if (e.key.toLowerCase() === "b") { e.preventDefault(); narrow ? setDrawer((d) => (d === "sidebar" ? null : "sidebar")) : setLayout({ sidebarOpen: !useLayout.getState().sidebarOpen }); }
      if (e.key.toLowerCase() === "i" && route.name === "session") { e.preventDefault(); narrow ? setDrawer((d) => (d === "inspector" ? null : "inspector")) : setLayout({ inspectorOpen: !useLayout.getState().inspectorOpen }); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [narrow, route.name]);

  const inSession = route.name === "session";
  const showSidebar = narrow ? drawer === "sidebar" : layout.sidebarOpen;
  const showInspector = inSession && (narrow ? drawer === "inspector" : layout.inspectorOpen);
  const sidebar = <Sidebar onNavigate={() => setDrawer(null)} />;
  const iconBtn = "grid h-7 w-7 place-items-center rounded-md text-muted transition-colors hover:bg-selected hover:text-fg aria-pressed:text-fg";

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-11 shrink-0 items-center gap-1 border-b border-line bg-bg px-2">
        <button className={iconBtn} aria-label="Toggle sidebar" title="Toggle sidebar (Ctrl+B)" aria-pressed={showSidebar} onClick={() => (narrow ? setDrawer(showSidebar ? null : "sidebar") : setLayout({ sidebarOpen: !layout.sidebarOpen }))}><IconSidebar /></button>
        <button className="flex items-center gap-2 rounded px-1.5 py-1 text-[13px] font-semibold tracking-tight hover:bg-selected" title="Home (g h)" onClick={() => navigate({ name: "sessions" })}>
          <Logo size={16} /> Beehive
        </button>
        {inSession && sessionName ? <span className="hidden min-w-0 items-center gap-2 truncate text-[13px] text-muted sm:flex"><span className="text-grid">/</span><span className="truncate text-fg">{sessionName}</span></span> : null}
        <button className="ml-auto flex h-7 items-center gap-2 rounded-md border border-line bg-surface px-2 text-[12px] text-muted transition-colors hover:border-grid hover:text-fg" onClick={() => usePalette.setState({ open: true })} aria-label="Open command palette" title="Command palette (Ctrl+K)">
          <IconSearch /> <span className="hidden sm:inline">Search or run…</span> <kbd className="bh-kbd hidden sm:inline">Ctrl K</kbd>
        </button>
        <span className="flex items-center gap-1.5 whitespace-nowrap px-2 text-[12px] text-dim" title={`Daemon connection: ${status}`}>
          <span className={`h-1.5 w-1.5 rounded-full ${status === "ready" ? "bg-success" : "bg-primary bh-pulse"}`} /> <span className="hidden sm:inline">{status}</span>
        </span>
        {inSession ? (
          <button className={iconBtn} aria-label="Toggle inspector" title="Toggle inspector (Ctrl+I)" aria-pressed={showInspector} onClick={() => (narrow ? setDrawer(showInspector ? null : "inspector") : setLayout({ inspectorOpen: !layout.inspectorOpen }))}><IconInspector /></button>
        ) : null}
      </header>
      <div className="relative flex min-h-0 flex-1">
        {showSidebar && !narrow ? (
          <>
            <div className="shrink-0 border-r border-line" style={{ width: layout.sidebarWidth }}>{sidebar}</div>
            <ResizeHandle side="right" width={layout.sidebarWidth} label="Resize sidebar" onResize={(w) => setLayout({ sidebarWidth: clampWidth(w) })} />
          </>
        ) : null}
        <main className="min-w-0 flex-1">
          {status !== "ready" ? (
            <div className="grid h-full place-items-center text-muted">Connecting to the Beehive daemon&hellip;</div>
          ) : route.name === "session" ? (
            <SessionView activeSessionId={route.activeSessionId} />
          ) : (
            <Home />
          )}
        </main>
        {showInspector && route.name === "session" && !narrow ? (
          <>
            <ResizeHandle side="left" width={layout.inspectorWidth} label="Resize inspector" onResize={(w) => setLayout({ inspectorWidth: clampWidth(w) })} />
            <div className="shrink-0 border-l border-line" style={{ width: layout.inspectorWidth }}><Inspector activeSessionId={route.activeSessionId} /></div>
          </>
        ) : null}
        {narrow && drawer ? (
          <div className="absolute inset-0 z-40 flex bg-black/60 backdrop-blur-[1px]" onMouseDown={(e) => e.target === e.currentTarget && setDrawer(null)}>
            <div className={`bh-fade-in h-full w-[86%] max-w-sm border-line bg-surface shadow-2xl ${drawer === "sidebar" ? "border-r" : "ml-auto border-l"}`}>
              {drawer === "sidebar" ? sidebar : route.name === "session" ? <Inspector activeSessionId={route.activeSessionId} /> : null}
            </div>
          </div>
        ) : null}
      </div>
      <PanelHost />
      <CommandPalette />
    </div>
  );
}
