import { describe, expect, it } from "vitest";
import { buildInput } from "./workflowInput";

describe("buildInput", () => {
  it("parses typed fields and skips empty optionals", () => {
    const fields = { q: { type: "string", required: true }, n: { type: "integer" }, f: { type: "boolean" }, o: { type: "object" } } as const;
    expect(buildInput(fields, { q: "hello", n: "3", f: "", o: '{"a":1}' })).toEqual({ input: { q: "hello", n: 3, o: { a: 1 } }, errors: [] });
  });
  it("reports errors", () => {
    expect(buildInput({ n: { type: "integer", required: true } }, { n: "x" }).errors).toHaveLength(1);
    expect(buildInput({ n: { type: "integer", required: true } }, {}).errors).toEqual(["n: required"]);
  });
});
