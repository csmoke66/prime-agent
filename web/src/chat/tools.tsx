import { useState, type ReactNode } from "react";
import { Code } from "./markdown";
import { textOfContent, type Block, type ToolExec } from "./model";
import type { DetailLevel } from "./store";

export interface ResultView {
  content: Block[];
  details: any;
  isError: boolean;
}

export function resultView(source: any, isError?: boolean): ResultView | null {
  if (!source) return null;
  const content = Array.isArray(source.content) ? source.content : typeof source.content === "string" ? [{ type: "text", text: source.content }] : [];
  return { content, details: source.details, isError: isError ?? !!source.isError };
}

const resultText = (r: ResultView | null) => (r ? textOfContent(r.content) : "");

export function Images({ blocks }: { blocks: Block[] }) {
  const images = blocks.filter((b) => b.type === "image");
  if (!images.length) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {images.map((image, i) =>
        image.data ? (
          <a key={i} href={`data:${image.mimeType};base64,${image.data}`} target="_blank" rel="noreferrer">
            <img src={`data:${image.mimeType};base64,${image.data}`} alt="attachment" className="max-h-72 max-w-full rounded-lg border border-line object-contain" />
          </a>
        ) : (
          <div key={i} className="rounded-lg border border-dashed border-line px-3 py-2 text-xs text-dim">
            image{image.widthPx ? ` ${image.widthPx}×${image.heightPx}` : ""}
            {image.elidedBytes ? ` · ${Math.round(image.elidedBytes / 1365)} KB not loaded` : ""}
          </div>
        ),
      )}
    </div>
  );
}

/** Unified diff rows as produced by the edit tool: `+`/`-`/space prefixed lines. */
export function DiffView({ diff }: { diff: string }) {
  const lines = diff.split("\n");
  return (
    <pre className="bh-pre overflow-x-auto !p-0 text-[12px] leading-[1.45]">
      {lines.map((line, i) => {
        const c = line[0];
        const cls = c === "+" ? "bg-[#0f2415] text-[#8fcf9b]" : c === "-" ? "bg-[#2a1214] text-[#e59aa0]" : "text-muted";
        return (
          <div key={i} className={`px-2 ${cls}`}>
            {line || " "}
          </div>
        );
      })}
    </pre>
  );
}

export const diffStats = (diff: string) => {
  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+")) added++;
    else if (line.startsWith("-")) removed++;
  }
  return { added, removed };
};

const STATE_STYLE = {
  running: { bar: "border-primary/40", bg: "bg-tool-pending", label: "running", tone: "text-primary" },
  done: { bar: "border-line", bg: "bg-tool-success", label: "ok", tone: "text-success" },
  error: { bar: "border-error/40", bg: "bg-tool-error", label: "error", tone: "text-error" },
} as const;

export const fmtMs = (ms: unknown): string | null => (typeof ms === "number" ? (ms < 1000 ? `${Math.round(ms)}ms` : `${(ms / 1000).toFixed(1)}s`) : null);

/** One timeline row per tool call: state bar, tool name, command preview, outcome chips, expandable body. */
function Frame({
  name,
  title,
  state,
  open,
  onToggle,
  children,
  meta,
}: {
  name: string;
  title: ReactNode;
  state: "running" | "done" | "error";
  open: boolean;
  onToggle: () => void;
  children?: ReactNode;
  meta?: ReactNode;
}) {
  const st = STATE_STYLE[state];
  return (
    <div className={`my-1.5 overflow-hidden rounded-md border ${st.bar} ${st.bg}`}>
      <button type="button" onClick={onToggle} aria-expanded={open} className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left font-mono text-[12px] hover:bg-white/[0.03]">
        {state === "running" ? <span className="bh-spin shrink-0" aria-label="running" /> : <span className={`w-3 shrink-0 text-center ${st.tone}`} aria-hidden>{state === "error" ? "✕" : "✓"}</span>}
        <span className="shrink-0 font-semibold text-fg">{name}</span>
        <span className="min-w-0 flex-1 truncate text-fg/90">{title}</span>
        <span className="flex shrink-0 items-center gap-2 text-[11px] text-dim">
          {meta}
          <span className={st.tone}>{st.label}</span>
          <span className={`transition-transform ${open ? "rotate-90" : ""}`} aria-hidden>›</span>
        </span>
      </button>
      {open ? <div className="border-t border-line px-2.5 py-2">{children}</div> : null}
    </div>
  );
}

const OutputBlock = ({ text, error }: { text: string; error?: boolean }) =>
  text.trim() ? <pre className={`bh-pre max-h-96 overflow-auto whitespace-pre-wrap break-words text-[12px] leading-5 ${error ? "text-error" : "text-muted"}`}>{text}</pre> : null;

/** Open state: a per-card override wins, otherwise the conversation detail level decides. */
function useOpen(initial: boolean, detail: DetailLevel, levelOpens: DetailLevel): [boolean, () => void] {
  const [override, setOverride] = useState<boolean | null>(null);
  const open = override ?? (detail >= levelOpens || initial);
  return [open, () => setOverride(!open)];
}

export function ToolCard({ call, exec, result, detail }: { call: { id: string; name: string; arguments?: any }; exec?: ToolExec; result?: ResultView | null; detail: DetailLevel }) {
  const args = call.arguments ?? exec?.args ?? {};
  const running = !result && (exec?.status ?? "running") === "running";
  const view = result ?? resultView(exec?.result, exec?.status === "error");
  const partial = running ? resultView(exec?.partial) : null;
  const state: "running" | "done" | "error" = running ? "running" : view?.isError ? "error" : "done";
  const name = call.name;
  if (name === "bash") return <BashCard args={args} view={view ?? partial} state={state} detail={detail} />;
  if (name === "python_repl") return <PythonCard args={args} view={view ?? partial} state={state} detail={detail} />;
  if (name === "edit") return <EditCard args={args} view={view} state={state} detail={detail} />;
  return <GenericCard name={name} args={args} view={view ?? partial} state={state} detail={detail} />;
}

function BashCard({ args, view, state, detail }: { args: any; view: ResultView | null; state: "running" | "done" | "error"; detail: DetailLevel }) {
  const text = resultText(view);
  const [open, toggle] = useOpen(state === "running" && text.trim() !== "", detail, 2);
  return (
    <Frame
      name="bash"
      title={<><span className="text-success">$ </span>{String(args.command ?? "").split("\n")[0]}</>}
      meta={<>{typeof view?.details?.exitCode === "number" ? <span className={view.details.exitCode ? "text-error" : ""}>exit {view.details.exitCode}</span> : null}<span>{fmtMs(view?.details?.durationMs)}</span></>}
      state={state}
      open={open}
      onToggle={toggle}
    >
      {String(args.command ?? "").includes("\n") || String(args.command ?? "").length > 100 ? <Code code={String(args.command ?? "")} lang="bash" /> : null}
      <OutputBlock text={text} error={state === "error"} />
      <Images blocks={view?.content ?? []} />
    </Frame>
  );
}

function PythonCard({ args, view, state, detail }: { args: any; view: ResultView | null; state: "running" | "done" | "error"; detail: DetailLevel }) {
  const details = view?.details ?? {};
  const diffs: { path: string; diff: string }[] = Array.isArray(details.diffs) ? details.diffs.filter((d: any) => d?.path && d?.diff) : [];
  const [open, toggle] = useOpen(state === "running", detail, 2);
  const code = String(args.code ?? args.source ?? "");
  const first = code.split("\n").find((l) => l.trim()) ?? "python";
  const text = resultText(view);
  const totals = diffs.reduce((acc, d) => {
    const s = diffStats(d.diff);
    return { added: acc.added + s.added, removed: acc.removed + s.removed };
  }, { added: 0, removed: 0 });
  const duration = fmtMs(details.durationMs);
  const showDiffs = detail >= 1;
  return (
    <>
      <Frame
        name="python"
        title={first}
        meta={<>{code.split("\n").length > 1 ? <span>{code.split("\n").length} lines</span> : null}<span>{duration}</span></>}
        state={state}
        open={open}
        onToggle={toggle}
      >
        <Code code={code} lang="python" />
        {details.error ? (
          <pre className="bh-pre mt-2 max-h-96 overflow-auto whitespace-pre-wrap text-[12px] text-error">
            {details.error.ename}: {details.error.evalue}
            {Array.isArray(details.error.traceback) ? "\n" + details.error.traceback.join("\n") : ""}
          </pre>
        ) : (
          <OutputBlock text={text} error={state === "error"} />
        )}
        <Images blocks={view?.content ?? []} />
      </Frame>
      {diffs.length ? <DiffSummary diffs={diffs} totals={totals} expanded={showDiffs} /> : null}
    </>
  );
}

function DiffSummary({ diffs, totals, expanded }: { diffs: { path: string; diff: string }[]; totals: { added: number; removed: number }; expanded: boolean }) {
  const [override, setOverride] = useState<boolean | null>(null);
  const open = override ?? expanded;
  const byPath = new Map<string, string[]>();
  for (const d of diffs) byPath.set(d.path, [...(byPath.get(d.path) ?? []), d.diff]);
  return (
    <div className="mb-0.5 ml-3 border-l-2 border-primary/40 pl-2">
      <button type="button" className="text-xs text-muted hover:text-fg" onClick={() => setOverride(!open)} aria-expanded={open}>
        {open ? "▾" : "▸"} {byPath.size} file{byPath.size === 1 ? "" : "s"} edited <span className="text-success">+{totals.added}</span> <span className="text-error">−{totals.removed}</span>
      </button>
      {open
        ? [...byPath].map(([path, edits]) => (
            <div key={path} className="mt-1 overflow-hidden rounded border border-line">
              <div className="bg-panel px-2 py-0.5 font-mono text-xs text-muted">{path}</div>
              {edits.map((d, i) => (
                <DiffView key={i} diff={d} />
              ))}
            </div>
          ))
        : null}
    </div>
  );
}

function EditCard({ args, view, state, detail }: { args: any; view: ResultView | null; state: "running" | "done" | "error"; detail: DetailLevel }) {
  const [open, toggle] = useOpen(state === "error", detail, 1);
  const path = String(args.path ?? args.file_path ?? args.file ?? "");
  const diff: string = typeof view?.details?.diff === "string" ? view.details.diff : "";
  const stats = diff ? diffStats(diff) : null;
  return (
    <Frame
      name="edit"
      title={path}
      meta={stats ? <span><span className="text-success">+{stats.added}</span> <span className="text-error">−{stats.removed}</span></span> : null}
      state={state}
      open={open}
      onToggle={toggle}
    >
      {diff ? <DiffView diff={diff} /> : <OutputBlock text={resultText(view) || JSON.stringify(args, null, 2)} error={state === "error"} />}
    </Frame>
  );
}

function GenericCard({ name, args, view, state, detail }: { name: string; args: any; view: ResultView | null; state: "running" | "done" | "error"; detail: DetailLevel }) {
  const [open, toggle] = useOpen(false, detail, 2);
  const argText = Object.keys(args ?? {}).length ? JSON.stringify(args, null, 2) : "";
  const brief = typeof args === "object" && args ? Object.values(args).find((v) => typeof v === "string") : undefined;
  return (
    <Frame name={name} title={typeof brief === "string" ? brief.slice(0, 160) : ""} state={state} open={open} onToggle={toggle}>
      {argText ? <Code code={argText} lang="json" /> : null}
      <OutputBlock text={resultText(view)} error={state === "error"} />
      <Images blocks={view?.content ?? []} />
    </Frame>
  );
}
