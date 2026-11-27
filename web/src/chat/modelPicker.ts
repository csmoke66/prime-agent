export type CatalogModel = { provider: string; id: string; name?: string; reasoning?: boolean; contextWindow?: number; thinkingLevelMap?: Record<string, unknown> | null; cost?: { input?: number; output?: number } };

const LADDER = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/** Thinking levels a model supports (mirrors `get_supported_thinking_levels`). */
export function supportedLevels(model: CatalogModel): string[] {
  const map = model.thinkingLevelMap ?? {};
  const wired = Object.values(map).some((v) => v !== null && v !== undefined);
  if (!model.reasoning && !wired) return ["off"];
  return LADDER.filter((level) => {
    const mapped = (map as Record<string, unknown>)[level];
    if (mapped === undefined) return level !== "xhigh" && level !== "max";
    return mapped !== null;
  });
}

/** The level `wanted` clamps to: the nearest higher supported level, else the nearest lower one. */
export function clampLevel(levels: string[], wanted: string): string {
  const rank = LADDER.indexOf(wanted);
  return levels.find((l) => LADDER.indexOf(l) >= rank) ?? [...levels].reverse().find((l) => LADDER.indexOf(l) <= rank) ?? levels[0] ?? "off";
}

/** Picker order: the current model, then recently used ones (newest first), then provider/id order. */
export function orderModels<T extends CatalogModel>(models: T[], current: { provider?: string; id?: string }, recent: string[]): T[] {
  const key = (m: CatalogModel) => `${m.provider}/${m.id}`;
  const rank = (m: T) => (m.provider === current.provider && m.id === current.id ? -2 : recent.includes(key(m)) ? recent.indexOf(key(m)) - 1 : Number.MAX_SAFE_INTEGER);
  return [...models].sort((a, b) => rank(a) - rank(b) || key(a).localeCompare(key(b), undefined, { numeric: true }));
}
