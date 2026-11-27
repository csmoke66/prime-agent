import { useEffect } from "react";
import { loadKeybindings, matches } from "../app/keybindings";
import { abort, closeSidePane, cycleDetail, cycleModel, cycleThinking, useChat } from "./store";

/** Browser-safe chat shortcuts mirroring the TUI bindings (crates/bh-tui/src/hotkeys.rs). */
export function useChatHotkeys(): void {
  useEffect(() => {
    void loadKeybindings();
    const onKey = (e: KeyboardEvent) => {
      const { data, sideOpen, info } = useChat.getState();
      if (info) return;
      if (matches(e, "app.tools.expand")) { e.preventDefault(); return cycleDetail(); }
      if (matches(e, "app.model.select")) { e.preventDefault(); return void window.dispatchEvent(new Event("bh:chat-model-picker")); }
      if (matches(e, "app.model.cycleForward")) { e.preventDefault(); return void cycleModel("forward"); }
      if (matches(e, "app.model.cycleBackward")) { e.preventDefault(); return void cycleModel("backward"); }
      if (e.shiftKey && e.key === "Tab" && !e.ctrlKey && !e.altKey) { e.preventDefault(); return void cycleThinking(); }
      if (e.key === "Escape" && sideOpen) { e.preventDefault(); return void closeSidePane(); }
      if (e.key === "Escape" && data.state.isStreaming && document.activeElement?.tagName !== "TEXTAREA") return void abort();
      if (e.key === "End" && e.ctrlKey) return void window.dispatchEvent(new Event("bh:chat-follow"));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
