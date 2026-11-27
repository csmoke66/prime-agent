import { useSyncExternalStore } from "react";

// Local presentation groups keyed by durable session id (the TUI keeps these in `agents-view-groups`;
// the browser keeps them in localStorage). They never affect roster status.
const KEY = "bh.agentGroups";
const listeners = new Set<() => void>();

export type Groups = Record<string, string>;

export function readGroups(): Groups {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

let cache = readGroups();

export function withGroup(groups: Groups, sessionId: string, label: string): Groups {
  const next = { ...groups };
  if (label.trim()) next[sessionId] = label.trim();
  else delete next[sessionId];
  return next;
}

export function setGroup(sessionId: string, label: string): void {
  cache = withGroup(cache, sessionId, label);
  localStorage.setItem(KEY, JSON.stringify(cache));
  listeners.forEach((l) => l());
}

export function useGroups(): Groups {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => cache,
  );
}
