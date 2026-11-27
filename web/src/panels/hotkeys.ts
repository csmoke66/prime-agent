// Web-UI shortcuts plus the TUI bindings that keep their meaning in the browser (from
// crates/bh-tui/src/keybindings/definitions.rs). Shown by `/hotkeys`.
import type { KeyAction } from "../app/keybindings";
export type HotkeyGroup = { title: string; keys: { keys: string; action: string; id?: KeyAction }[] };

export const HOTKEYS: HotkeyGroup[] = [
  {
    title: "Global",
    keys: [
      { keys: "Ctrl/⌘+K", action: "Command palette" },
      { keys: "g h", action: "Go home (no session open)" },
      { keys: "Ctrl+B", action: "Toggle the sessions sidebar" },
      { keys: "Ctrl+I", action: "Toggle the inspector" },
      { keys: "?", action: "Show this keyboard reference" },
      { keys: "Esc", action: "Close dialog / interrupt response / clear prompt" },
    ],
  },
  {
    title: "Chat",
    keys: [
      { keys: "Enter", action: "Send message" },
      { keys: "Shift+Enter", action: "New line" },
      { keys: "Alt+Enter", action: "Queue follow-up message", id: "app.message.followUp" },
      { keys: "Ctrl+L", action: "Open model selector", id: "app.model.select" },
      { keys: "Alt+M", action: "Cycle to the next scoped model", id: "app.model.cycleForward" },
      { keys: "Shift+Alt+M", action: "Cycle to the previous scoped model", id: "app.model.cycleBackward" },
      { keys: "Alt+S", action: "Toggle model selector scope", id: "app.model.toggleScope" },
      { keys: "Ctrl+O", action: "Cycle conversation detail / expand tools", id: "app.tools.expand" },
      { keys: "Ctrl+S", action: "Stash or restore draft prompt", id: "app.prompt.stash" },
      { keys: "Alt+↑", action: "Select older pending message (Enter saves the edit)", id: "app.message.navigateOlder" },
      { keys: "Alt+↓", action: "Select newer pending message or the draft", id: "app.message.navigateNewer" },
      { keys: "Ctrl+Alt+↑", action: "Move selected pending message earlier", id: "app.message.moveEarlier" },
      { keys: "Ctrl+Alt+↓", action: "Move selected pending message later", id: "app.message.moveLater" },
      { keys: "Ctrl+V", action: "Paste image from clipboard", id: "app.clipboard.pasteImage" },
      { keys: "Alt+A", action: "Open the inspector on subagents", id: "app.subagents.focus" },
    ],
  },
  {
    title: "Agents view",
    keys: [
      { keys: "↑ / ↓", action: "Move selection" },
      { keys: "→ / Enter", action: "Open selected agent" },
      { keys: "←", action: "Return to parent scope" },
      { keys: "Space", action: "Reply to selected agent" },
      { keys: "Ctrl+N", action: "New session" },
      { keys: "Ctrl+X", action: "Stop or delete selected agent" },
      { keys: "Ctrl+R", action: "Rename selected agent" },
      { keys: "Alt+G", action: "Move selected agent to a group" },
      { keys: "Ctrl+F", action: "Search session message text" },
      { keys: "/", action: "Focus search" },
    ],
  },
  {
    title: "Session tree",
    keys: [
      { keys: "Shift+L", action: "Edit label" },
      { keys: "Ctrl+D / T / U / L / A", action: "Filter: default / no tools / user only / labeled / all" },
      { keys: "Ctrl+O", action: "Cycle filter" },
    ],
  },
  {
    title: "Terminal only",
    keys: [
      { keys: "Ctrl+Z", action: "Suspend (not applicable in a browser)" },
      { keys: "Ctrl+G", action: "External editor (not applicable in a browser)" },
    ],
  },
];
