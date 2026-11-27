import { useEffect, useMemo, useRef, useState } from "react";
import { wire } from "../wire";
import { navigate } from "../app/router";
import { useRosterSubscription } from "./rosterStore";
import { activityLabel, buildForest, isUnnamedEmpty, modelLabel, descendantCount, flatten, sessionTitle, type AgentNode, type RosterEntry } from "./roster";
import { setGroup, useGroups } from "./groups";
import { AgentDetail } from "./AgentDetail";
import { PersistentShells } from "./Shells";
import { destructiveKind, destructiveLabel, openEntry, runDestructive, runRename } from "./useAgentActions";
import { InlineRename } from "../panels/InlineRename";
import { openPanel } from "../panels/store";
import { toast, errorText } from "../panels/toasts";
import { Badge, ConfirmButton, Empty, ErrorLine, SearchInput, Spinner, StatusDot, Tabs, type Tone } from "../panels/ui/bits";
import { fmtCost, fuzzyFilter, relativeTime, shortPath } from "../panels/ui/format";

const TONE: Record<RosterEntry["status"], Tone> = { running: "success", idle: "info", inactive: "neutral" };
const UNGROUPED = "";

export function AgentsView() {
  const { roster, ready, error } = useRosterSubscription();
  const groups = useGroups();
  const [tab, setTab] = useState<"agents" | "shells">("agents");
  const [query, setQuery] = useState("");
  const [content, setContent] = useState(false);
  const [contentIds, setContentIds] = useState<Set<string> | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [grouping, setGrouping] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);

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

  const forest = useMemo(() => buildForest(roster.filter((e) => !isUnnamedEmpty(e))), [roster]);
  const searching = query.trim() !== "" || (content && contentIds !== null);
  const rows: AgentNode[] = useMemo(() => {
    if (!searching) return flatten(forest, collapsed);
    const all = flatten(forest, new Set());
    if (content && contentIds) return all.filter((n) => contentIds.has(String(n.entry.summary.sessionId)));
    return fuzzyFilter(all, query, (n) => `${sessionTitle(n.entry)} ${n.entry.summary.cwd ?? ""} ${modelLabel(n.entry.summary)} ${activityLabel(n.entry)}`);
  }, [forest, collapsed, searching, content, contentIds, query]);

  // Group sections only apply to top-level rows; a row's descendants follow it.
  const sections = useMemo(() => {
    const out = new Map<string, AgentNode[]>();
    let label = UNGROUPED;
    for (const node of rows) {
      if (node.depth === 0 || searching) label = groups[String(node.entry.summary.sessionId)] ?? UNGROUPED;
      out.set(label, [...(out.get(label) ?? []), node]);
    }
    return [...out.entries()].sort(([a], [b]) => (a === UNGROUPED ? -1 : b === UNGROUPED ? 1 : a.localeCompare(b)));
  }, [rows, groups, searching]);

  const selectedEntry = roster.find((e) => e.agentId === selected);
  const detailEntry = roster.find((e) => e.agentId === detail);
  const order = sections.flatMap(([, list]) => list);

  const move = (delta: number) => {
    const index = order.findIndex((n) => n.entry.agentId === selected);
    const next = order[Math.min(order.length - 1, Math.max(0, index < 0 ? 0 : index + delta))];
    if (next) {
      setSelected(next.entry.agentId);
      document.getElementById(`agent-${next.entry.agentId}`)?.scrollIntoView({ block: "nearest" });
    }
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    const target = event.target as HTMLElement;
    const typing = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
    if (event.key === "/" && !typing) { event.preventDefault(); search.current?.focus(); return; }
    if (event.key === "ArrowDown" || (event.key === "j" && !typing)) { event.preventDefault(); move(1); }
    else if (event.key === "ArrowUp" || (event.key === "k" && !typing)) { event.preventDefault(); move(-1); }
    else if (typing || !selectedEntry) return;
    else if (event.key === "Enter") void openEntry(selectedEntry, (id) => navigate({ name: "session", activeSessionId: id }));
    else if (event.key === "ArrowRight") {
      const node = order.find((n) => n.entry.agentId === selected);
      if (node?.children.length && collapsed.has(node.entry.agentId)) toggle(node.entry.agentId);
      else setDetail(selectedEntry.agentId);
    } else if (event.key === "ArrowLeft") {
      const node = order.find((n) => n.entry.agentId === selected);
      if (node?.children.length && !collapsed.has(node.entry.agentId)) toggle(node.entry.agentId);
    }
    else if (event.key === " ") { event.preventDefault(); setDetail(selectedEntry.agentId); }
    else if ((event.ctrlKey && event.key === "r") || event.key === "F2") { event.preventDefault(); setRenaming(selectedEntry.agentId); }
    else if (event.altKey && event.key === "g") { event.preventDefault(); setGrouping(selectedEntry.agentId); }
    else if (event.ctrlKey && event.key === "n") { event.preventDefault(); openPanel("new"); }
  };

  const toggle = (id: string) => setCollapsed((prev) => { const next = new Set(prev); if (!next.delete(id)) next.add(id); return next; });
  const counts = useMemo(() => ({ running: roster.filter((e) => e.status === "running").length, total: roster.length }), [roster]);

  return (
    <div className="mx-auto flex h-full w-full max-w-5xl flex-col gap-3 px-4 py-5" onKeyDown={onKeyDown}>
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="mr-auto text-xl font-semibold tracking-tight">Agents <span className="ml-2 text-sm font-normal text-muted">{counts.running} running · {counts.total} total</span></h1>
        <button className="bh-btn" onClick={() => openPanel("heartbeats", { activeSessionId: selectedEntry?.summary.activeSessionId })}>Heartbeats</button>
        <button className="bh-btn bh-btn-primary" onClick={() => openPanel("new")}>+ New session</button>
      </div>
      <Tabs tabs={[{ id: "agents", label: "Agents" }, { id: "shells", label: "Persistent shells" }]} active={tab} onChange={setTab} />
      {tab === "shells" ? (
        <div className="min-h-0 flex-1 overflow-y-auto"><PersistentShells /></div>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-3">
            <div className="min-w-56 flex-1" ref={(el) => { const input = el?.querySelector("input"); if (input) search.current = input; }}>
              <SearchInput value={query} onChange={setQuery} placeholder={content ? "Search message text…  ( / )" : "Search agents…  ( / )"} />
            </div>
            <label className="flex items-center gap-2 text-sm text-muted"><input type="checkbox" checked={content} onChange={(e) => setContent(e.target.checked)} /> message text</label>
          </div>
          <ErrorLine>{error}</ErrorLine>
          <div className="min-h-0 flex-1 overflow-y-auto pb-6" role="tree" aria-label="Agents">
            {!ready && !error && <Spinner label="Loading agents" />}
            {ready && rows.length === 0 && <Empty title={searching ? "No agents match" : "No agents yet"} hint="Start a session to see it here." />}
            {sections.map(([label, list]) => (
              <section key={label || "ungrouped"} className="mb-3">
                {(label !== UNGROUPED || sections.length > 1) && <h2 className="mb-1 px-1 text-xs font-medium uppercase tracking-wider text-dim">{label || "Ungrouped"} · {list.filter((n) => n.depth === 0).length}</h2>}
                <ul className="flex flex-col gap-1">
                  {list.map((node) => (
                    <AgentRow
                      key={node.entry.agentId}
                      node={node}
                      searching={searching}
                      selected={selected === node.entry.agentId}
                      collapsed={collapsed.has(node.entry.agentId)}
                      renaming={renaming === node.entry.agentId}
                      grouping={grouping === node.entry.agentId}
                      group={groups[String(node.entry.summary.sessionId)]}
                      onSelect={() => setSelected(node.entry.agentId)}
                      onToggle={() => toggle(node.entry.agentId)}
                      onDetail={() => setDetail(node.entry.agentId)}
                      onRename={() => setRenaming(node.entry.agentId)}
                      onGroup={() => setGrouping(node.entry.agentId)}
                      onDone={() => { setRenaming(null); setGrouping(null); }}
                    />
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </>
      )}
      {detailEntry && <AgentDetail entry={detailEntry} onClose={() => setDetail(null)} />}
    </div>
  );
}

function AgentRow(props: {
  node: AgentNode; searching: boolean; selected: boolean; collapsed: boolean; renaming: boolean; grouping: boolean; group?: string;
  onSelect: () => void; onToggle: () => void; onDetail: () => void; onRename: () => void; onGroup: () => void; onDone: () => void;
}) {
  const { node, selected } = props;
  const entry = node.entry;
  const s = entry.summary;
  const descendants = descendantCount(node);
  const destructive = destructiveLabel(entry);
  const cost = fmtCost(s.cost ?? s.usage?.cost);
  return (
    <li id={`agent-${entry.agentId}`} role="treeitem" aria-selected={selected} aria-expanded={node.children.length ? !props.collapsed : undefined} style={{ marginLeft: props.searching ? 0 : `${node.depth * 1.25}rem` }}>
      <div className={`bh-card bh-fade-in flex flex-wrap items-center gap-2 px-2.5 py-2 ${selected ? "border-primary bg-selected" : "hover:border-primary/50"}`} onClick={props.onSelect}>
        <button className={`w-4 text-xs text-dim ${node.children.length && !props.searching ? "" : "invisible"}`} aria-label={props.collapsed ? "Expand subagents" : "Collapse subagents"} onClick={(e) => { e.stopPropagation(); props.onToggle(); }}>{props.collapsed ? "▸" : "▾"}</button>
        <StatusDot tone={TONE[entry.status]} />
        <div className="min-w-40! flex-1 basis-48">
          {props.renaming ? (
            <InlineRename initial={sessionTitle(entry)} onCancel={props.onDone} onSave={(name) => void runRename(entry, name).then(props.onDone)} />
          ) : props.grouping ? (
            <InlineRename initial={props.group ?? ""} onCancel={props.onDone} onSave={(label) => { setGroup(String(s.sessionId), label); props.onDone(); }} />
          ) : (
            <button className="block max-w-full truncate text-left text-sm font-medium" onDoubleClick={() => void openEntry(entry, (id) => navigate({ name: "session", activeSessionId: id }))} onClick={props.onDetail}>
              {sessionTitle(entry)}
            </button>
          )}
          <p className="truncate text-xs text-dim">
            {activityLabel(entry)}{s.cwd ? ` · ${shortPath(s.cwd, 2)}` : ""}{modelLabel(s) ? ` · ${modelLabel(s)}` : ""}{cost ? ` · ${cost}` : ""} · {relativeTime(s.lastActivityAt ?? s.modified ?? s.created)}
          </p>
        </div>
        {props.group && <Badge tone="primary">{props.group}</Badge>}
        {descendants > 0 && <Badge tone={entry.status === "running" ? "success" : "neutral"}>{descendants} sub</Badge>}
        {s.runtimeKind === "subagent" && <Badge>subagent</Badge>}
        <Badge tone={TONE[entry.status]}>{entry.status}</Badge>
        <div className="flex gap-1">
          <button className="bh-btn px-2! py-1!" onClick={(e) => { e.stopPropagation(); void openEntry(entry, (id) => navigate({ name: "session", activeSessionId: id })); }}>Open</button>
          <button className="bh-btn px-2! py-1!" onClick={(e) => { e.stopPropagation(); props.onRename(); }}>Rename</button>
          {s.sessionId && !s.rlmChildId && <button className="bh-btn hidden px-2! py-1! sm:inline-flex" onClick={(e) => { e.stopPropagation(); props.onGroup(); }}>Group</button>}
          {destructive && destructiveKind(entry) && <span onClick={(e) => e.stopPropagation()}><ConfirmButton label={destructive} confirmLabel={`${destructive}?`} className="px-2 py-1" onConfirm={() => void runDestructive(entry)} /></span>}
        </div>
      </div>
    </li>
  );
}
