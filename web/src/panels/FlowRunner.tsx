import { useEffect, useRef, useState } from "react";
import { type FlowRequest } from "./api";
import { isWebUrl, type FlowOutcome } from "./flowModel";
import { useFlow } from "./useFlow";
import { ErrorLine, Spinner } from "./ui/bits";
import { toast } from "./toasts";

/** Starts `request` on mount and renders the flow: progress, sign-in link, prompts, and the result. */
export function FlowRunner({ request, onFinished }: { request: FlowRequest; onFinished: (outcome: FlowOutcome) => void }) {
  const { view, running, start, answer, cancel } = useFlow();
  const started = useRef(false);
  const [value, setValue] = useState("");

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void start(request);
  }, [request, start]);

  const outcome = view.outcome;
  const finished = useRef(onFinished);
  finished.current = onFinished;
  useEffect(() => {
    if (outcome) finished.current(outcome);
  }, [outcome]);

  const submit = () => {
    const text = value.trim();
    if (!text && !view.prompt?.allowEmpty) return;
    setValue("");
    void answer(text);
  };

  return (
    <div className="bh-card flex flex-col gap-2 border-primary/50 p-3">
      {view.progress.map((line, index) => <p key={index} className="text-xs text-dim">{line}</p>)}
      {view.authUrl && (
        <div className="flex flex-col gap-1.5 text-sm">
          <p className="text-muted">{view.authUrl.instructions ?? "Complete the sign-in in your browser."}</p>
          {isWebUrl(view.authUrl.url) ? (
            <a className="break-all text-info underline" href={view.authUrl.url} target="_blank" rel="noreferrer">Open the sign-in page</a>
          ) : (
            <span className="break-all font-mono text-xs">{view.authUrl.url}</span>
          )}
          <button type="button" className="bh-btn self-start" onClick={() => void navigator.clipboard.writeText(view.authUrl!.url).then(() => toast.success("Link copied"))}>Copy link</button>
        </div>
      )}
      {view.waiting && <p className="text-sm text-info">{view.waiting}</p>}
      {view.prompt && (
        <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <label className="text-sm text-muted">{view.prompt.prompt}</label>
          <input className="bh-input font-mono" type={view.prompt.masked ? "password" : "text"} autoComplete="off" autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
          <div className="flex justify-end gap-2">
            <button type="button" className="bh-btn" onClick={() => void answer(null)}>Cancel</button>
            <button type="submit" className="bh-btn bh-btn-primary" disabled={!value.trim() && !view.prompt.allowEmpty}>Submit</button>
          </div>
        </form>
      )}
      {running && !view.prompt && (
        <div className="flex items-center gap-2">
          <Spinner label="Working…" />
          <button type="button" className="bh-btn ml-auto" onClick={() => void cancel()}>Cancel</button>
        </div>
      )}
      {outcome?.kind === "error" && <ErrorLine>{outcome.message}</ErrorLine>}
      {outcome?.kind === "status" && <p className="text-sm">{outcome.message}</p>}
      {outcome?.kind === "cancelled" && <p className="text-sm text-dim">Cancelled.</p>}
    </div>
  );
}
