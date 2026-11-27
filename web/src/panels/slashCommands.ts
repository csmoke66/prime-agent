// Builtin slash-command table, mirroring crates/bh-types/src/slash_commands.rs (names, aliases, hints).
// `owner` says who handles the command in the web UI: "panel" opens a panel from this module,
// "chat" is run by the chat view.
export type SlashCommand = { name: string; description: string; hint?: string; aliases?: string[]; owner: "panel" | "chat" | "none" };

export const SLASH_COMMANDS: SlashCommand[] = [
  { name: "settings", description: "Open settings menu", owner: "panel" },
  { name: "export", description: "Export session (HTML or JSONL download)", hint: "[path]", owner: "panel" },
  { name: "import", description: "Import and resume a session from a JSONL file", hint: "<path.jsonl>", owner: "panel" },
  { name: "share", description: "Share session as a secret GitHub gist", owner: "panel" },
  { name: "copy", description: "Copy last agent message to clipboard", owner: "chat" },
  { name: "btw", description: "Ask a side question without adding it to the session", hint: "<question>", aliases: ["side"], owner: "chat" },
  { name: "name", description: "Set or show the session display name", hint: "[name]", aliases: ["rename"], owner: "panel" },
  { name: "session", description: "Show session info", owner: "chat" },
  { name: "cost", description: "Show or reset this session tree's displayed cost", hint: "[reset]", owner: "chat" },
  { name: "system-prompt", description: "Show the exact system prompt sent to the model", owner: "chat" },
  { name: "logs", description: "Show where daemon and client logs are saved", owner: "panel" },
  { name: "context", description: "Show token, cost, and context usage for agent and sub-agents", aliases: ["usage"], owner: "chat" },
  { name: "changelog", description: "Show changelog entries", owner: "panel" },
  { name: "hotkeys", description: "Show all keyboard shortcuts", owner: "panel" },
  { name: "fork", description: "Create a new fork from a previous user message", owner: "panel" },
  { name: "clone", description: "Duplicate the current session at the current position", owner: "panel" },
  { name: "tree", description: "Navigate session tree (switch branches)", owner: "panel" },
  { name: "login", description: "Configure provider authentication", owner: "panel" },
  { name: "logout", description: "Remove provider authentication", owner: "panel" },
  { name: "mcp", description: "Open the MCP connections menu", owner: "panel" },
  { name: "plugins", description: "Browse and connect external services", hint: "[search]", owner: "panel" },
  { name: "new", description: "Start a new session, optionally named and/or with an initial prompt", hint: '[--name "session name" --] [prompt]', aliases: ["clear"], owner: "panel" },
  { name: "edit_context", description: "Edit the model-visible context as JSON", owner: "panel" },
  { name: "compact", description: "Compact the session context; optional instructions focus the summary", hint: "[instructions]", owner: "chat" },
  { name: "refine", description: "Refine continual harness prompt notes, skills, subagents, and memory", owner: "chat" },
  { name: "harness", description: "Browse and delete local and global harness entries", owner: "panel" },
  { name: "run", description: "Run a named workflow with JSON input and output", hint: "[workflow [JSON]]", owner: "panel" },
  { name: "goal", description: "Set or view a persistent goal; supports pause, resume, and clear", hint: "[objective]", owner: "chat" },
  { name: "heartbeat", description: "Set or view a persistent heartbeat for this session", hint: "[status|pause|resume|stop|every <duration> <instruction>]", owner: "panel" },
  { name: "heartbeats", description: "View and manage all user and agent heartbeats", owner: "panel" },
  { name: "resume", description: "Open the agents view, or resume a session by id or path", hint: "[id|path]", owner: "panel" },
  { name: "reload", description: "Reload keybindings, skills, and prompts", owner: "panel" },
  { name: "quit", description: "Quit Beehive (terminal only)", owner: "none" },
];

/** Resolve a typed name or alias to its canonical command. */
export function resolveSlash(name: string): SlashCommand | undefined {
  return SLASH_COMMANDS.find((c) => c.name === name || c.aliases?.includes(name));
}
