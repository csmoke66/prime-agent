import { useSyncExternalStore } from "react";
import { wire } from "../wire";
import { errorText, toast } from "../panels/toasts";

// Presentation groups keyed by durable session id, shared with the TUI agents view through
// `<agent-dir>/agents-view-groups` (served by `/api/groups`). They never affect roster status.
const listeners = new Set<() => void>();

export type Groups = Record<string, string>;

let cache: Groups = {};

function publish(next: Groups): void {
  cache = next;
  listeners.forEach((l) => l());
}

async function load(): Promise<void> {
  try {
    const response = await fetch("/api/groups", { credentials: "same-origin" });
    if (response.ok) publish(await response.json());
  } catch {
    /* offline: keep the last groups */
  }
}

export async function setGroup(sessionId: string, label: string): Promise<void> {
  try {
    const response = await fetch("/api/groups", {
      method: "PUT",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ sessionId, label: label.trim() }),
    });
    if (!response.ok) throw new Error(await response.text());
    publish(await response.json());
  } catch (error) {
    toast.error(`Could not save the group: ${errorText(error)}`);
  }
}

let started = false;

export function useGroups(): Groups {
  if (!started) {
    started = true;
    void load();
    wire.onStatus((status) => {
      if (status === "ready") void load();
    });
  }
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => cache,
  );
}
