import { wire } from "../wire";
import { deleteSaved, killSession, renameLive, renameSaved, resumeSaved } from "../panels/sessionActions";
import { sessionTitle, isSubagent, type RosterEntry } from "./roster";
import { toast, errorText } from "../panels/toasts";

const str = (v: unknown) => (typeof v === "string" ? v : "");

export type AgentAction = "stop" | "delete" | "rename" | "open";

/** What the destructive button does for an entry: stop live work, delete a subagent, or delete a saved file. */
export function destructiveKind(entry: RosterEntry): "stop-subagent" | "delete-subagent" | "stop-agent" | "delete-saved" | null {
  const s = entry.summary;
  if (isSubagent(entry) && s.rlmChildId && s.parentActiveSessionId) return entry.status === "running" ? "stop-subagent" : "delete-subagent";
  if (s.activeSessionId) return "stop-agent";
  if (s.sessionFile || s.path) return "delete-saved";
  return null;
}

export const destructiveLabel = (entry: RosterEntry): string => {
  switch (destructiveKind(entry)) {
    case "stop-subagent":
    case "stop-agent":
      return "Stop";
    case "delete-subagent":
    case "delete-saved":
      return "Delete";
    case null:
      return "";
  }
};

export async function runDestructive(entry: RosterEntry): Promise<void> {
  const s = entry.summary;
  try {
    switch (destructiveKind(entry)) {
      case "stop-subagent":
        await wire.call({ type: "cancel_rlm_child", activeSessionId: s.parentActiveSessionId, childId: s.rlmChildId });
        toast.success(`Stopped ${sessionTitle(entry)}`);
        break;
      case "delete-subagent":
        await wire.call({ type: "delete_rlm_subagent", activeSessionId: s.parentActiveSessionId, childId: s.rlmChildId });
        toast.success(`Deleted ${sessionTitle(entry)}`);
        break;
      case "stop-agent":
        await killSession(s.activeSessionId);
        toast.success(`Stopped ${sessionTitle(entry)}`);
        break;
      case "delete-saved":
        await deleteSaved(str(s.sessionFile) || str(s.path));
        toast.success(`Deleted ${sessionTitle(entry)}`);
        break;
      case null:
        break;
    }
  } catch (error) {
    toast.error(errorText(error));
  }
}

export async function runRename(entry: RosterEntry, name: string): Promise<boolean> {
  const s = entry.summary;
  try {
    if (s.activeSessionId) await renameLive(s.activeSessionId, name);
    else await renameSaved(str(s.sessionFile) || str(s.path), name);
    return true;
  } catch (error) {
    toast.error(errorText(error));
    return false;
  }
}

export async function openEntry(entry: RosterEntry, openLive: (id: string) => void): Promise<void> {
  const s = entry.summary;
  try {
    if (s.activeSessionId) openLive(s.activeSessionId);
    else await resumeSaved({ path: str(s.sessionFile) || str(s.path), cwd: str(s.cwd) || "/" });
  } catch (error) {
    toast.error(errorText(error));
  }
}
