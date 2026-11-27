import { rest } from "../panels/api";

/**
 * Browser-safe subset of the TUI keybinding table (crates/bh-tui/src/keybindings/definitions.rs), with
 * the user's `keybindings.json` overrides applied: an action id maps to a key or a list of keys, and an
 * empty list disables it. Web-only extra defaults are listed after the TUI default.
 */
const DEFAULTS = {
  "app.model.select": ["ctrl+l", "alt+l"],
  "app.model.cycleForward": ["alt+m"],
  "app.model.cycleBackward": ["shift+alt+m"],
  "app.model.toggleScope": ["alt+s"],
  "app.tools.expand": ["ctrl+o"],
  "app.subagents.focus": ["alt+a"],
  "app.prompt.stash": ["ctrl+s", "super+s"],
  "app.message.followUp": ["alt+enter", "ctrl+enter", "super+enter"],
  "app.message.navigateOlder": ["alt+up"],
  "app.message.navigateNewer": ["alt+down"],
  "app.message.moveEarlier": ["ctrl+alt+up"],
  "app.message.moveLater": ["ctrl+alt+down"],
  "app.clipboard.pasteImage": ["ctrl+v"],
  "app.input.clear": ["escape"],
} as const;

export type KeyAction = keyof typeof DEFAULTS;

const LEGACY_NAMES: Record<string, string> = {
  selectModel: "app.model.select",
  expandTools: "app.tools.expand",
  focusSubagents: "app.subagents.focus",
  followUp: "app.message.followUp",
  dequeue: "app.message.navigateOlder",
  pasteImage: "app.clipboard.pasteImage",
  clearInput: "app.input.clear",
};

let overrides: Record<string, string[]> = {};
let loading: Promise<void> | undefined;

/** Fetch `keybindings.json` once; unreadable or malformed entries keep the defaults. */
export function loadKeybindings(): Promise<void> {
  loading ??= rest.keybindings().then(
    (raw) => {
      const next: Record<string, string[]> = {};
      for (const [name, value] of Object.entries(raw)) {
        const id = LEGACY_NAMES[name] ?? name;
        if (id !== name && id in raw) continue;
        if (typeof value === "string") next[id] = [value];
        else if (Array.isArray(value) && value.every((v) => typeof v === "string")) next[id] = value as string[];
      }
      overrides = next;
    },
    () => undefined,
  );
  return loading;
}

/** Forget the loaded overrides and read `keybindings.json` again (`/reload`). */
export function resetKeybindings(): Promise<void> {
  loading = undefined;
  return loadKeybindings();
}

export const keysOf = (action: KeyAction): readonly string[] => overrides[action] ?? DEFAULTS[action];

const NAMED: Record<string, string> = { arrowup: "up", arrowdown: "down", arrowleft: "left", arrowright: "right", " ": "space", esc: "escape" };

function eventKey(e: KeyboardEvent): string {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  const key = e.key.toLowerCase();
  return NAMED[key] ?? key;
}

/** Whether `e` is exactly one of the keys bound to `action`. */
export function matches(e: KeyboardEvent, action: KeyAction): boolean {
  const key = eventKey(e);
  return keysOf(action).some((spec) => {
    const parts = spec.toLowerCase().split("+");
    const wanted = NAMED[parts[parts.length - 1]] ?? parts[parts.length - 1];
    const mods = new Set(parts.slice(0, -1));
    return wanted === key && mods.has("ctrl") === e.ctrlKey && mods.has("alt") === e.altKey && mods.has("shift") === e.shiftKey && (mods.has("super") || mods.has("meta")) === e.metaKey;
  });
}

const GLYPHS: Record<string, string> = { up: "↑", down: "↓", left: "←", right: "→", escape: "Esc", enter: "Enter", space: "Space", tab: "Tab" };

/** Display text such as `Ctrl+Alt+↑` for the first binding of `action`; "unbound" when disabled. */
export function keyLabel(action: KeyAction): string {
  const [first] = keysOf(action);
  if (!first) return "unbound";
  return first.split("+").map((part) => GLYPHS[part] ?? (part.length === 1 ? part.toUpperCase() : part[0].toUpperCase() + part.slice(1))).join("+");
}
