import { useEffect, useMemo, useRef, useState } from "react";
import { navigate, useRoute } from "./router";
import { usePalette } from "./shortcuts";
import { useRoster } from "../agents/rosterStore";
import { retainRoster } from "../agents/rosterStore";
import { sessionTitle, isSubagent, isUnnamedEmpty } from "../agents/roster";
import { openPanel } from "../panels/store";
import { dispatchSlash } from "../panels/PanelHost";
import { SLASH_COMMANDS } from "../panels/slashCommands";
import { fuzzyScore, shortPath } from "../panels/ui/format";

type Item = { id: string; group: string; label: string; hint?: string; keywords?: string; run: () => void };

/** Ctrl/⌘+K palette: navigation, live sessions, and every builtin slash command. */
export function CommandPalette() {
  const open = usePalette((s) => s.open);
  const setOpen = usePalette((s) => s.setOpen);
  const route = useRoute();
  const roster = useRoster((s) => s.roster);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLUListElement>(null);
  const activeSessionId = route.name === "session" ? route.activeSessionId : undefined;

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
    return retainRoster();
  }, [open]);

  const items = useMemo<Item[]>(() => {
    const close = (fn: () => void) => () => { setOpen(false); fn(); };
    const nav: Item[] = [
      { id: "nav-sessions", group: "Go to", label: "Home", hint: "g h", run: close(() => navigate({ name: "sessions" })) },
      { id: "nav-new", group: "Go to", label: "New session", run: close(() => openPanel("new")) },
    ];
    const sessions: Item[] = roster
      .filter((e) => e.summary.activeSessionId && !isSubagent(e) && !isUnnamedEmpty(e))
      .map((e) => ({
        id: `session-${e.agentId}`,
        group: "Active sessions",
        label: sessionTitle(e),
        hint: shortPath(e.summary.cwd, 2),
        keywords: String(e.summary.cwd ?? ""),
        run: close(() => navigate({ name: "session", activeSessionId: e.summary.activeSessionId })),
      }));
    const slash: Item[] = SLASH_COMMANDS.filter((c) => c.owner !== "none").map((c) => ({
      id: `slash-${c.name}`,
      group: "Commands",
      label: `/${c.name}`,
      hint: c.description,
      keywords: `${c.aliases?.join(" ") ?? ""} ${c.description}`,
      run: close(() => dispatchSlash(c.name, "", activeSessionId)),
    }));
    return [...nav, ...sessions, ...slash];
  }, [roster, activeSessionId, setOpen]);

  const results = useMemo(() => {
    const q = query.trim().replace(/^\//, "");
    if (!q) return items;
    return items
      .map((item) => ({ item, score: fuzzyScore(q, `${item.label} ${item.keywords ?? ""}`) }))
      .filter((r): r is { item: Item; score: number } => r.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((r) => r.item);
  }, [items, query]);

  useEffect(() => setIndex(0), [query]);
  useEffect(() => { list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }); }, [index]);

  if (!open) return null;
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") { event.stopPropagation(); setOpen(false); }
    else if (event.key === "ArrowDown") { event.preventDefault(); setIndex((i) => Math.min(results.length - 1, i + 1)); }
    else if (event.key === "ArrowUp") { event.preventDefault(); setIndex((i) => Math.max(0, i - 1)); }
    else if (event.key === "Enter") { event.preventDefault(); results[index]?.run(); }
  };

  let lastGroup = "";
  return (
    <div className="fixed inset-0 z-[60] flex items-start justify-center bg-black/60 p-3 pt-[12vh] backdrop-blur-[2px] bh-fade-in" onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}>
      <div role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={onKeyDown} className="flex max-h-[70vh] w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl">
        <input
          autoFocus
          className="border-b border-line bg-transparent px-4 py-3 text-base outline-none placeholder:text-dim"
          placeholder="Type a command, session or page…"
          aria-label="Command palette search"
          role="combobox"
          aria-expanded
          aria-controls="bh-palette-list"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <ul id="bh-palette-list" ref={list} role="listbox" className="min-h-0 flex-1 overflow-y-auto p-1.5">
          {results.length === 0 && <li className="px-3 py-6 text-center text-sm text-muted">Nothing matches</li>}
          {results.map((item, i) => {
            const header = !query.trim() && item.group !== lastGroup ? item.group : "";
            lastGroup = item.group;
            return (
              <li key={item.id} role="presentation">
                {header && <div className="px-3 pb-1 pt-2 text-[0.7rem] font-medium uppercase tracking-wider text-dim">{header}</div>}
                <button role="option" aria-selected={i === index} onMouseMove={() => setIndex(i)} onClick={item.run} className={`flex w-full items-baseline gap-3 rounded-lg px-3 py-2 text-left text-sm ${i === index ? "bg-selected" : ""}`}>
                  <span className="shrink-0 font-medium">{item.label}</span>
                  {item.hint && <span className="min-w-0 flex-1 truncate text-xs text-dim">{item.hint}</span>}
                  {query.trim() && <span className="shrink-0 text-[0.65rem] text-dim">{item.group}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
