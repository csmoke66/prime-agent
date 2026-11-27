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
  const iconBtn = "grid h-6 min-w-6 place-items-center rounded px-1 text-muted hover:bg-selected hover:text-fg";

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-8 shrink-0 items-center gap-2 border-b border-grid/30 bg-surface px-2">
        <button className={iconBtn} aria-label="Toggle sidebar" title="Toggle sidebar (Ctrl+B)" aria-pressed={showSidebar} onClick={() => (narrow ? setDrawer(showSidebar ? null : "sidebar") : setLayout({ sidebarOpen: !layout.sidebarOpen }))}>&#x2630;</button>
        <button className="flex items-center gap-1.5 font-semibold tracking-tight" onClick={() => navigate({ name: "sessions" })}>
          <span className="text-primary-soft">&#x2B22;</span> Beehive
        </button>
        {inSession && sessionName ? <span className="hidden min-w-0 truncate text-muted sm:block">/ {sessionName}</span> : null}
        <button className="bh-btn ml-auto !border-transparent !bg-transparent whitespace-nowrap text-muted" onClick={() => usePalette.setState({ open: true })} aria-label="Open command palette">
          <span className="hidden sm:inline">Search</span> <kbd className="bh-kbd hidden sm:inline">Ctrl K</kbd><span className="sm:hidden">&#x2315;</span>
        </button>
        <span className="whitespace-nowrap text-[11px] text-dim">
          <span className={status === "ready" ? "text-success" : "text-warning bh-pulse"}>&#x25CF;</span> <span className="hidden sm:inline">{status}</span>
        </span>
        {inSession ? (
          <button className={iconBtn} aria-label="Toggle inspector" title="Toggle inspector (Ctrl+I)" aria-pressed={showInspector} onClick={() => (narrow ? setDrawer(showInspector ? null : "inspector") : setLayout({ inspectorOpen: !layout.inspectorOpen }))}>&#x25E7;</button>
        ) : null}
      </header>
      <div className="relative flex min-h-0 flex-1">
        {showSidebar && !narrow ? (
          <>
            <div className="shrink-0 border-r border-grid/30" style={{ width: layout.sidebarWidth }}>{sidebar}</div>
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
            <div className="shrink-0 border-l border-grid/30" style={{ width: layout.inspectorWidth }}><Inspector activeSessionId={route.activeSessionId} /></div>
          </>
        ) : null}
        {narrow && drawer ? (
          <div className="absolute inset-0 z-40 flex bg-black/50" onMouseDown={(e) => e.target === e.currentTarget && setDrawer(null)}>
            <div className={`bh-fade-in h-full w-[86%] max-w-sm border-grid/30 bg-surface shadow-2xl ${drawer === "sidebar" ? "border-r" : "ml-auto border-l"}`}>
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
