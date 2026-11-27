import { describe, expect, it } from "vitest";
import { parseNewArgs } from "./NewSessionDialog";

describe("parseNewArgs", () => {
  it("parses the TUI /new forms", () => {
    expect(parseNewArgs('--name "my work" -- fix the bug')).toEqual({ name: "my work", prompt: "fix the bug" });
    expect(parseNewArgs("--name foo")).toEqual({ name: "foo", prompt: "" });
    expect(parseNewArgs("just a prompt")).toEqual({ name: "", prompt: "just a prompt" });
  });
});
