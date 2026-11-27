import { useEffect, useMemo, useState } from "react";
import { wire } from "../wire";
import { retainRoster, useRoster } from "../agents/rosterStore";
import { isSubagent, isUnnamedEmpty, modelLabel, sessionTitle, activityLabel, type RosterEntry } from "../agents/roster";
import { openSession, renameLive, renameSaved, deleteSaved, killSession, resumeSaved } from "./sessionActions";
import { useSavedSessions, type SavedSession } from "./savedSessions";
import { openPanel } from "./store";
import { toast, errorText } from "./toasts";
import { Badge, ConfirmButton, Empty, ErrorLine, SearchInput, Spinner, StatusDot, type Tone } from "./ui/bits";
import { fmtCost, fuzzyFilter, oneLine, relativeTime, shortPath } from "./ui/format";
import { InlineRename } from "./InlineRename";

const STATUS_TONE: Record<RosterEntry["status"], Tone> = { running: "success", idle: "info", inactive: "neutral" };

function useContentSearch(query: string, enabled: boolean): Set<string> | null {
  const [matches, setMatches] = useState<Set<string> | null>(null);
  useEffect(() => {
    const q = query.trim();
    if (!enabled || q.length < 2) {
      setMatches(null);
      return;
    }
    let live = true;
    const timer = window.setTimeout(() => {
      wire.call({ type: "search_session_content", query: q }, 120_000).then(
        (data) => live && setMatches(new Set<string>(data?.sessionIds ?? [])),
        (error) => live && toast.error(`Message search failed: ${errorText(error)}`),
      );
    }, 300);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [query, enabled]);
  return matches;
}

export function SessionPicker() {
  const [query, setQuery] = useState("");
  const [content, setContent] = useState(false);
  const [showChildren, setShowChildren] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const { state, dispatch, reload, loadMore } = useSavedSessions();
  const roster = useRoster((s) => s.roster);
  useEffect(() => retainRoster(), []);
  const matches = useContentSearch(query, content);

  const live = useMemo(() => roster.filter((entry) => entry.summary.activeSessionId && !isSubagent(entry) && !isUnnamedEmpty(entry)), [roster]);
  const liveKeys = useMemo(() => new Set(roster.flatMap((e) => [e.summary.sessionId, e.summary.sessionFile].filter(Boolean))), [roster]);

  const filteredLive = useMemo(
    () => fuzzyFilter(live, content ? "" : query, (e) => `${sessionTitle(e)} ${e.summary.cwd ?? ""} ${modelLabel(e.summary)}`),
    [live, query, content],
  );
  const saved = useMemo(() => {
    let list = state.sessions.filter((s) => !liveKeys.has(s.id) && !liveKeys.has(s.path) && (showChildren || !s.parentSessionPath));
    if (content && matches) list = list.filter((s) => matches.has(s.id));
    else if (!content) list = fuzzyFilter(list, query, (s) => `${s.name ?? ""} ${s.firstMessage} ${s.cwd} ${s.id}`);
    return list;
  }, [state.sessions, liveKeys, showChildren, content, matches, query]);

  const guard = (action: () => Promise<unknown>) => () => action().catch((error) => toast.error(errorText(error)));

  return (
    <div className="mx-auto flex h-full w-full max-w-5xl flex-col gap-4 overflow-y-auto px-4 py-5">
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="mr-auto text-xl font-semibold tracking-tight">Sessions</h1>
        <button className="bh-btn" onClick={() => openPanel("import")}>Import</button>
        <button className="bh-btn bh-btn-primary" onClick={() => openPanel("new")}>+ New session</button>
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-56 flex-1">
          <SearchInput value={query} onChange={setQuery} placeholder={content ? "Search message text…" : "Search sessions…"} autoFocus />
        </div>
        <label className="flex items-center gap-2 text-sm text-muted">
          <input type="checkbox" checked={content} onChange={(e) => setContent(e.target.checked)} /> message text
        </label>
        <label className="flex items-center gap-2 text-sm text-muted">
          <input type="checkbox" checked={showChildren} onChange={(e) => setShowChildren(e.target.checked)} /> subagent sessions
        </label>
      </div>

      {filteredLive.length > 0 && (
        <section aria-label="Active sessions" className="flex flex-col gap-2">
          <h2 className="text-xs font-medium uppercase tracking-wider text-dim">Active · {filteredLive.length}</h2>
          {filteredLive.map((entry) => {
            const s = entry.summary;
            const id: string = s.activeSessionId;
            return (
              <div key={entry.agentId} className="bh-card bh-fade-in flex flex-wrap items-center gap-3 px-3 py-2.5 hover:border-primary/60">
                <button className="flex min-w-48! flex-1 basis-56 flex-col items-start gap-0.5 text-left" onClick={() => void openSession(id)}>
                  {renaming === id ? (
                    <InlineRename
                      initial={sessionTitle(entry)}
                      onCancel={() => setRenaming(null)}
                      onSave={(name) => void renameLive(id, name).then(() => setRenaming(null), (e) => toast.error(errorText(e)))}
                    />
                  ) : (
                    <span className="flex max-w-full items-center gap-2 font-medium">
                      <StatusDot tone={STATUS_TONE[entry.status]} />
                      <span className="truncate">{sessionTitle(entry)}</span>
                    </span>
                  )}
                  <span className="max-w-full truncate text-xs text-dim">{shortPath(s.cwd, 3)} · {activityLabel(entry)}{modelLabel(s) ? ` · ${modelLabel(s)}` : ""}</span>
                </button>
                <Badge tone={STATUS_TONE[entry.status]}>{entry.status}</Badge>
                <div className="flex gap-1">
                  <button className="bh-btn px-2! py-1!" onClick={() => setRenaming(id)}>Rename</button>
                  <ConfirmButton label="Stop" confirmLabel="Stop?" onConfirm={guard(() => killSession(id))} />
                </div>
              </div>
            );
          })}
        </section>
      )}

      <section aria-label="Saved sessions" className="flex flex-col gap-2">
        <div className="flex items-center gap-3">
          <h2 className="text-xs font-medium uppercase tracking-wider text-dim">Saved · {saved.length}</h2>
          {state.loading && (
            <span className="flex items-center gap-2 text-xs text-muted">
              <Spinner /> {state.total ? `${state.loaded} / ${state.total}` : "scanning…"}
            </span>
          )}
          <button className="bh-btn ml-auto px-2! py-1!" onClick={reload} disabled={state.loading}>Refresh</button>
        </div>
        <ErrorLine>{state.error}</ErrorLine>
        {saved.length === 0 && !state.loading && <Empty title={query ? "No sessions match" : "No saved sessions yet"} hint="Start one with “New session”." />}
        {saved.map((session) => (
          <SavedRow
            key={session.path}
            session={session}
            renaming={renaming === session.path}
            onRename={() => setRenaming(session.path)}
            onCancel={() => setRenaming(null)}
            onSave={(name) =>
              void renameSaved(session.path, name).then(
                () => {
                  dispatch({ type: "rename", path: session.path, name });
                  setRenaming(null);
                },
                (e) => toast.error(errorText(e)),
              )
            }
            onOpen={guard(() => resumeSaved(session))}
            onDelete={guard(async () => {
              await deleteSaved(session.path);
              dispatch({ type: "remove", path: session.path });
              toast.success("Session deleted");
            })}
          />
        ))}
        {state.cursor && (
          <button className="bh-btn self-center" onClick={loadMore} disabled={state.loading}>
            Load more{state.remaining ? ` (${state.remaining} more files)` : ""}
          </button>
        )}
      </section>
    </div>
  );
}

function SavedRow(props: { session: SavedSession; renaming: boolean; onRename: () => void; onCancel: () => void; onSave: (name: string) => void; onOpen: () => void; onDelete: () => void }) {
  const { session } = props;
  const cost = fmtCost(session.usage?.cost);
  return (
    <div className="bh-card flex flex-wrap items-center gap-3 px-3 py-2.5 hover:border-primary/60">
      <button className="flex min-w-48! flex-1 basis-56 flex-col items-start gap-0.5 text-left" onClick={props.onOpen}>
        {props.renaming ? (
          <InlineRename initial={session.name ?? oneLine(session.firstMessage, 60)} onCancel={props.onCancel} onSave={props.onSave} />
        ) : (
          <span className="max-w-full truncate font-medium">{session.name || oneLine(session.firstMessage, 90) || session.id.slice(0, 8)}</span>
        )}
        {session.name && <span className="max-w-full truncate text-xs text-muted">{oneLine(session.firstMessage, 120)}</span>}
        <span className="max-w-full truncate text-xs text-dim">
          {shortPath(session.cwd, 3)} · {session.messageCount} msgs · {relativeTime(session.modified)}
          {cost ? ` · ${cost}` : ""}
          {session.model?.modelId ? ` · ${session.model.modelId}` : ""}
        </span>
      </button>
      {session.parentSessionPath && <Badge tone="primary">subagent</Badge>}
      <div className="flex gap-1">
        <button className="bh-btn px-2! py-1!" onClick={props.onOpen}>Resume</button>
        <button className="bh-btn px-2! py-1!" onClick={props.onRename}>Rename</button>
        <ConfirmButton label="Delete" confirmLabel="Delete?" onConfirm={props.onDelete} />
      </div>
    </div>
  );
}
