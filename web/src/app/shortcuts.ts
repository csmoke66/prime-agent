import { useEffect } from "react";
import { create } from "zustand";
import { navigate } from "./router";
import { openPanel } from "../panels/store";

export const usePalette = create<{ open: boolean; setOpen: (open: boolean) => void }>((set) => ({ open: false, setOpen: (open) => set({ open }) }));

const isTyping = (target: EventTarget | null): boolean => {
  const el = target as HTMLElement | null;
  return Boolean(el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable));
};

/**
 * Global shortcuts: Ctrl/⌘+K palette, `?` hotkeys, `g h` home (outside text fields).
 * Chat-specific keys belong to the chat view.
 */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    let pendingG = 0;
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        usePalette.setState((s) => ({ open: !s.open }));
        return;
      }
      if (isTyping(event.target) || event.ctrlKey || event.metaKey || event.altKey || document.querySelector('[role="dialog"]')) return;
      if (event.key === "?") {
        event.preventDefault();
        openPanel("hotkeys");
      } else if (event.key === "g") {
        pendingG = Date.now();
      } else if (pendingG && Date.now() - pendingG < 900 && event.key === "h") {
        pendingG = 0;
        navigate({ name: "sessions" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
