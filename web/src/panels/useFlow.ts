import { useCallback, useEffect, useRef, useState } from "react";
import { rest, type FlowRequest } from "./api";
import { applyEvents, emptyFlow, type FlowView } from "./flowModel";
import { errorText } from "./toasts";

/** Runs one credential flow on the backend and follows its events until it ends. */
export function useFlow() {
  const [view, setView] = useState<FlowView>(emptyFlow);
  const [running, setRunning] = useState(false);
  const flowId = useRef<number | null>(null);
  const live = useRef(true);

  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      if (flowId.current !== null) void rest.cancelFlow(flowId.current).catch(() => undefined);
    };
  }, []);

  const start = useCallback(async (request: FlowRequest) => {
    setView(emptyFlow);
    setRunning(true);
    try {
      const { flowId: id } = await rest.startFlow(request);
      flowId.current = id;
      let since = 0;
      while (live.current) {
        const batch = await rest.pollFlow(id, since);
        since = batch.next;
        let done = false;
        if (batch.events.length > 0) {
          done = batch.events.some((event) => event.type === "done");
          setView((current) => applyEvents(current, batch.events));
        }
        if (done) break;
      }
    } catch (reason) {
      setView((current) => ({ ...current, outcome: { kind: "error", message: errorText(reason) } }));
    } finally {
      flowId.current = null;
      setRunning(false);
    }
  }, []);

  const answer = useCallback(async (value: string | null) => {
    setView((current) => ({ ...current, prompt: undefined }));
    if (flowId.current !== null) await rest.answerFlow(flowId.current, value);
  }, []);

  const cancel = useCallback(async () => {
    if (flowId.current !== null) await rest.cancelFlow(flowId.current);
  }, []);

  return { view, running, start, answer, cancel };
}
