import { useCallback, useEffect, useReducer, useRef } from "react";
import { wire, type Frame } from "../wire";
import { errorText } from "./toasts";

export type SavedSession = {
  path: string;
  id: string;
  cwd: string;
  name?: string;
  created: string;
  modified: string;
  messageCount: number;
  firstMessage: string;
  parentSessionPath?: string;
  rlmDepth?: number;
  usage?: { cost?: number; [key: string]: unknown };
  model?: { provider?: string; modelId?: string };
  [key: string]: any;
};

export type SavedState = {
  sessions: SavedSession[];
  loaded: number;
  total: number;
  loading: boolean;
  error: string;
  /** Resume token of a partially walked store. */
  cursor: string | null;
  remaining: number;
};

export const initialSaved: SavedState = { sessions: [], loaded: 0, total: 0, loading: false, error: "", cursor: null, remaining: 0 };

export type SavedAction =
  | { type: "start"; reset: boolean }
  | { type: "item"; session: SavedSession }
  | { type: "progress"; loaded: number; total: number }
  | { type: "done"; sessions: SavedSession[]; cursor: string | null; remaining: number }
  | { type: "fail"; error: string }
  | { type: "remove"; path: string }
  | { type: "rename"; path: string; name: string };

const byRecent = (a: SavedSession, b: SavedSession) => (b.modified || "").localeCompare(a.modified || "");

function upsert(list: SavedSession[], items: SavedSession[]): SavedSession[] {
  const next = [...list];
  for (const item of items) {
    const index = next.findIndex((existing) => existing.path === item.path);
    if (index >= 0) next[index] = item;
    else next.push(item);
  }
  return next.sort(byRecent);
}

export function savedReducer(state: SavedState, action: SavedAction): SavedState {
  switch (action.type) {
    case "start":
      return { ...(action.reset ? initialSaved : state), loading: true, error: "" };
    case "item":
      return { ...state, sessions: upsert(state.sessions, [action.session]) };
    case "progress":
      return { ...state, loaded: action.loaded, total: action.total };
    case "done":
      return { ...state, sessions: upsert(state.sessions, action.sessions), loading: false, cursor: action.cursor, remaining: action.remaining };
    case "fail":
      return { ...state, loading: false, error: action.error };
    case "remove":
      return { ...state, sessions: state.sessions.filter((s) => s.path !== action.path) };
    case "rename":
      return { ...state, sessions: state.sessions.map((s) => (s.path === action.path ? { ...s, name: action.name } : s)) };
  }
}

const PAGE = 400;

/** Saved-session catalog, streamed row by row while `list_saved_sessions` scans the store. */
export function useSavedSessions(cwd?: string) {
  const [state, dispatch] = useReducer(savedReducer, initialSaved);
  const running = useRef(false);
  const cursor = useRef<string | null>(null);
  cursor.current = state.cursor;

  useEffect(
    () =>
      wire.onFrame((frame: Frame) => {
        if (frame.command !== "list_saved_sessions" || frame.activeSessionId) return;
        if (frame.type === "session_list_item" && frame.session) dispatch({ type: "item", session: frame.session });
        else if (frame.type === "session_list_progress") dispatch({ type: "progress", loaded: frame.loaded ?? 0, total: frame.total ?? 0 });
      }),
    [],
  );

  const load = useCallback(
    async (more: boolean) => {
      if (running.current) return;
      running.current = true;
      dispatch({ type: "start", reset: !more });
      try {
        const data = await wire.call(
          { type: "list_saved_sessions", cwd: cwd || "/", scope: null, limit: PAGE, cursor: more ? cursor.current ?? undefined : undefined },
          600_000,
        );
        dispatch({ type: "done", sessions: data?.sessions ?? [], cursor: data?.nextCursor ?? null, remaining: data?.remaining ?? 0 });
      } catch (error) {
        dispatch({ type: "fail", error: errorText(error) });
      } finally {
        running.current = false;
      }
    },
    [cwd],
  );

  useEffect(() => {
    if (wire.status === "ready") void load(false);
  }, [load]);

  return { state, dispatch, reload: () => load(false), loadMore: () => load(true) };
}
