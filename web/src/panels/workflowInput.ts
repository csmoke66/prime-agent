// Turns the `/run` form fields into the JSON input object (rules of crates/bh-tui/src/workflow_form.rs).
export type FieldDef = { type: "string" | "number" | "integer" | "boolean" | "array" | "object" | "null"; required?: boolean; description?: string; choices?: unknown[] };

export function parseField(name: string, def: FieldDef, text: string): { value?: unknown; error?: string } {
  const trimmed = text.trim();
  if (trimmed === "" && def.type !== "string") return def.required ? { error: `${name}: required` } : {};
  try {
    switch (def.type) {
      case "string":
        if (text.trimStart().startsWith('"')) return { value: JSON.parse(text) as string };
        return text === "" && !def.required ? {} : { value: text };
      case "boolean":
        if (trimmed === "true" || trimmed === "false") return { value: trimmed === "true" };
        return { error: `${name}: expected true or false` };
      case "integer": {
        const n = JSON.parse(trimmed);
        return Number.isInteger(n) ? { value: n } : { error: `${name}: expected an integer` };
      }
      case "number": {
        const n = JSON.parse(trimmed);
        return typeof n === "number" ? { value: n } : { error: `${name}: expected a number` };
      }
      case "array": {
        const v = JSON.parse(trimmed);
        return Array.isArray(v) ? { value: v } : { error: `${name}: expected a JSON array` };
      }
      case "object": {
        const v = JSON.parse(trimmed);
        return v && typeof v === "object" && !Array.isArray(v) ? { value: v } : { error: `${name}: expected a JSON object` };
      }
      case "null":
        return { value: null };
    }
  } catch (error) {
    return { error: `${name}: ${(error as Error).message}` };
  }
}

export function buildInput(fields: Record<string, FieldDef>, texts: Record<string, string>): { input?: Record<string, unknown>; errors: string[] } {
  const input: Record<string, unknown> = {};
  const errors: string[] = [];
  for (const [name, def] of Object.entries(fields)) {
    const result = parseField(name, def, texts[name] ?? "");
    if (result.error) errors.push(result.error);
    else if ("value" in result) input[name] = result.value;
  }
  return errors.length ? { errors } : { input, errors };
}
