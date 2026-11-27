import { useState } from "react";
import { wire } from "../wire";
import { Modal } from "./ui/Modal";
import { ErrorLine, Spinner } from "./ui/bits";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";

/** `/edit_context`: edit the model-visible message array as JSON; the next turn uses the saved text. */
export function ContextEditPanel({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const loaded = useAsync(async () => {
    const data = await wire.call<{ context: { messages: unknown } }>({ type: "get_session_context", activeSessionId });
    return JSON.stringify(data.context.messages, null, 2);
  }, [activeSessionId]);
  const [draft, setDraft] = useState<string>();
  const [error, setError] = useState("");
  const text = draft ?? loaded.data ?? "";

  const save = async () => {
    setError("");
    let messages: unknown;
    try {
      messages = JSON.parse(text);
    } catch (reason) {
      return setError(`Invalid JSON: ${errorText(reason)}`);
    }
    if (!Array.isArray(messages)) return setError("The context must be a JSON array of messages.");
    try {
      const result = await wire.call<{ changed?: boolean }>({ type: "set_session_context", activeSessionId, messages }, 60_000);
      if (result?.changed === false) toast.info("No changes to the context");
      else toast.success("Context updated; the next turn uses it");
      onClose();
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  return (
    <Modal title="Edit context" subtitle="The messages the model sees, as JSON. Wait for the current response to finish first." size="lg" onClose={onClose} footer={<button className="bh-btn bh-btn-primary" disabled={loaded.loading} onClick={() => void save()}>Save</button>}>
      {loaded.loading ? <Spinner label="Loading context" /> : (
        <textarea className="h-[60vh] w-full rounded border border-grid/60 bg-transparent p-2 font-mono text-xs" spellCheck={false} value={text} onChange={(e) => setDraft(e.target.value)} />
      )}
      <ErrorLine>{loaded.error || error}</ErrorLine>
    </Modal>
  );
}
