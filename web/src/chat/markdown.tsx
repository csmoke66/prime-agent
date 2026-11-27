import { memo, useMemo } from "react";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";

const escapeHtml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export function highlightCode(code: string, lang?: string): string {
  const language = lang && hljs.getLanguage(lang) ? lang : undefined;
  try {
    return language ? hljs.highlight(code, { language, ignoreIllegals: true }).value : hljs.highlightAuto(code).value;
  } catch {
    return escapeHtml(code);
  }
}

const marked = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    code({ text, lang }) {
      const language = (lang ?? "").split(/\s+/)[0];
      const label = language ? `<span class="bh-code-lang">${escapeHtml(language)}</span>` : "";
      return `<div class="bh-code"><div class="bh-code-bar">${label}<button type="button" class="bh-code-copy" data-copy>Copy</button></div><pre><code class="hljs">${highlightCode(text, language)}</code></pre></div>`;
    },
    link({ href, title, tokens }) {
      const inner = this.parser.parseInline(tokens);
      const t = title ? ` title="${escapeHtml(title)}"` : "";
      return `<a href="${escapeHtml(href)}"${t} target="_blank" rel="noopener noreferrer">${inner}</a>`;
    },
    table({ header, rows }) {
      const cell = (c: any, tag: string) => `<${tag}${c.align ? ` style="text-align:${c.align}"` : ""}>${this.parser.parseInline(c.tokens)}</${tag}>`;
      const head = `<tr>${header.map((c: any) => cell(c, "th")).join("")}</tr>`;
      const body = rows.map((r: any[]) => `<tr>${r.map((c) => cell(c, "td")).join("")}</tr>`).join("");
      return `<div class="bh-table"><table><thead>${head}</thead><tbody>${body}</tbody></table></div>`;
    },
  },
});

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noopener noreferrer");
  }
});

export function renderMarkdown(source: string): string {
  const html = marked.parse(source, { async: false }) as string;
  return DOMPurify.sanitize(html, { ADD_ATTR: ["target", "data-copy"] });
}

function onClick(event: React.MouseEvent<HTMLDivElement>) {
  const target = event.target as HTMLElement;
  if (!target.matches("[data-copy]")) return;
  const code = target.closest(".bh-code")?.querySelector("code")?.textContent ?? "";
  void navigator.clipboard?.writeText(code).then(() => {
    target.textContent = "Copied";
    window.setTimeout(() => (target.textContent = "Copy"), 1200);
  });
}

/** Markdown with highlighted, copyable code blocks. Memoised on the text so streaming only re-parses what changed. */
export const Markdown = memo(function Markdown({ text, className = "" }: { text: string; className?: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);
  return <div className={`bh-md ${className}`} onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />;
});

/** Highlighted preformatted code (tool cards, diffs). */
export function Code({ code, lang, className = "" }: { code: string; lang?: string; className?: string }) {
  const html = useMemo(() => highlightCode(code, lang), [code, lang]);
  return (
    <pre className={`bh-pre ${className}`}>
      <code className="hljs" dangerouslySetInnerHTML={{ __html: html }} />
    </pre>
  );
}
