import { memo, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Markdown } from "./markdown";
import { Images, ToolCard, resultView } from "./tools";
import { assistantText, formatDuration, parseSkillBlock, resultsById, textOfContent, type Block, type Msg, type SessionData } from "./model";
import { abortRetry, loadOlder, useChat, type DetailLevel } from "./store";

const keys = new WeakMap<object, number>();
let nextKey = 0;
const keyOf = (m: Msg): string | number => m.key ?? (keys.get(m) ?? (keys.set(m, ++nextKey), nextKey));

const THINKING_COLOR: Record<string, string> = { off: "text-dim", minimal: "text-info", low: "text-info", medium: "text-primary-soft", high: "text-warning", xhigh: "text-error", max: "text-error" };

function Collapsible({ title, open: initial, children, tone = "text-muted" }: { title: ReactNode; open: boolean; children: ReactNode; tone?: string }) {
  const [override, setOverride] = useState<boolean | null>(null);
  const open = override ?? initial;
  return (
    <div className="my-0.5">
      <button type="button" className={`flex items-center gap-1.5 font-mono text-[11px] ${tone} hover:text-fg`} aria-expanded={open} onClick={() => setOverride(!open)}>
        <span className={`w-2.5 transition-transform ${open ? "rotate-90" : ""}`} aria-hidden>›</span>
        {title}
      </button>
      {open ? <div className="mt-0.5 border-l border-grid/40 pl-2.5">{children}</div> : null}
    </div>
  );
}

function UserRow({ message }: { message: Msg }) {
  const blocks: Block[] = Array.isArray(message.content) ? message.content : [{ type: "text", text: String(message.content ?? "") }];
  const text = textOfContent(blocks);
  const skill = parseSkillBlock(text);
  return (
    <div className="bh-fade-in mb-1 mt-3 border-l-2 border-primary-soft bg-user-msg">
      <div className="flex gap-2 px-2.5 py-1.5">
        <span className="select-none font-mono font-bold text-primary-soft" aria-hidden>❯</span>
        <div className="min-w-0 flex-1">
        {skill ? (
          <>
            <Collapsible title={<>skill <b className="font-mono">{skill.name}</b></>} open={false} tone="text-primary-soft">
              <div className="mb-1 font-mono text-[11px] text-dim">{skill.location}</div>
              <Markdown text={skill.content} className="text-sm" />
            </Collapsible>
            {skill.userMessage ? <div className="whitespace-pre-wrap break-words">{skill.userMessage}</div> : null}
          </>
        ) : text.startsWith("/") ? (
          <div className="whitespace-pre-wrap break-words"><span className="font-semibold text-primary-soft">{text.split(/\s/)[0]}</span>{text.slice(text.split(/\s/)[0].length)}</div>
        ) : (
          <div className="whitespace-pre-wrap break-words">{text}</div>
        )}
        <Images blocks={blocks} />
        </div>
      </div>
    </div>
  );
}

function AssistantBody({ message, results, data, detail, streaming }: { message: Msg; results: Map<string, Msg>; data: SessionData; detail: DetailLevel; streaming: boolean }) {
  const level = String(data.state.thinkingLevel ?? "medium");
  const blocks: Block[] = Array.isArray(message.content) ? message.content : [];
  const error = message.stopReason === "error" ? message.errorMessage : message.stopReason === "aborted" ? "Interrupted" : null;
  return (
    <div className="bh-fade-in my-0.5 pl-[1.15rem]">
      {blocks.map((block, i) => {
        if (block.type === "text") return block.text ? <div key={i} className="my-1"><Markdown text={block.text} /></div> : null;
        if (block.type === "thinking")
          return block.thinking ? (
            <Collapsible key={i} title={<span className={THINKING_COLOR[level] ?? "text-muted"}>thinking</span>} open={detail >= 1 || (streaming && i === blocks.length - 1)}>
              <div className="whitespace-pre-wrap text-[12px] italic text-[#8b8b94]">{block.thinking}</div>
            </Collapsible>
          ) : null;
        if (block.type === "toolCall") {
          const result = results.get(block.id);
          return <ToolCard key={block.id ?? i} call={block as unknown as { id: string; name: string; arguments?: any }} exec={data.tools[block.id]} result={result ? resultView(result) : null} detail={detail} />;
        }
        return null;
      })}
      {error ? <div className={`mt-1 rounded border px-2 py-1 text-[12px] ${message.stopReason === "error" ? "border-error/40 bg-tool-error text-error" : "border-warning/30 text-warning"}`}>{error}</div> : null}
      {!streaming && message.role === "assistant" && assistantText(message) ? <CopyButton text={assistantText(message)} /> : null}
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      className="text-[11px] text-dim opacity-0 transition-opacity hover:text-fg focus-visible:opacity-100 group-hover:opacity-100"
      onClick={() => void navigator.clipboard?.writeText(text).then(() => { setDone(true); window.setTimeout(() => setDone(false), 1200); })}
    >
      {done ? "Copied" : "Copy"}
    </button>
  );
}

function StatusRow({ kind, children }: { kind: string; children: ReactNode }) {
  const cls = kind === "error" ? "text-error" : kind === "warning" ? "text-warning" : "text-muted";
  return <div className={`bh-fade-in my-1 whitespace-pre-wrap break-words pl-[1.15rem] font-mono text-[11.5px] ${cls}`}>{children}</div>;
}

function BashRow({ m }: { m: Msg }) {
  const [open, setOpen] = useState(false);
  const failed = m.exitCode !== 0 && m.exitCode != null;
  return (
    <div className={`my-0.5 rounded-r border-l-2 ${failed ? "border-error bg-tool-error" : "border-success/70 bg-tool-success"}`}>
      <button type="button" className="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[12px]" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="font-semibold text-success">!</span>
        <code className="min-w-0 flex-1 truncate">{m.command}</code>
        {m.excludeFromContext ? <span className="text-[11px] text-dim">not in context</span> : null}
        <span className={`text-[11px] ${failed ? "text-error" : "text-dim"}`}>{m.cancelled ? "cancelled" : m.exitCode != null ? `exit ${m.exitCode}` : ""}</span>
      </button>
      {open || (m.output && m.output.length < 400) ? (
        <pre className="bh-pre max-h-96 overflow-auto whitespace-pre-wrap break-words border-t border-grid/25 !rounded-none px-2 text-[12px] text-muted">{m.output}{m.truncated && m.fullOutputPath ? `\n[truncated — full output: ${m.fullOutputPath}]` : ""}</pre>
      ) : null}
    </div>
  );
}

const STATUS_CUSTOM = new Set(["provider_retry_outcome", "model_prompt_error", "skill_problems", "compaction_outcome", "session_slash_command_result"]);
const INJECTED_CUSTOM = new Set(["heartbeat_prompt", "goal_context", "ipython_state_restored", "python_skills_unavailable"]);

function CustomRow({ m, detail }: { m: Msg; detail: DetailLevel }) {
  if (m.display === false) return null;
  const text = textOfContent(m.content);
  const type = String(m.customType ?? "");
  const d = m.details ?? {};
  if (type === "session_slash_command") return <UserRow message={{ role: "user", content: text }} />;
  if (STATUS_CUSTOM.has(type)) {
    const ok = d.success === true || d.outcome === "skipped";
    return <StatusRow kind={type === "model_prompt_error" || type === "skill_problems" || d.outcome === "skipped" ? "warning" : ok || type === "session_slash_command_result" ? "info" : "error"}>{text}</StatusRow>;
  }
  if (type === "agent_message" || type === "rlm_child_failure" || type === "rlm_child_terminal_notice") {
    const from = d.from ?? {};
    const who = from.sessionName ?? from.activeSessionId ?? d.childName ?? "agent";
    return (
      <div className="my-1 ml-[1.15rem] rounded border-l-2 border-info/60 bg-panel px-2 py-1 text-[13px]">
        <div className="mb-0.5 text-[10px] uppercase tracking-wider text-info">Agent message · {String(who)}</div>
        <Markdown text={String(d.message ?? text)} />
      </div>
    );
  }
  if (type === "inbox_notice") return <StatusRow kind="info">{text}</StatusRow>;
  if (type === "async_bash_completion")
    return (
      <Collapsible title={<>background shell finished{d.exitCode != null ? ` (exit ${d.exitCode})` : ""}</>} open={detail >= 2}>
        <pre className="bh-pre whitespace-pre-wrap text-[12px] text-muted">{text}</pre>
      </Collapsible>
    );
  if (INJECTED_CUSTOM.has(type))
    return (
      <Collapsible title={<>{type.replace(/_/g, " ")}</>} open={detail >= 2} tone="text-dim">
        <Markdown text={text} className="text-sm" />
      </Collapsible>
    );
  if (type === "refinement_outcome")
    return (
      <div className="my-1 ml-[1.15rem] rounded border-l-2 border-primary/60 bg-panel px-2 py-1 text-[13px]">
        <div className="mb-1 text-[11px] uppercase tracking-wide text-primary-soft">Refinement</div>
        <Markdown text={text} />
      </div>
    );
  return (
    <div className="my-1 ml-[1.15rem] rounded border-l-2 border-grid/60 bg-panel px-2 py-1 text-[13px]">
      <div className="mb-1 text-[11px] font-semibold text-muted">[{type}]</div>
      <Markdown text={text} />
    </div>
  );
}

function SummaryRow({ title, m }: { title: string; m: Msg }) {
  return (
    <div className="my-2 border-y border-primary/30 bg-panel/60 px-2 py-0.5">
      <Collapsible title={<span className="text-primary-soft">◆ {title}{m.tokensBefore ? ` · ${m.tokensBefore.toLocaleString()} tokens before` : ""}</span>} open={false}>
        {m.customInstructions ? <div className="mb-1 text-xs text-dim">focus: {m.customInstructions}</div> : null}
        <Markdown text={String(m.summary ?? "")} className="text-sm" />
      </Collapsible>
    </div>
  );
}

const Row = memo(function Row({ m, results, data, detail }: { m: Msg; results: Map<string, Msg>; data: SessionData; detail: DetailLevel }) {
  switch (m.role) {
    case "user":
      return <UserRow message={m} />;
    case "assistant":
      return <div className="group"><AssistantBody message={m} results={results} data={data} detail={detail} streaming={false} /></div>;
    case "toolResult":
      return null;
    case "bashExecution":
      return <BashRow m={m} />;
    case "custom":
      return <CustomRow m={m} detail={detail} />;
    case "compactionSummary":
      return <SummaryRow title="Context compacted" m={m} />;
    case "branchSummary":
      return <SummaryRow title="Branch summary" m={m} />;
    case "_status":
      return <StatusRow kind={m.kind}>{m.text}</StatusRow>;
    default:
      return null;
  }
});

function Working({ data }: { data: SessionData }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);
  const { retry, compaction, userBash } = data;
  return (
    <div className="space-y-1 pl-[1.15rem]">
      {userBash?.running ? (
        <div className="overflow-hidden rounded border-l-2 border-primary-soft bg-tool-pending">
          <div className="flex items-center gap-2 px-3 py-2 text-[13px]"><span className="bh-pulse text-primary-soft">$</span><code className="flex-1 truncate font-mono">{userBash.command}</code></div>
          {userBash.output ? <pre className="bh-pre max-h-72 overflow-auto whitespace-pre-wrap border-t border-grid/30 px-3 py-2 text-[12px] text-muted">{userBash.output}</pre> : null}
        </div>
      ) : null}
      {compaction?.active ? (
        <div className="rounded border-l-2 border-primary-soft bg-panel px-2 py-1 text-[13px]">
          <div className="bh-pulse text-primary-soft">◆ Compacting context… <span className="text-dim">({compaction.reason})</span></div>
          {compaction.summary ? <div className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap text-xs text-muted">{compaction.summary}</div> : null}
        </div>
      ) : null}
      {retry ? (
        <div role="status" className="flex flex-wrap items-center gap-2 rounded border-l-2 border-warning bg-panel px-2 py-1 text-[13px] text-warning">
          <span className="bh-pulse">↻</span>
          <span className="min-w-0 flex-1">
            Retry {retry.attempt}/{retry.maxAttempts}
            {retry.backupModel ? ` on ${retry.backupModel}` : ""} in {formatDuration(Math.max(0, retry.delayMs - (Date.now() - retry.startedAt)))} — {retry.errorMessage}
          </span>
          <button className="bh-btn" onClick={() => void abortRetry()}>Cancel retry</button>
        </div>
      ) : null}
      {data.state.isStreaming && !compaction?.active ? (
        <div className="flex items-center gap-2 font-mono text-[11px] text-dim" aria-live="polite">
          <span className="bh-spin" /> Working{data.turnStartedAt ? ` · ${formatDuration(Date.now() - data.turnStartedAt)}` : ""}
        </div>
      ) : null}
    </div>
  );
}

export function Transcript() {
  const data = useChat((s) => s.data);
  const detail = useChat((s) => s.detail);
  const loadingOlder = useChat((s) => s.loadingOlder);
  const ref = useRef<HTMLDivElement>(null);
  const [stick, setStick] = useState(true);
  const stickRef = useRef(true);
  const results = resultsById(data.messages);

  const toBottom = (smooth = false) => {
    const el = ref.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  };

  useLayoutEffect(() => {
    if (stickRef.current) toBottom();
  });

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const inner = el.firstElementChild;
    if (!inner) return;
    const observer = new ResizeObserver(() => stickRef.current && toBottom());
    observer.observe(inner);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const onJump = () => { stickRef.current = true; setStick(true); toBottom(true); };
    window.addEventListener("bh:chat-follow", onJump);
    return () => window.removeEventListener("bh:chat-follow", onJump);
  }, []);

  const onScroll = () => {
    const el = ref.current!;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (near !== stickRef.current) {
      stickRef.current = near;
      setStick(near);
    }
    if (el.scrollTop < 200 && data.historyBeforeEntryId && !loadingOlder) {
      const before = el.scrollHeight;
      void loadOlder().then(() => requestAnimationFrame(() => { el.scrollTop += el.scrollHeight - before; }));
    }
  };

  const empty = !data.messages.length && !data.live;
  return (
    <div className="relative min-h-0 flex-1">
      <div ref={ref} onScroll={onScroll} className="h-full overflow-y-auto overscroll-contain" role="log" aria-label="Conversation">
        <div className="w-full px-3 py-2">
          {data.historyBeforeEntryId ? (
            <button className="mx-auto mb-3 block text-xs text-dim hover:text-fg" onClick={() => void loadOlder()} disabled={loadingOlder}>
              {loadingOlder ? "Loading earlier messages…" : "Load earlier messages"}
            </button>
          ) : null}
          {empty ? <div className="py-16 text-center text-dim">Send a message to start.</div> : null}
          {data.messages.map((m) => (
            <Row key={keyOf(m)} m={m} results={results} data={data} detail={detail} />
          ))}
          {data.live ? <AssistantBody message={data.live} results={results} data={data} detail={detail} streaming /> : null}
          <Working data={data} />
        </div>
      </div>
      {!stick ? (
        <button
          className="bh-btn bh-btn-primary absolute bottom-3 left-1/2 -translate-x-1/2 shadow-lg"
          onClick={() => { stickRef.current = true; setStick(true); toBottom(true); }}
        >
          ↓ Jump to latest
        </button>
      ) : null}
    </div>
  );
}
