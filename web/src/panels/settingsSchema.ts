// The settings menu layout of the TUI (settings_menu/tabs.rs + mod.rs rows) as data. `path` is the
// dotted location in settings.json.

export type SettingRow = {
  id: string;
  label: string;
  description: string;
  path?: string;
  kind: "bool" | "enum" | "number" | "text" | "lines" | "special";
  values?: readonly string[];
  /** Default shown when the key is absent from settings.json. */
  fallback?: unknown;
  /** Value is kept in the browser only. */
  local?: boolean;
  min?: number;
  max?: number;
  step?: number;
};

export type SettingsTab = { id: string; label: string; rows: SettingRow[] };

const BOOL = { kind: "bool" } as const;

export const SETTINGS_TABS: SettingsTab[] = [
  {
    id: "general",
    label: "General",
    rows: [
      { id: "autocompact", label: "Auto-compact", description: "Automatically compact context when it gets too large", path: "compaction.enabled", fallback: true, ...BOOL },
      { id: "autocompact-threshold", label: "Auto-compact threshold (%)", description: "Context percentage (1–100) that triggers compaction", path: "compaction.thresholdPercent", fallback: 95, kind: "number", min: 1, max: 100, step: 5 },
      { id: "tool-output-reminder", label: "Tool-result reminder (%)", description: "Remind on every request at or above this share of the model window; 0 disables", path: "compaction.toolOutputReminderPercent", fallback: 0, kind: "number", min: 0, max: 100, step: 5 },
      { id: "steering-mode", label: "Steering mode", description: "How queued steering messages are delivered while streaming", path: "steeringMode", fallback: "one-at-a-time", kind: "enum", values: ["one-at-a-time", "all"] },
      { id: "follow-up-mode", label: "Follow-up mode", description: "How queued follow-up messages are delivered when the agent stops", path: "followUpMode", fallback: "one-at-a-time", kind: "enum", values: ["one-at-a-time", "all"] },
      { id: "quiet-startup", label: "Quiet startup", description: "Disable verbose printing at startup", path: "quietStartup", fallback: false, ...BOOL },
      { id: "warnings", label: "Anthropic extra-usage warning", description: "Warn when a request would bill extra usage", path: "warnings.anthropicExtraUsage", fallback: true, ...BOOL },
      { id: "provider-block-delivery", label: "Provider rejection delivery", description: "Show in chat, silence locally, or route a sanitized report to another session", path: "providerBlockDelivery", fallback: "show", kind: "enum", values: ["show", "silence", "route"] },
      { id: "telemetry", label: "Usage telemetry", description: "Persist the telemetry preference; environment overrides still apply", path: "telemetry.enabled", fallback: true, ...BOOL },
    ],
  },
  {
    id: "models",
    label: "Models",
    rows: [
      { id: "thinking", label: "Default thinking level", description: "Reasoning effort for new sessions", path: "defaultThinkingLevel", fallback: "medium", kind: "enum", values: ["off", "minimal", "low", "medium", "high", "max"] },
      { id: "transport", label: "Transport", description: "Preferred transport for providers that support several", path: "transport", fallback: "auto", kind: "enum", values: ["sse", "websocket", "websocket-cached", "auto"] },
      { id: "default-service-tier", label: "Default service tier", description: "Service tier for new sessions", path: "defaultServiceTier", fallback: "default", kind: "enum", values: ["default", "priority", "flex"] },
      { id: "default-model", label: "Default model", description: "provider/model used by new sessions", kind: "special" },
      { id: "allowed-models", label: "Allowed models", description: "Patterns of models that may be used globally, one per line; empty allows all", path: "allowedModels", kind: "lines" },
    ],
  },
  {
    id: "display",
    label: "Display",
    rows: [
      { id: "show-images", label: "Show image metadata", description: "Show image type and dimensions", path: "terminal.showImages", fallback: true, ...BOOL },
      { id: "auto-resize-images", label: "Auto-resize images", description: "Resize large images to 2000×2000 max for model compatibility", path: "images.autoResize", fallback: true, ...BOOL },
      { id: "block-images", label: "Block images", description: "Prevent images from being sent to LLM providers", path: "images.blockImages", fallback: false, ...BOOL },
      { id: "mermaid-rendering", label: "Mermaid diagrams", description: "Render Mermaid code blocks as diagrams", path: "markdown.mermaid", fallback: "streaming", kind: "enum", values: ["off", "final", "streaming"] },
      { id: "speed-display", label: "Output speed", description: "Show output tokens per second in the status bar (this browser)", path: "bh.speedDisplay", fallback: false, local: true, ...BOOL },
    ],
  },
  {
    id: "editor",
    label: "Editor",
    rows: [
      { id: "editor-padding", label: "Editor padding", description: "Horizontal padding for the input editor (0–3)", path: "editorPaddingX", fallback: 1, kind: "number", min: 0, max: 3, step: 1 },
      { id: "autocomplete-max-visible", label: "Autocomplete max items", description: "Max visible items in the autocomplete list (3–20)", path: "autocompleteMaxVisible", fallback: 5, kind: "number", min: 3, max: 20, step: 1 },
      { id: "emoji-shortcodes", label: "Emoji shortcodes", description: "Suggest emoji when typing :name", path: "emojiShortcodes", fallback: true, ...BOOL },
    ],
  },
  {
    id: "agents",
    label: "Agents",
    rows: [
      { id: "skill-commands", label: "Skill commands", description: "Register skills as /skill:name commands", path: "enableSkillCommands", fallback: true, ...BOOL },
      { id: "builtin-skills", label: "Built-in skills", description: "Load built-in skills (takes effect after reload)", path: "enableBuiltinSkills", fallback: true, ...BOOL },
      { id: "idle-eviction", label: "Idle worker eviction (minutes)", description: "Stop fully idle agent trees after this many minutes; “off” disables", path: "idleEvictionMinutes", fallback: "90", kind: "enum", values: ["off", "30", "60", "90", "180", "360"] },
      { id: "tree-filter-mode", label: "Tree filter mode", description: "Default filter when opening /tree", path: "treeFilterMode", fallback: "user-only", kind: "enum", values: ["default", "no-tools", "user-only", "labeled-only", "all"] },
      { id: "harness-refinements", label: "Harness refinements", description: "Create and load harness refinements", path: "harnessRefinements", fallback: true, ...BOOL },
      { id: "web-search-provider", label: "Web search provider", description: "Search provider for new sessions", path: "webSearchProvider", fallback: "serper", kind: "enum", values: ["serper", "eco", "chatgpt", "claude"] },
      { id: "factory", label: "Factory", description: "Enable factory orchestration for agents", path: "factory.enabled", fallback: false, ...BOOL },
    ],
  },
  {
    id: "reliability",
    label: "Compaction & retry",
    rows: [
      { id: "reserve-tokens", label: "Compaction reserve tokens", description: "Tokens kept free for the model's reply", path: "compaction.reserveTokens", kind: "number", min: 0, step: 1000 },
      { id: "keep-recent", label: "Compaction keep-recent tokens", description: "Recent tokens kept verbatim after compaction", path: "compaction.keepRecentTokens", kind: "number", min: 0, step: 1000 },
      { id: "auto-retry", label: "Auto-retry", description: "Retry failed provider requests", path: "retry.enabled", fallback: true, ...BOOL },
      { id: "max-retries", label: "Max retries", description: "Attempts before giving up", path: "retry.maxRetries", kind: "number", min: 0, max: 20, step: 1 },
      { id: "retry-delay", label: "Retry base delay (ms)", description: "First backoff delay", path: "retry.baseDelayMs", kind: "number", min: 0, step: 500 },
      { id: "branch-summary-skip", label: "Skip branch-summary prompt", description: "Never ask to summarize when navigating the tree", path: "branchSummary.skipPrompt", fallback: false, ...BOOL },
    ],
  },
];

export function getPath(source: Record<string, any>, path: string): unknown {
  return path.split(".").reduce<any>((value, key) => (value && typeof value === "object" ? value[key] : undefined), source);
}

/** Immutable set; `undefined` removes the key (and prunes emptied parents). */
export function setPath(source: Record<string, any>, path: string, value: unknown): Record<string, any> {
  const [head, ...rest] = path.split(".");
  const next = { ...source };
  if (rest.length === 0) {
    if (value === undefined) delete next[head];
    else next[head] = value;
    return next;
  }
  const child = setPath(source[head] && typeof source[head] === "object" ? source[head] : {}, rest.join("."), value);
  if (Object.keys(child).length === 0) delete next[head];
  else next[head] = child;
  return next;
}

export function rowValue(row: SettingRow, settings: Record<string, any>): unknown {
  const value = row.path ? getPath(settings, row.path) : undefined;
  return value === undefined ? row.fallback : value;
}
