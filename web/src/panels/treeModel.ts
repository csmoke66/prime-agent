// Pure session-tree model for `/tree`: parent/child nodes from `get_session_tree.flatNodes`,
// the TUI filter modes, search, and the text shown for each entry.

export type FlatNode = { entry: Record<string, any>; label?: string; labelTimestamp?: string };
export type TreeNode = { id: string; parentId: string | null; node: FlatNode; children: TreeNode[]; timestamp: string };
export type FilterMode = "default" | "no-tools" | "user-only" | "labeled-only" | "all";
export const FILTER_MODES: readonly FilterMode[] = ["default", "no-tools", "user-only", "labeled-only", "all"];

const SETTINGS_TYPES = new Set(["label", "custom", "model_change", "thinking_level_change", "service_tier_change", "session_info", "child_usage_attributed", "session_state"]);

export function buildTree(flat: FlatNode[]): TreeNode[] {
  const nodes = new Map<string, TreeNode>();
  for (const node of flat) {
    const id = node.entry.id;
    if (typeof id !== "string") continue;
    nodes.set(id, { id, parentId: node.entry.parentId ?? null, node, children: [], timestamp: node.entry.timestamp ?? "" });
  }
  const roots: TreeNode[] = [];
  for (const node of nodes.values()) {
    const parent = node.parentId ? nodes.get(node.parentId) : undefined;
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }
  const sort = (list: TreeNode[]) => {
    list.sort((a, b) => a.timestamp.localeCompare(b.timestamp));
    list.forEach((n) => sort(n.children));
  };
  sort(roots);
  return roots;
}

export function messageText(entry: Record<string, any>): string {
  const content = entry.message?.content ?? entry.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => (part?.type === "text" ? part.text : part?.type === "toolCall" ? `[tool: ${part.name}]` : part?.type === "image" ? "[image]" : ""))
      .filter(Boolean)
      .join(" ");
  }
  return "";
}

export function entryRole(entry: Record<string, any>): string {
  return entry.type === "message" ? String(entry.message?.role ?? "message") : String(entry.type ?? "entry");
}

/** One-line description of an entry (what the TUI tree shows). */
export function describeEntry(entry: Record<string, any>): string {
  const flat = (text: string, max = 120) => {
    const one = text.replace(/\s+/g, " ").trim();
    return one.length > max ? `${one.slice(0, max - 1)}…` : one;
  };
  switch (entry.type) {
    case "message": {
      const role = entry.message?.role;
      const text = flat(messageText(entry));
      if (role === "toolResult") return `[${entry.message?.toolName ?? "tool"} result] ${text}`;
      if (role === "bashExecution") return `[bash] ${flat(String(entry.message?.command ?? ""))}`;
      return `${role === "user" ? "user" : role === "assistant" ? "assistant" : role}: ${text || "(no text)"}`;
    }
    case "compaction":
      return `[compaction] ${flat(String(entry.summary ?? ""), 80)}`;
    case "branch_summary":
      return `[branch summary] ${flat(String(entry.summary ?? ""), 80)}`;
    case "model_change":
      return `[model] ${entry.provider ?? ""}/${entry.modelId ?? ""}`;
    case "thinking_level_change":
      return `[thinking] ${entry.thinkingLevel ?? ""}`;
    case "session_info":
      return `[name] ${entry.name ?? ""}`;
    case "custom_message":
      return `[${entry.customType ?? "custom"}] ${flat(messageText(entry))}`;
    default:
      return `[${entry.type ?? "entry"}]`;
  }
}

function assistantIsToolOnly(entry: Record<string, any>): boolean {
  if (entry.message?.role !== "assistant") return false;
  const content = entry.message?.content;
  const hasText = Array.isArray(content) ? content.some((p: any) => p?.type === "text" && String(p.text).trim()) : Boolean(content);
  return !hasText && (entry.message.stopReason === "stop" || entry.message.stopReason === "toolUse");
}

export function passesFilter(node: FlatNode, mode: FilterMode, leafId: string | null, search: string): boolean {
  const entry = node.entry;
  if (entry.id !== leafId && assistantIsToolOnly(entry)) return false;
  const isSettings = SETTINGS_TYPES.has(entry.type);
  const isUser = entry.type === "message" && entry.message?.role === "user";
  const isToolResult = entry.type === "message" && entry.message?.role === "toolResult";
  const ok =
    mode === "user-only" ? isUser : mode === "no-tools" ? !isSettings && !isToolResult : mode === "labeled-only" ? Boolean(node.label) : mode === "all" ? true : !isSettings;
  if (!ok) return false;
  const tokens = search.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return true;
  const haystack = `${describeEntry(entry)} ${node.label ?? ""}`.toLowerCase();
  return tokens.every((token) => haystack.includes(token));
}

export type TreeRow = { node: TreeNode; indent: number; onPath: boolean; hasChildren: boolean };

/**
 * Visible rows in display order. Indentation only grows below a branch point (a node with several
 * visible children), like the TUI; folded nodes hide their subtree.
 */
export function visibleRows(roots: TreeNode[], opts: { mode: FilterMode; leafId: string | null; search: string; folded: ReadonlySet<string> }): TreeRow[] {
  const byId = new Map<string, TreeNode>();
  const index = (list: TreeNode[]) => list.forEach((n) => (byId.set(n.id, n), index(n.children)));
  index(roots);
  const path = new Set<string>();
  for (let cursor = opts.leafId ? byId.get(opts.leafId) : undefined; cursor; cursor = cursor.parentId ? byId.get(cursor.parentId) : undefined) path.add(cursor.id);

  const rows: TreeRow[] = [];
  // Descendants that pass the filter, skipping filtered-out nodes but keeping their subtrees.
  const collect = (node: TreeNode): TreeNode[] =>
    passesFilter(node.node, opts.mode, opts.leafId, opts.search) ? [node] : node.children.flatMap(collect);
  const walk = (nodes: TreeNode[], indent: number) => {
    for (const node of nodes) {
      const kids = node.children.flatMap(collect);
      rows.push({ node, indent, onPath: path.has(node.id), hasChildren: kids.length > 0 });
      if (!opts.folded.has(node.id)) walk(kids, kids.length > 1 ? indent + 1 : indent);
    }
  };
  walk(roots.flatMap(collect), 0);
  return rows;
}
