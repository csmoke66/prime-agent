import { useEffect } from "react";
import { abort, closeSidePane, cycleDetail, cycleModel, cycleThinking, useChat } from "./store";

/** Browser-safe chat shortcuts mirroring the TUI bindings (crates/bh-tui/src/hotkeys.rs). */
export function useChatHotkeys(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { data, sideOpen, info } = useChat.getState();
      if (info) return;
      const key = e.key.toLowerCase();
      if (e.ctrlKey && key === "o") { e.preventDefault(); return cycleDetail(); }
      if ((e.ctrlKey && key === "l") || (e.altKey && key === "l")) { e.preventDefault(); return void window.dispatchEvent(new Event("bh:chat-model-picker")); }
      if (e.altKey && key === "m") { e.preventDefault(); return void cycleModel(e.shiftKey ? "backward" : "forward"); }
      if (e.shiftKey && e.key === "Tab" && !e.ctrlKey && !e.altKey) { e.preventDefault(); return void cycleThinking(); }
      if (e.key === "Escape" && sideOpen) { e.preventDefault(); return void closeSidePane(); }
      if (e.key === "Escape" && data.state.isStreaming && document.activeElement?.tagName !== "TEXTAREA") return void abort();
      if (e.key === "End" && e.ctrlKey) return void window.dispatchEvent(new Event("bh:chat-follow"));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
