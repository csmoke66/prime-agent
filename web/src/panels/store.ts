import { create } from "zustand";

export type PanelName =
  | "settings" | "mcp" | "plugins" | "harness" | "heartbeats" | "tree" | "fork" | "clone" | "login" | "logout"
  | "hotkeys" | "changelog" | "logs" | "export" | "import" | "share" | "workflows" | "new" | "rename" | "onboarding" | "messages" | "edit_context";

export type PanelRequest = { name: PanelName; args: string; activeSessionId?: string };

type PanelState = { open: PanelRequest | null; show: (request: PanelRequest) => void; close: () => void };

export const usePanel = create<PanelState>((set) => ({
  open: null,
  show: (request) => set({ open: request }),
  close: () => set({ open: null }),
}));

/** Open a panel from anywhere (palette, views, shortcuts). */
export function openPanel(name: PanelName, options: { args?: string; activeSessionId?: string } = {}): void {
  usePanel.getState().show({ name, args: options.args ?? "", activeSessionId: options.activeSessionId });
}

/** Chat view must re-snapshot after an in-place rewrite (tree navigation, fork, clone). */
export function announceRewrite(activeSessionId: string): void {
  window.dispatchEvent(new CustomEvent("bh:session-rewritten", { detail: { activeSessionId } }));
}
