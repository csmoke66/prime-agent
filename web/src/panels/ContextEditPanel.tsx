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
    try {
      await wire.call({ type: "set_session_context", activeSessionId, messages: text }, 60_000);
      toast.success("Context updated; the next turn uses it");
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
