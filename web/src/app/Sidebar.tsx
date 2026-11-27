import { useEffect, useMemo, useRef, useState } from "react";
import { wire } from "../wire";
import { navigate, useRoute } from "./router";
import { retainRoster, useRoster } from "../agents/rosterStore";
import { activityLabel, buildForest, descendantCount, flatten, isUnnamedEmpty, modelLabel, sessionTitle, type AgentNode, type RosterEntry } from "../agents/roster";
import { inactiveEntries, sidebarSections } from "../agents/sidebarModel";
import { setGroup, useGroups } from "../agents/groups";
import { PersistentShells } from "../agents/Shells";
import { destructiveKind, destructiveLabel, openEntry, runDestructive, runRename } from "../agents/useAgentActions";
import { InlineRename } from "../panels/InlineRename";
import { openPanel } from "../panels/store";
import { useSavedSessions } from "../panels/savedSessions";
import { toast, errorText } from "../panels/toasts";
import { relativeTime, shortPath } from "../panels/ui/format";
import { setLayout } from "./layout";

const PULSE = ["◇", "◈", "◆", "◈"];

function StatusGlyph({ status, tick }: { status: RosterEntry["status"]; tick: number }) {
  if (status === "running") return <span className="w-3 shrink-0 text-center text-fg" aria-label="running">{PULSE[tick % 4]}</span>;
  return <span className={`w-3 shrink-0 text-center ${status === "idle" ? "text-warning" : "text-dim"}`} aria-label={status}>&bull;</span>;
}

type RowProps = {
  node: AgentNode;
  tick: number;
  searching: boolean;
  active: boolean;
  focused: boolean;
  collapsed: boolean;
  renaming: "name" | "group" | null;
  group?: string;
  onToggle: () => void;
  onOpen: () => void;
  onMode: (mode: "name" | "group" | null) => void;
};

function Row({ node, tick, searching, active, focused, collapsed, renaming, group, onToggle, onOpen, onMode }: RowProps) {
  const entry = node.entry;
  const s = entry.summary;
  const subs = descendantCount(node);
  const goal = s.goal?.status ?? s.goalStatus;
  const queued = Number(s.queuedCount ?? 0);
  const destructive = destructiveLabel(entry);
  const dim = entry.status === "inactive";
  const meta = [activityLabel(entry), modelLabel(s).replace(/^[^/]*\//, ""), relativeTime(s.lastActivityAt ?? s.modified ?? s.created)].filter(Boolean).join(" · ");
  return (
    <li role="treeitem" aria-selected={active} data-agent-id={entry.agentId} aria-expanded={node.children.length ? !collapsed : undefined}>
      <div
        className={`group relative flex cursor-pointer items-start gap-1 border-l-2 py-1 pr-1.5 ${focused ? "outline outline-1 -outline-offset-1 outline-primary-soft" : ""} ${active ? "border-primary-soft bg-selected" : "border-transparent hover:bg-selected/60"} ${dim && !active ? "opacity-60 hover:opacity-100" : ""}`}
        style={{ paddingLeft: searching ? 6 : 4 + node.depth * 12 }}
        onClick={onOpen}
      >
        <button className={`mt-0.5 w-3 shrink-0 text-[10px] text-dim ${node.children.length && !searching ? "" : "invisible"}`} aria-label={collapsed ? "Expand subagents" : "Collapse subagents"} onClick={(e) => { e.stopPropagation(); onToggle(); }}>{collapsed ? "▸" : "▾"}</button>
        <span className="mt-px"><StatusGlyph status={entry.status} tick={tick} /></span>
        <div className="min-w-0 flex-1">
          {renaming ? (
            <InlineRename
              initial={renaming === "name" ? sessionTitle(entry) : group ?? ""}
              onCancel={() => onMode(null)}
              onSave={(value) => {
                if (renaming === "group") { setGroup(String(s.sessionId), value); onMode(null); }
                else void runRename(entry, value).then(() => onMode(null));
              }}
            />
          ) : (
            <div className="flex items-center gap-1.5">
              <span className={`truncate text-[13px] ${active ? "font-medium text-fg" : "text-fg/90"}`} title={sessionTitle(entry)} onDoubleClick={(e) => { e.stopPropagation(); onMode("name"); }}>{sessionTitle(entry)}</span>
              {goal === "active" ? <span className="shrink-0 text-[11px] text-success" title="Goal active">◎</span> : null}
              {queued > 0 ? <span className="shrink-0 rounded bg-warning/15 px-1 text-[10px] text-warning" title="Queued / inbox messages">{queued}</span> : null}
              {subs > 0 ? <span className={`shrink-0 rounded px-1 text-[10px] ${entry.status === "running" ? "bg-success/15 text-success" : "bg-grid/30 text-muted"}`} title={`${subs} subagents`}>{subs}</span> : null}
            </div>
          )}
          <div className="truncate text-[11px] text-dim" title={`${s.cwd ?? ""}`}>{node.depth === 0 && s.cwd ? `${shortPath(s.cwd, 1)} · ` : ""}{meta}</div>
        </div>
        {!renaming ? (
          <span className="absolute right-1 top-1 hidden items-center gap-0.5 rounded bg-surface/95 px-0.5 group-hover:flex" onClick={(e) => e.stopPropagation()}>
            <button className="rounded px-1 text-dim hover:text-fg" title="Rename (double-click)" onClick={() => onMode("name")}>✎</button>
            {s.sessionId && !s.rlmChildId ? <button className="rounded px-1 text-dim hover:text-fg" title="Move to group" onClick={() => onMode("group")}>#</button> : null}
            {destructive && destructiveKind(entry) ? (
              <button className="rounded px-1 text-dim hover:text-error" title={destructive} onClick={() => { if (window.confirm(`${destructive} "${sessionTitle(entry)}"?`)) void runDestructive(entry); }}>✕</button>
            ) : null}
          </span>
        ) : null}
      </div>
    </li>
  );
}

export function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const route = useRoute();
  const roster = useRoster((s) => s.roster);
  const ready = useRoster((s) => s.ready);
  const groups = useGroups();
  const saved = useSavedSessions();
  const [query, setQuery] = useState("");
  const [content, setContent] = useState(false);
  const [contentIds, setContentIds] = useState<Set<string> | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [folded, setFolded] = useState<Set<string>>(new Set(["status:inactive"]));
  const [editing, setEditing] = useState<{ id: string; mode: "name" | "group" } | null>(null);
  const [showShells, setShowShells] = useState(false);
  const [tick, setTick] = useState(0);
  const activeId = route.name === "session" ? route.activeSessionId : "";

  useEffect(() => retainRoster(), []);
  useEffect(() => {
    const id = window.setInterval(() => setTick((n) => n + 1), 350);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    const q = query.trim();
    if (!content || q.length < 2) return setContentIds(null);
    let live = true;
    const timer = window.setTimeout(() => {
      wire.call({ type: "search_session_content", query: q }, 120_000).then(
        (d) => live && setContentIds(new Set<string>(d?.sessionIds ?? [])),
        (e) => toast.error(`Message search failed: ${errorText(e)}`),
      );
    }, 300);
    return () => { live = false; window.clearTimeout(timer); };
  }, [query, content]);

  const entries = useMemo(() => [...roster.filter((e) => !isUnnamedEmpty(e)), ...inactiveEntries(roster, saved.state.sessions)], [roster, saved.state.sessions]);
  const forest = useMemo(() => buildForest(entries), [entries]);
  const searching = query.trim() !== "";
  const found = useMemo(() => {
    if (!searching) return null;
    const all = flatten(forest, new Set());
    if (content && contentIds) return all.filter((n) => contentIds.has(String(n.entry.summary.sessionId)));
    if (content) return [];
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    return all.filter((n) => {
      const hay = `${sessionTitle(n.entry)} ${n.entry.summary.cwd ?? ""} ${modelLabel(n.entry.summary)} ${activityLabel(n.entry)}`.toLowerCase();
      return terms.every((t) => hay.includes(t));
    });
  }, [forest, searching, content, contentIds, query]);
  const sections = useMemo(() => sidebarSections(forest, groups), [forest, groups]);
  const counts = useMemo(() => ({ running: entries.filter((e) => e.status === "running").length, total: entries.length }), [entries]);

  const toggle = (set: Set<string>, id: string, write: (next: Set<string>) => void) => {
    const next = new Set(set);
    if (!next.delete(id)) next.add(id);
    write(next);
  };

  const visible = useMemo(
    () => found ?? sections.filter((s) => !folded.has(s.key)).flatMap((s) => flatten(s.roots, collapsed)),
    [found, sections, folded, collapsed],
  );
  const [cursor, setCursor] = useState<string | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);

  /** The agents-view keys of the TUI, on the focused tree: arrows move/fold, Enter opens, Space replies, Ctrl+R/Alt+G/Ctrl+X manage. */
  const onTreeKey = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input, textarea")) return;
    const index = visible.findIndex((n) => n.entry.agentId === (cursor ?? visible.find((v) => Boolean(activeId) && v.entry.summary.activeSessionId === activeId)?.entry.agentId));
    const node = visible[index];
    const move = (to: number) => { const next = visible[Math.max(0, Math.min(visible.length - 1, to))]; if (next) { setCursor(next.entry.agentId); treeRef.current?.querySelector(`[data-agent-id="${CSS.escape(next.entry.agentId)}"]`)?.scrollIntoView({ block: "nearest" }); } };
    const open = (reply: boolean) => node && void openEntry(node.entry, (sid) => navigate({ name: "session", activeSessionId: sid })).then(() => { onNavigate?.(); if (reply) window.setTimeout(() => window.dispatchEvent(new Event("bh:chat-focus")), 100); });
    const handled = (): void => e.preventDefault();
    if (e.key === "ArrowDown") { handled(); move(index + 1); }
    else if (e.key === "ArrowUp") { handled(); move(index < 0 ? 0 : index - 1); }
    else if (!node) return;
    else if (e.key === "ArrowRight") { handled(); if (node.children.length && collapsed.has(node.entry.agentId)) toggle(collapsed, node.entry.agentId, setCollapsed); else open(false); }
    else if (e.key === "ArrowLeft") {
      handled();
      if (node.children.length && !collapsed.has(node.entry.agentId)) toggle(collapsed, node.entry.agentId, setCollapsed);
      else { const parent = visible.slice(0, index).reverse().find((n) => n.depth < node.depth); if (parent) setCursor(parent.entry.agentId); }
    }
    else if (e.key === "Enter") { handled(); open(false); }
    else if (e.key === " ") { handled(); open(true); }
    else if (e.ctrlKey && e.key.toLowerCase() === "r") { handled(); setEditing({ id: node.entry.agentId, mode: "name" }); }
    else if (e.altKey && e.code === "KeyG" && node.entry.summary.sessionId && !node.entry.summary.rlmChildId) { handled(); setEditing({ id: node.entry.agentId, mode: "group" }); }
    else if (e.ctrlKey && e.key.toLowerCase() === "x") {
      handled();
      const label = destructiveLabel(node.entry);
      if (label && destructiveKind(node.entry) && window.confirm(`${label} "${sessionTitle(node.entry)}"?`)) void runDestructive(node.entry);
    }
    else if (e.ctrlKey && e.key.toLowerCase() === "n") { handled(); openPanel("new"); onNavigate?.(); }
    else if (e.ctrlKey && e.key.toLowerCase() === "f") { handled(); setContent((v) => !v); filterRef.current?.focus(); }
    else if (e.key === "/") { handled(); filterRef.current?.focus(); }
  };

  const row = (node: AgentNode, flat: boolean) => {
    const entry = node.entry;
    const id = entry.agentId;
    return (
      <Row
        key={id}
        node={node}
        tick={tick}
        searching={flat}
        active={Boolean(activeId) && entry.summary.activeSessionId === activeId}
        focused={cursor === id}
        collapsed={collapsed.has(id)}
        renaming={editing?.id === id ? editing.mode : null}
        group={groups[String(entry.summary.sessionId)]}
        onToggle={() => toggle(collapsed, id, setCollapsed)}
        onOpen={() => void openEntry(entry, (sid) => navigate({ name: "session", activeSessionId: sid })).then(onNavigate)}
        onMode={(mode) => setEditing(mode ? { id, mode } : null)}
      />
    );
  };

  return (
    <aside className="flex h-full min-h-0 flex-col bg-surface" aria-label="Sessions and agents">
      <div className="flex items-center gap-1.5 px-2 pb-1.5 pt-2">
        <input
          ref={filterRef}
          className="bh-input !py-1"
          placeholder={content ? "Search message text…" : "Filter agents…"}
          aria-label="Filter agents"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Escape") setQuery(""); if (e.key === "ArrowDown") { e.preventDefault(); treeRef.current?.focus(); } }}
        />
        <button className={`bh-btn !px-1.5 ${content ? "!border-primary text-primary-soft" : ""}`} title="Search message text" aria-pressed={content} onClick={() => setContent(!content)}>¶</button>
      </div>
      <div className="flex items-center gap-1.5 px-2 pb-2">
        <button className="bh-btn bh-btn-primary flex-1 justify-center" onClick={() => { openPanel("new"); onNavigate?.(); }}>+ New session</button>
        <button className="bh-btn" title="Import a session file" onClick={() => openPanel("import")}>Import</button>
      </div>
      <div className="flex items-center justify-between border-y border-grid/25 px-2 py-1 text-[11px] text-dim">
        <span>{counts.running} running &middot; {counts.total} total</span>
        {saved.state.loading ? <span className="bh-pulse">scanning saved…</span> : saved.state.remaining > 0 ? <button className="hover:text-fg" onClick={() => void saved.loadMore()}>+{saved.state.remaining} more</button> : null}
      </div>
      <div ref={treeRef} className="min-h-0 flex-1 overflow-y-auto pb-2 outline-none" role="tree" aria-label="Agents" tabIndex={0} onKeyDown={onTreeKey} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget)) setCursor(null); }}>
        {!ready && !entries.length ? <p className="px-3 py-3 text-xs text-dim bh-pulse">Loading agents…</p> : null}
        {ready && !entries.length && !saved.state.loading ? <p className="px-3 py-3 text-xs text-dim">No agents yet. Start a new session.</p> : null}
        {found ? (
          <ul>{found.map((n) => row(n, true))}{!found.length ? <li className="px-3 py-3 text-xs text-dim">No matches</li> : null}</ul>
        ) : (
          sections.map((section) => {
            const isFolded = folded.has(section.key);
            return (
              <section key={section.key}>
                <button className="bh-section-title flex w-full items-center gap-1 px-2 pb-0.5 pt-2.5 text-left hover:text-muted" aria-expanded={!isFolded} onClick={() => toggle(folded, section.key, setFolded)}>
                  <span className="w-3 text-[10px]">{isFolded ? "▸" : "▾"}</span>{section.title}<span className="font-normal opacity-70">{section.roots.length}</span>
                </button>
                {isFolded ? null : <ul>{flatten(section.roots, collapsed).map((n) => row(n, false))}</ul>}
              </section>
            );
          })
        )}
      </div>
      <div className="border-t border-grid/25">
        <button className="bh-section-title flex w-full items-center gap-1 px-2 py-1.5 text-left hover:text-muted" aria-expanded={showShells} onClick={() => setShowShells(!showShells)}>
          <span className="w-3 text-[10px]">{showShells ? "▾" : "▸"}</span>Persistent shells
        </button>
        {showShells ? <div className="max-h-56 overflow-y-auto px-2 pb-2"><PersistentShells /></div> : null}
        <div className="flex items-center gap-1 border-t border-grid/25 px-2 py-1.5">
          <button className="bh-btn !border-transparent !bg-transparent" onClick={() => openPanel("heartbeats")}>Heartbeats</button>
          <button className="bh-btn !border-transparent !bg-transparent" onClick={() => openPanel("settings")}>Settings</button>
          <button className="bh-btn ml-auto !border-transparent !bg-transparent" title="Collapse sidebar" aria-label="Collapse sidebar" onClick={() => setLayout({ sidebarOpen: false })}>«</button>
        </div>
      </div>
    </aside>
  );
}
