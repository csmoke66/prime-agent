export type FlowOutcome = { kind: "status" | "error"; message: string } | { kind: "cancelled" };
export type FlowEvent =
  | { type: "progress"; message: string }
  | { type: "waiting"; message: string }
  | { type: "authUrl"; url: string; instructions: string | null }
  | { type: "prompt"; prompt: string; masked: boolean; allowEmpty: boolean }
  | { type: "done"; outcome: FlowOutcome };

/** What the page shows for one running credential flow. */
export type FlowView = {
  progress: string[];
  waiting?: string;
  authUrl?: { url: string; instructions: string | null };
  prompt?: { prompt: string; masked: boolean; allowEmpty: boolean };
  outcome?: FlowOutcome;
};

export const emptyFlow: FlowView = { progress: [] };

/** Fold newly polled events into the view. Any event after a prompt means the prompt was answered. */
export function applyEvents(view: FlowView, events: FlowEvent[]): FlowView {
  let next = view;
  for (const event of events) {
    const settled = event.type === "prompt" ? next : { ...next, prompt: undefined };
    switch (event.type) {
      case "progress": next = { ...settled, progress: [...settled.progress, event.message] }; break;
      case "waiting": next = { ...settled, waiting: event.message }; break;
      case "authUrl": next = { ...settled, authUrl: { url: event.url, instructions: event.instructions } }; break;
      case "prompt": next = { ...settled, prompt: { prompt: event.prompt, masked: event.masked, allowEmpty: event.allowEmpty } }; break;
      case "done": next = { ...settled, waiting: undefined, outcome: event.outcome }; break;
    }
  }
  return next;
}

/** Only http(s) sign-in links are rendered as clickable. */
export const isWebUrl = (url: string) => /^https?:\/\//i.test(url);
