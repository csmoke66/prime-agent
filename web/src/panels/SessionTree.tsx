import { useMemo, useState } from "react";
import { wire } from "../wire";
import { navigate } from "../app/router";
import { Modal } from "./ui/Modal";
import { Badge, Empty, ErrorLine, SearchInput, Spinner, Tabs } from "./ui/bits";
import { relativeTime } from "./ui/format";
import { useAsync } from "./useAsync";
import { announceRewrite } from "./store";
import { toast, errorText } from "./toasts";
import { buildTree, describeEntry, entryRole, FILTER_MODES, messageText, visibleRows, type FilterMode, type FlatNode } from "./treeModel";

type TreeData = { flatNodes?: FlatNode[]; leafId?: string | null };

function prefill(activeSessionId: string, text: string | undefined) {
  if (text) window.dispatchEvent(new CustomEvent("bh:composer-prefill", { detail: { activeSessionId, text } }));
}

function finishRewrite(activeSessionId: string, text?: string) {
  announceRewrite(activeSessionId);
  prefill(activeSessionId, text);
  navigate({ name: "session", activeSessionId });
}

/** `/tree`: branch navigator with filters, search, folding, labels and optional branch summaries. */
export function TreePanel({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const tree = useAsync(() => wire.call<TreeData>({ type: "get_session_tree", activeSessionId }), [activeSessionId]);
  const [mode, setMode] = useState<FilterMode>("default");
  const [search, setSearch] = useState("");
  const [folded, setFolded] = useState<Set<string>>(new Set());
  const [target, setTarget] = useState<string | null>(null);
  const [labelFor, setLabelFor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const roots = useMemo(() => buildTree(tree.data?.flatNodes ?? []), [tree.data]);
  const leafId = tree.data?.leafId ?? null;
  const rows = useMemo(() => visibleRows(roots, { mode, leafId, search, folded }), [roots, mode, leafId, search, folded]);

  const navigateTo = async (targetId: string, summarize: boolean, customInstructions: string) => {
    setBusy(true);
    setError("");
    try {
      const data = await wire.call({ type: "navigate_tree", activeSessionId, targetId, summarize, customInstructions: customInstructions || undefined }, 180_000);
      if (data?.aborted) return setError("Branch summarization cancelled");
      if (data?.cancelled) return setError("Navigation cancelled");
      toast.success("Navigated to selected point");
      finishRewrite(activeSessionId, data?.editorText);
      onClose();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  const setLabel = async (entryId: string, label: string) => {
    try {
      await wire.call({ type: "set_session_entry_label", activeSessionId, entryId, label: label || null });
      setLabelFor(null);
      tree.reload();
    } catch (reason) {
      setError(errorText(reason));
    }
  };

  return (
    <Modal title="Session tree" subtitle="Pick a point to continue from. The active branch is highlighted." size="xl" variant="drawer" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <Tabs tabs={FILTER_MODES.map((m) => ({ id: m, label: m }))} active={mode} onChange={setMode} />
        <SearchInput value={search} onChange={setSearch} placeholder="Filter entries…" autoFocus />
        <ErrorLine>{tree.error || error}</ErrorLine>
        {tree.loading && <Spinner label="Loading tree" />}
        {!tree.loading && rows.length === 0 && <Empty title="No entries to show" hint="Try another filter." />}
        <ul className="flex flex-col gap-0.5" role="tree">
          {rows.map(({ node, indent, onPath, hasChildren }) => {
            const entry = node.node.entry;
            const isLeaf = node.id === leafId;
            return (
              <li key={node.id} role="treeitem" aria-selected={isLeaf} style={{ paddingLeft: `${indent * 1.25}rem` }}>
                <div className={`group flex items-center gap-2 rounded-lg px-2 py-1 ${isLeaf ? "bg-selected" : "hover:bg-selected/60"}`}>
                  <button
                    className={`w-4 text-xs text-dim ${hasChildren ? "" : "invisible"}`}
                    aria-label={folded.has(node.id) ? "Unfold branch" : "Fold branch"}
                    onClick={() => setFolded((prev) => { const next = new Set(prev); if (!next.delete(node.id)) next.add(node.id); return next; })}
                  >
                    {folded.has(node.id) ? "▸" : "▾"}
                  </button>
                  <span className={`size-1.5 shrink-0 rounded-full ${onPath ? "bg-primary-soft" : "bg-grid"}`} />
                  <button
                    className={`min-w-0 flex-1 truncate text-left text-sm ${entryRole(entry) === "user" || entry.message?.role === "user" ? "text-fg" : "text-muted"}`}
                    title={messageText(entry) || describeEntry(entry)}
                    disabled={busy || isLeaf}
                    onClick={() => setTarget(node.id)}
                  >
                    {describeEntry(entry)}
                  </button>
                  {node.node.label && <Badge tone="primary">{node.node.label}</Badge>}
                  {isLeaf && <Badge tone="success">current</Badge>}
                  <span className="hidden text-xs text-dim sm:inline">{relativeTime(node.timestamp)}</span>
                  <button className="bh-btn px-2! py-0.5! text-xs opacity-0 focus:opacity-100 group-hover:opacity-100" onClick={() => setLabelFor(node.id)}>
                    {node.node.label ? "Relabel" : "Label"}
                  </button>
                </div>
                {labelFor === node.id && (
                  <LabelEditor initial={node.node.label ?? ""} onSave={(label) => void setLabel(node.id, label)} onCancel={() => setLabelFor(null)} />
                )}
                {target === node.id && (
                  <NavigateConfirm busy={busy} onCancel={() => setTarget(null)} onConfirm={(summarize, instructions) => void navigateTo(node.id, summarize, instructions)} />
                )}
              </li>
            );
          })}
        </ul>
      </div>
    </Modal>
  );
}

function LabelEditor({ initial, onSave, onCancel }: { initial: string; onSave: (label: string) => void; onCancel: () => void }) {
  const [value, setValue] = useState(initial);
  return (
    <form className="my-1 ml-8 flex gap-2" onSubmit={(e) => { e.preventDefault(); onSave(value.trim()); }}>
      <input className="bh-input py-1!" value={value} autoFocus placeholder="Label (empty removes)" onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), onCancel())} />
      <button className="bh-btn bh-btn-primary" type="submit">Save</button>
      <button className="bh-btn" type="button" onClick={onCancel}>Cancel</button>
    </form>
  );
}

function NavigateConfirm({ busy, onConfirm, onCancel }: { busy: boolean; onConfirm: (summarize: boolean, instructions: string) => void; onCancel: () => void }) {
  const [summarize, setSummarize] = useState(false);
  const [instructions, setInstructions] = useState("");
  return (
    <div className="bh-card my-1 ml-8 flex flex-col gap-2 p-3 text-sm">
      <label className="flex items-center gap-2"><input type="checkbox" checked={summarize} onChange={(e) => setSummarize(e.target.checked)} /> Summarize the branch you leave</label>
      {summarize && <input className="bh-input" placeholder="Summary instructions (optional)" value={instructions} onChange={(e) => setInstructions(e.target.value)} />}
      <div className="flex gap-2">
        <button className="bh-btn bh-btn-primary" disabled={busy} onClick={() => onConfirm(summarize, instructions)}>{busy ? "Navigating…" : "Continue from here"}</button>
        <button className="bh-btn" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

/** `/fork`: choose a previous user message; the session continues from just before it. */
export function ForkPanel({ activeSessionId, onClose }: { activeSessionId: string; onClose: () => void }) {
  const messages = useAsync(() => wire.call<{ messages?: { entryId: string; text: string }[] }>({ type: "get_user_messages_for_forking", activeSessionId }), [activeSessionId]);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const list = messages.data?.messages ?? [];

  const fork = async (entryId: string) => {
    setBusy(entryId);
    try {
      const data = await wire.call({ type: "fork", activeSessionId, entryId }, 180_000);
      if (data?.cancelled) return setError("Fork cancelled");
      toast.success("Forked to new session");
      finishRewrite(activeSessionId, data?.selectedText);
      onClose();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy("");
    }
  };

  return (
    <Modal title="Fork from a message" subtitle="The new session starts before the message you pick; its text returns to the composer." size="lg" onClose={onClose}>
      <div className="flex flex-col gap-2">
        <ErrorLine>{messages.error || error}</ErrorLine>
        {messages.loading && <Spinner label="Loading messages" />}
        {!messages.loading && list.length === 0 && <Empty title="No messages to fork from" />}
        {[...list].reverse().map((message) => (
          <button key={message.entryId} disabled={Boolean(busy)} onClick={() => void fork(message.entryId)} className="bh-card px-3 py-2 text-left text-sm hover:border-primary disabled:opacity-50">
            <span className="line-clamp-3 whitespace-pre-wrap break-words">{message.text}</span>
            {busy === message.entryId && <span className="ml-2 text-xs text-muted">forking…</span>}
          </button>
        ))}
      </div>
    </Modal>
  );
}

/** `/clone`: duplicate the session at its current position. */
export async function cloneSession(activeSessionId: string): Promise<void> {
  try {
    const tree = await wire.call<TreeData>({ type: "get_session_tree", activeSessionId });
    if (!tree?.leafId) return toast.info("Nothing to clone yet");
    const data = await wire.call({ type: "fork", activeSessionId, entryId: tree.leafId, position: "at" }, 180_000);
    if (data?.cancelled) return;
    toast.success("Cloned to new session");
    finishRewrite(activeSessionId);
  } catch (reason) {
    toast.error(`Clone failed: ${errorText(reason)}`);
  }
}
