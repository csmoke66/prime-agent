import { useEffect } from "react";
import { create } from "zustand";
import { wire, type Frame } from "../wire";
import { applyRosterUpdate, type RosterEntry } from "./roster";

type RosterState = { roster: RosterEntry[]; ready: boolean; error: string };

export const useRoster = create<RosterState>(() => ({ roster: [], ready: false, error: "" }));

let users = 0;
let offFrame: (() => void) | null = null;
let offStatus: (() => void) | null = null;

async function subscribe(): Promise<void> {
  try {
    const data = await wire.call<{ roster?: RosterEntry[] }>({ type: "roster_subscribe" });
    useRoster.setState({ roster: data?.roster ?? [], ready: true, error: "" });
  } catch (error) {
    useRoster.setState({ error: error instanceof Error ? error.message : String(error) });
  }
}

function onFrame(frame: Frame): void {
  if (frame.type === "roster_update") {
    useRoster.setState((state) => ({ roster: applyRosterUpdate(state.roster, frame as any), ready: true }));
  }
}

/** Reference-counted live roster: subscribes on first use, re-subscribes after reconnects. */
export function retainRoster(): () => void {
  if (users++ === 0) {
    offFrame = wire.onFrame(onFrame);
    offStatus = wire.onStatus((status) => {
      if (status === "ready") void subscribe();
      else useRoster.setState({ ready: false });
    });
    if (wire.status === "ready") void subscribe();
  }
  return () => {
    if (--users > 0) return;
    offFrame?.();
    offStatus?.();
    offFrame = offStatus = null;
    if (wire.status === "ready") void wire.request({ type: "roster_unsubscribe" }).catch(() => undefined);
    useRoster.setState({ roster: [], ready: false });
  };
}

/** Hook form: subscribes while the calling component is mounted. */
export function useRosterSubscription(): RosterState {
  useEffect(() => retainRoster(), []);
  return useRoster();
}
