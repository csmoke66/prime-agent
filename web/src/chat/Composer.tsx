import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../wire";
import { BUILTIN_COMMANDS, filterCommands, parseBang, pathTokenAt, type SlashCommand } from "./model";
import { abort, abortBash, runBash, sendPrompt, toast, useChat, type ImageAttachment, type SendMode } from "./store";
import { runSlash } from "./slash";

const HISTORY_KEY = "bh:prompt-history";
const stashKey = (id: string) => `bh:stash:${id}`;

const loadHistory = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]");
  } catch {
    return [];
  }
};
const pushHistory = (text: string) => {
  const history = loadHistory().filter((h) => h !== text);
  history.push(text);
  localStorage.setItem(HISTORY_KEY, JSON.stringify(history.slice(-100)));
};

const readImage = (file: File): Promise<ImageAttachment> =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => {
      const url = String(reader.result);
      resolve({ data: url.slice(url.indexOf(",") + 1), mimeType: file.type || "image/png", name: file.name });
    };
    reader.readAsDataURL(file);
  });

interface Suggestion {
  label: string;
  detail?: string;
  insert: string;
  /** Range of the draft replaced by `insert`. */
  from: number;
  to: number;
}

export function Composer() {
  const activeSessionId = useChat((s) => s.activeSessionId);
  const state = useChat((s) => s.data.state);
  const commands = useChat((s) => s.commands);
  const insert = useChat((s) => s.composerInsert);
  const [text, setText] = useState("");
  const [images, setImages] = useState<ImageAttachment[]>([]);
  const [caret, setCaret] = useState(0);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [selected, setSelected] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [dragging, setDragging] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const historyIndex = useRef<number | null>(null);
  const draftBeforeHistory = useRef("");
  const streaming = !!state.isStreaming;
  const bash = parseBang(text) ?? (text.trimStart().startsWith("!") ? { command: "", excludeFromContext: false } : null);

  const setDraft = useCallback((value: string, moveCaret = true) => {
    setText(value);
    setDismissed(false);
    if (moveCaret) requestAnimationFrame(() => {
      area.current?.setSelectionRange(value.length, value.length);
      setCaret(value.length);
    });
  }, []);

  useEffect(() => {
    if (insert) setDraft(text ? `${text}\n${insert.text}` : insert.text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [insert?.nonce]);

  useEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 260)}px`;
  }, [text]);

  useEffect(() => {
    const onFocus = () => area.current?.focus();
    window.addEventListener("bh:chat-focus", onFocus);
    onFocus();
    return () => window.removeEventListener("bh:chat-focus", onFocus);
  }, []);

  // Autocomplete: slash commands at the start of the draft, `@path` tokens anywhere.
  const skillCommands = useMemo<SlashCommand[]>(
    () => commands.filter((c) => c.source === "skill").map((c) => ({ name: String(c.name), description: String(c.description ?? "Skill"), argumentHint: "<request>", kind: "session" as const })),
    [commands],
  );
  useEffect(() => {
    if (dismissed) return setSuggestions([]);
    const slash = /^\/([^\s]*)$/.exec(text.slice(0, caret));
    if (slash && caret === text.length) {
      const list = filterCommands([...BUILTIN_COMMANDS, ...skillCommands], slash[1]).slice(0, 8);
      setSuggestions(list.map((c) => ({ label: `/${c.name}`, detail: `${c.argumentHint ? c.argumentHint + " — " : ""}${c.description}`, insert: `/${c.name}${c.argumentHint ? " " : ""}`, from: 0, to: text.length })));
      setSelected(0);
      return;
    }
    const token = pathTokenAt(text, caret);
    if (!token) return setSuggestions([]);
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      try {
        const cwd = useChat.getState().data.state.cwd ?? "";
        const result = await api<any>(`/files?cwd=${encodeURIComponent(cwd)}&prefix=${encodeURIComponent(token.query)}`);
        const files: string[] = (Array.isArray(result) ? result : (result.files ?? result.entries ?? [])).map((f: any) => (typeof f === "string" ? f : f.path ?? f.name));
        if (cancelled) return;
        setSuggestions(files.slice(0, 8).map((f) => ({ label: f, insert: `@${f}${f.endsWith("/") ? "" : " "}`, from: token.start, to: caret })));
        setSelected(0);
      } catch {
        if (!cancelled) setSuggestions([]);
      }
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [text, caret, dismissed, skillCommands]);

  const accept = (s: Suggestion) => {
    const next = text.slice(0, s.from) + s.insert + text.slice(s.to);
    setText(next);
    const pos = s.from + s.insert.length;
    requestAnimationFrame(() => {
      area.current?.focus();
      area.current?.setSelectionRange(pos, pos);
      setCaret(pos);
    });
  };

  const addFiles = async (files: FileList | File[]) => {
    const picked = [...files].filter((f) => f.type.startsWith("image/"));
    if (!picked.length) return;
    try {
      const loaded = await Promise.all(picked.map(readImage));
      setImages((prev) => [...prev, ...loaded]);
    } catch {
      toast("Could not read the image", "error");
    }
  };

  const submit = async (mode: SendMode) => {
    const value = text.trim();
    if (!value && !images.length) return;
    const attached = images;
    setText("");
    setImages([]);
    historyIndex.current = null;
    const restore = () => {
      setText(value);
      setImages(attached);
    };
    const bangCommand = parseBang(value);
    if (bangCommand) {
      pushHistory(value);
      await runBash(bangCommand.command, bangCommand.excludeFromContext);
      return;
    }
    if (value.startsWith("/") && !attached.length && (await runSlash(value, mode))) {
      pushHistory(value);
      return;
    }
    pushHistory(value);
    if (!(await sendPrompt(value, mode, attached))) restore();
    else window.dispatchEvent(new Event("bh:chat-follow"));
  };

  const browseHistory = (direction: -1 | 1): boolean => {
    const history = loadHistory();
    if (!history.length) return false;
    if (historyIndex.current === null) {
      if (direction === 1) return false;
      draftBeforeHistory.current = text;
      historyIndex.current = history.length;
    }
    const next = historyIndex.current + direction;
    if (next < 0) return true;
    if (next >= history.length) {
      historyIndex.current = null;
      setDraft(draftBeforeHistory.current);
      return true;
    }
    historyIndex.current = next;
    setDraft(history[next]);
    return true;
  };

  const stash = () => {
    if (!activeSessionId) return;
    const key = stashKey(activeSessionId);
    const saved = localStorage.getItem(key);
    if (text.trim()) {
      localStorage.setItem(key, text);
      setText("");
      toast("Draft stashed — Ctrl+S restores it");
    } else if (saved !== null) {
      localStorage.removeItem(key);
      setDraft(saved);
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const open = suggestions.length > 0;
    if (open) {
      if (event.key === "ArrowDown") { event.preventDefault(); return setSelected((selected + 1) % suggestions.length); }
      if (event.key === "ArrowUp") { event.preventDefault(); return setSelected((selected - 1 + suggestions.length) % suggestions.length); }
      if (event.key === "Tab" || (event.key === "Enter" && !event.shiftKey && !event.altKey && suggestions[selected].insert !== text)) {
        event.preventDefault();
        return accept(suggestions[selected]);
      }
      if (event.key === "Escape") { event.preventDefault(); return setDismissed(true); }
    }
    if (event.key === "Enter" && !event.nativeEvent.isComposing) {
      if (event.altKey || event.ctrlKey || event.metaKey) { event.preventDefault(); return void submit("followUp"); }
      if (!event.shiftKey) { event.preventDefault(); return void submit("steer"); }
    }
    if (event.key === "Escape") {
      if (bash && state.isBashRunning) return void abortBash();
      if (streaming) return void abort();
      if (text || images.length) { setText(""); setImages([]); }
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") { event.preventDefault(); return stash(); }
    if (event.key === "ArrowUp" && !event.shiftKey && !event.altKey && (text === "" || historyIndex.current !== null) && area.current?.selectionStart === 0 && browseHistory(-1)) return event.preventDefault();
    if (event.key === "ArrowDown" && !event.shiftKey && !event.altKey && historyIndex.current !== null && browseHistory(1)) return event.preventDefault();
    if (event.key === "c" && event.ctrlKey && streaming && area.current?.selectionStart === area.current?.selectionEnd) { event.preventDefault(); void abort(); }
  };

  const border = bash ? "border-success/60" : dragging ? "border-primary" : "border-grid/60 focus-within:border-primary-soft";
  return (
    <div className="border-t border-grid/30 bg-surface px-2 pb-1.5 pt-1.5">
      <div className="w-full">
        <div
          className={`relative rounded border bg-panel transition-colors ${border}`}
          onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
          onDragLeave={() => setDragging(false)}
          onDrop={(e) => { e.preventDefault(); setDragging(false); void addFiles(e.dataTransfer.files); }}
        >
          {suggestions.length ? (
            <ul role="listbox" className="absolute inset-x-0 bottom-full z-20 mb-2 max-h-72 overflow-auto rounded border border-grid/50 bg-surface p-0.5 shadow-xl">
              {suggestions.map((s, i) => (
                <li key={s.label} role="option" aria-selected={i === selected}>
                  <button type="button" className={`flex w-full items-baseline gap-3 rounded px-2 py-0.5 text-left text-[12px] ${i === selected ? "bg-selected" : ""}`} onMouseEnter={() => setSelected(i)} onMouseDown={(e) => { e.preventDefault(); accept(s); }}>
                    <span className="font-mono text-primary-soft">{s.label}</span>
                    {s.detail ? <span className="truncate text-xs text-dim">{s.detail}</span> : null}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {images.length ? (
            <div className="flex flex-wrap gap-2 px-3 pt-3">
              {images.map((image, i) => (
                <div key={i} className="group relative">
                  <img src={`data:${image.mimeType};base64,${image.data}`} alt={image.name ?? "attachment"} className="h-16 w-16 rounded-lg border border-grid/50 object-cover" />
                  <button type="button" aria-label="Remove image" className="absolute -right-1.5 -top-1.5 grid h-5 w-5 place-items-center rounded-full bg-error text-[11px] text-white" onClick={() => setImages(images.filter((_, j) => j !== i))}>×</button>
                </div>
              ))}
            </div>
          ) : null}
          <div className="flex items-end gap-1 p-1">
            {!bash ? <span className="mb-1.5 ml-1 select-none font-mono font-bold text-primary-soft" aria-hidden>❯</span> : null}
            {bash ? <span className="mb-1.5 ml-1 font-mono text-success" title={bash.excludeFromContext ? "Output excluded from context" : "Output joins the context"}>{bash.excludeFromContext ? "!!" : "!"}</span> : null}
            <textarea
              ref={area}
              value={text}
              rows={1}
              aria-label="Message"
              placeholder={streaming ? "Steer…" : "Message · / · @ · !"}
              className="max-h-64 min-h-[1.75rem] flex-1 resize-none bg-transparent px-2 py-1 font-mono text-[13px] leading-5 outline-none placeholder:text-dim focus-visible:!outline-none"
              onChange={(e) => { setText(e.target.value); setCaret(e.target.selectionStart); setDismissed(false); if (historyIndex.current !== null && e.target.value !== loadHistory()[historyIndex.current]) historyIndex.current = null; }}
              onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
              onKeyDown={onKeyDown}
              onPaste={(e) => {
                const files = [...e.clipboardData.files].filter((f) => f.type.startsWith("image/"));
                if (files.length) { e.preventDefault(); void addFiles(files); }
              }}
            />
            <input ref={fileInput} type="file" accept="image/*" multiple hidden onChange={(e) => { if (e.target.files) void addFiles(e.target.files); e.target.value = ""; }} />
            <button type="button" className="bh-btn !border-transparent !bg-transparent px-2" aria-label="Attach image" title="Attach image" onClick={() => fileInput.current?.click()}>📎</button>
            {streaming ? (
              <>
                <button type="button" className="bh-btn" title="Queue as follow-up (Alt+Enter)" disabled={!text.trim()} onClick={() => void submit("followUp")}>Queue</button>
                <button type="button" className="bh-btn bh-btn-danger" title="Interrupt (Esc)" onClick={() => void abort()}>■ Stop</button>
              </>
            ) : null}
            <button type="button" className="bh-btn bh-btn-primary" disabled={!text.trim() && !images.length} onClick={() => void submit("steer")} aria-label="Send">{streaming ? "Steer" : "Send"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
