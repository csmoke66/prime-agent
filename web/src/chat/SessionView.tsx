import { useEffect } from "react";
import { wire } from "../wire";
import "./chat.css";
import { Composer } from "./Composer";
import { StatusBar } from "./StatusBar";
import { Transcript } from "./Transcript";
import { useChatHotkeys } from "./hotkeys";
import { GoalBar, InfoModal, QueuePanel, SidePane } from "./panels";
import { attachSession, detachSession, handleFrame, useChat } from "./store";

export { SLASH_EVENT, type SlashDetail } from "./slash";

function ToastStack() {
  const toasts = useChat((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed inset-x-0 top-12 z-50 flex flex-col items-center gap-2" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`bh-fade-in rounded border bg-surface px-2.5 py-1 text-[12px] shadow-lg ${t.kind === "error" ? "border-error/50 text-error" : t.kind === "warning" ? "border-warning/50 text-warning" : "border-grid/50"}`}>{t.text}</div>
      ))}
    </div>
  );
}

export function SessionView({ activeSessionId }: { activeSessionId: string }) {
  const attach = useChat((s) => s.attach);
  const error = useChat((s) => s.attachError);
  const name = useChat((s) => s.data.state.sessionName);
  useChatHotkeys();

  useEffect(() => {
    void attachSession(activeSessionId);
    const offFrame = wire.onFrame(handleFrame);
    const offStatus = wire.onStatus((status) => {
      if (status === "ready") void attachSession(useChat.getState().activeSessionId ?? activeSessionId);
    });
    return () => {
      offFrame();
      offStatus();
      detachSession();
    };
  }, [activeSessionId]);

  useEffect(() => {
    document.title = name ? `${name} · Beehive` : "Beehive";
  }, [name]);

  if (attach === "failed")
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <div>
          <p className="mb-1 text-error">Could not attach to session {activeSessionId}</p>
          <p className="mb-4 text-sm text-muted">{error}</p>
          <button className="bh-btn" onClick={() => void attachSession(activeSessionId)}>Retry</button>
        </div>
      </div>
    );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <GoalBar />
      {attach === "attaching" ? <div className="grid flex-1 place-items-center text-muted"><span className="bh-pulse">Loading session…</span></div> : <Transcript />}
      <SidePane />
      <QueuePanel />
      <Composer />
      <StatusBar />
      <InfoModal />
      <ToastStack />
      {attach === "closed" ? <div className="bg-tool-error px-4 py-1 text-center text-xs text-error">This session is closed.</div> : null}
    </div>
  );
}
