/** Pure formatting and matching helpers shared by the session picker, agents view and palette. */

export function relativeTime(value: string | number | null | undefined, now = Date.now()): string {
  if (value === null || value === undefined || value === "") return "";
  const ms = typeof value === "number" ? value : Date.parse(value);
  if (Number.isNaN(ms)) return "";
  const seconds = Math.max(0, Math.round((now - ms) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.round(months / 12)}y ago`;
}

export function fmtDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "";
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function fmtCost(value: number | null | undefined): string {
  if (!value || !Number.isFinite(value)) return "";
  return value < 0.01 ? "<$0.01" : `$${value.toFixed(2)}`;
}

export function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** `/home/u/project/sub` -> `~/…/project/sub` style short form that keeps the tail. */
export function shortPath(path: string | null | undefined, keep = 2): string {
  if (!path) return "";
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) return path;
  return `…/${parts.slice(-keep).join("/")}`;
}

export function oneLine(text: string | null | undefined, max = 140): string {
  const flat = (text ?? "").replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

/**
 * Subsequence fuzzy match. Returns null when `query` is not a subsequence of `text`; higher is
 * better (consecutive runs and word-start hits score more, shorter text breaks ties).
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().trim();
  if (!q) return 0;
  const t = text.toLowerCase();
  let score = 0;
  let ti = 0;
  let run = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    const boundary = found === 0 || /[\s/_\-.:]/.test(t[found - 1]);
    run = found === ti ? run + 1 : 0;
    score += 1 + run * 2 + (boundary ? 4 : 0);
    ti = found + 1;
  }
  return score - t.length * 0.01;
}

export function fuzzyFilter<T>(items: T[], query: string, text: (item: T) => string): T[] {
  if (!query.trim()) return items;
  return items
    .map((item) => ({ item, score: fuzzyScore(query, text(item)) }))
    .filter((entry): entry is { item: T; score: number } => entry.score !== null)
    .sort((a, b) => b.score - a.score)
    .map((entry) => entry.item);
}
