import { formatCost, formatTokens, parseSlash, resolveCommand, type SlashCommand } from "./model";
import { command, errorText, note, openInfo, sendPrompt, startSideQuestion, toast, useChat, type SendMode } from "./store";

/** Event the panels owner listens for: client slash commands that open a panel or leave the chat. */
export const SLASH_EVENT = "bh:slash";

export interface SlashDetail {
  name: string;
  args: string;
  activeSessionId: string | null;
}

interface TreeNode {
  label?: string;
  id?: string;
  status?: string;
  model?: { id?: string };
  ownUsage?: any;
  contextUsage?: { tokens?: number; contextWindow?: number; percent?: number };
  deleted?: boolean;
  children?: TreeNode[];
}

const own = (n: TreeNode) => {
  const u = n.ownUsage ?? {};
  return { tokens: (u.input ?? 0) + (u.output ?? 0) + (u.cacheRead ?? 0) + (u.cacheWrite ?? 0), cost: u.cost?.total ?? 0 };
};

function totalOf(n: TreeNode): { tokens: number; cost: number } {
  const t = (n as any).totalUsage;
  if (t) return { tokens: (t.input ?? 0) + (t.output ?? 0) + (t.cacheRead ?? 0) + (t.cacheWrite ?? 0), cost: t.cost?.total ?? 0 };
  return (n.children ?? []).reduce((acc, c) => {
    const t = totalOf(c);
    return { tokens: acc.tokens + t.tokens, cost: acc.cost + t.cost };
  }, own(n));
}

function treeLines(n: TreeNode, depth: number, all: boolean, out: string[]): void {
  const t = totalOf(n);
  const ctx = n.contextUsage?.contextWindow && n.contextUsage.tokens != null ? `${(((n.contextUsage.tokens ?? 0) / n.contextUsage.contextWindow) * 100).toFixed(0)}% ctx` : "";
  const name = n.label || n.id || "session";
  out.push(`${"  ".repeat(depth)}${depth ? "└ " : ""}${name}${n.deleted ? " (deleted)" : ""}  ${n.status ?? ""}  ${formatTokens(t.tokens)} tok  ${formatCost(t.cost)}  ${ctx}`.trimEnd());
  const children = n.children ?? [];
  const shown = all || children.length <= 12 ? children : [...children].sort((a, b) => totalOf(b).cost - totalOf(a).cost).slice(0, 12);
  for (const c of shown) treeLines(c, depth + 1, all, out);
  if (shown.length < children.length) out.push(`${"  ".repeat(depth + 1)}… ${children.length - shown.length} more (use /context all)`);
}

const costKey = (id: string) => `bh:cost-baseline:${id}`;

async function fetchTree(): Promise<TreeNode | undefined> {
  const data = await command("get_context_tree");
  return data ? (data.tree ?? data.root ?? data) : undefined;
}

async function chatCommand(name: string, args: string): Promise<void> {
  const id = useChat.getState().activeSessionId ?? "";
  switch (name) {
    case "copy": {
      const data = await command("get_last_assistant_text");
      const text = String(data?.text ?? "").trim();
      if (!text) return note("No agent messages to copy yet.", "error");
      await navigator.clipboard?.writeText(text);
      return toast("Copied last agent message to clipboard");
    }
    case "btw":
      return args ? startSideQuestion(args) : note("Usage: /btw <question>", "warning");
    case "name": {
      if (!args) {
        const current = useChat.getState().data.state.sessionName;
        return note(current ? `Session name: ${current}` : "Usage: /name <name>", current ? "info" : "warning");
      }
      await command("set_session_name", { name: args });
      return;
    }
    case "session": {
      const stats = await command("get_session_stats");
      if (!stats) return;
      const name = useChat.getState().data.state.sessionName;
      const rows = [
        name ? `Name:        ${name}` : null,
        `File:        ${stats.sessionFile || "In-memory"}`,
        `ID:          ${stats.sessionId}`,
        "",
        `User messages:      ${stats.userMessages ?? 0}`,
        `Assistant messages: ${stats.assistantMessages ?? 0}`,
        `Tool calls:         ${stats.toolCalls ?? 0}`,
        `Tool results:       ${stats.toolResults ?? 0}`,
        `Total messages:     ${stats.totalMessages ?? 0}`,
        "",
        "Use /context for token, cost, and context usage.",
      ].filter((l) => l !== null) as string[];
      return openInfo({ title: "Session info", text: rows.join("\n") });
    }
    case "cost": {
      if (args && args !== "reset") return note("Usage: /cost [reset]", "error");
      const tree = await fetchTree();
      if (!tree) return;
      const total = totalOf(tree).cost;
      if (args === "reset") localStorage.setItem(costKey(id), String(total));
      const base = Number(localStorage.getItem(costKey(id)) ?? 0);
      return openInfo({ title: "Session cost", text: `Displayed cost: ${formatCost(Math.max(0, total - base))}\nTotal recorded: ${formatCost(total)}${base ? `\nCheckpoint (display only): ${formatCost(base)}` : ""}` });
    }
    case "context": {
      if (args && args !== "all") return note("Usage: /context [all]", "error");
      const tree = await fetchTree();
      if (!tree) return;
      const lines: string[] = [];
      treeLines(tree, 0, args === "all", lines);
      const t = totalOf(tree);
      lines.push("", `Total: ${formatTokens(t.tokens)} tokens · ${formatCost(t.cost)}`);
      return openInfo({ title: "Context usage", text: lines.join("\n") });
    }
    case "system-prompt": {
      const data = await command("get_system_prompt");
      if (data) openInfo({ title: "System prompt", text: String(data.systemPrompt ?? "") });
      return;
    }
    case "logs":
      return openInfo({ title: "Logs", text: "Daemon and client logs are saved under the agent directory's logs/ folder (see GET /api/info → agentDir)." });
    case "reload": {
      const state = useChat.getState().data.state;
      if (state.isStreaming) return note("Wait for the current response to finish before reloading.", "warning");
      await command("reload");
      return toast("Reloaded keybindings, skills, and prompts");
    }
  }
}

/**
 * Run submitted text that starts with `/`. Returns false when it is not a builtin client/session
 * command (the caller sends it as an ordinary prompt, e.g. `/skill:name request`).
 */
export async function runSlash(text: string, mode: SendMode): Promise<boolean> {
  const parsed = parseSlash(text);
  if (!parsed) return false;
  if (parsed.name.startsWith("skill:")) {
    if (!parsed.args) {
      note("Add your request after the skill, e.g. /skill:name make a release", "info");
      return true;
    }
    return false;
  }
  const cmd: SlashCommand | undefined = resolveCommand(parsed.name);
  if (!cmd) return false;
  try {
    switch (cmd.kind) {
      case "session":
        await sendPrompt(text.trim(), mode);
        return true;
      case "chat":
        await chatCommand(cmd.name, parsed.args);
        return true;
      case "panel":
        window.dispatchEvent(new CustomEvent<SlashDetail>(SLASH_EVENT, { detail: { name: cmd.name, args: parsed.args, activeSessionId: useChat.getState().activeSessionId } }));
        return true;
    }
  } catch (error) {
    note(errorText(error), "error");
    return true;
  }
}
