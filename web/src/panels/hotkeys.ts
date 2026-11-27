// Web-UI shortcuts plus the TUI bindings that keep their meaning in the browser (from
// crates/bh-tui/src/keybindings/definitions.rs). Shown by `/hotkeys`.
export type HotkeyGroup = { title: string; keys: { keys: string; action: string }[] };

export const HOTKEYS: HotkeyGroup[] = [
  {
    title: "Global",
    keys: [
      { keys: "Ctrl/⌘+K", action: "Command palette" },
      { keys: "g s", action: "Go to sessions" },
      { keys: "g a", action: "Go to agents view" },
      { keys: "?", action: "Show this keyboard reference" },
      { keys: "Esc", action: "Close dialog / interrupt response / clear prompt" },
    ],
  },
  {
    title: "Chat",
    keys: [
      { keys: "Enter", action: "Send message" },
      { keys: "Shift+Enter", action: "New line" },
      { keys: "Alt+Enter", action: "Queue follow-up message" },
      { keys: "Ctrl+L", action: "Open model selector" },
      { keys: "Alt+M / Shift+Alt+M", action: "Cycle scoped model" },
      { keys: "Ctrl+O", action: "Cycle conversation detail / expand tools" },
      { keys: "Ctrl+S", action: "Stash or restore draft prompt" },
      { keys: "Alt+↑ / Alt+↓", action: "Select older / newer pending message" },
      { keys: "Ctrl+Alt+↑ / ↓", action: "Move selected pending message earlier / later" },
      { keys: "Ctrl+V", action: "Paste image from clipboard" },
      { keys: "Alt+A", action: "Focus activity" },
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
