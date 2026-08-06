// Tests for the JSON editor component.
//
// The editor mounts a CodeMirror 6 view, which needs a DOM, so vitest (pure
// node, no DOM) guards only that the module imports cleanly — the same failure
// mode the browser hooks guard against (a regression that hoists a DOM handle
// to module top level would make the component un-importable and zero-tested).

import { describe, expect, it } from "vitest"

describe("JsonEditor imports in a non-DOM (node) environment", () => {
  it("imports without throwing and exports a component", async () => {
    const mod = await import("@/components/json-editor")
    expect(typeof mod.JsonEditor).toBe("function")
  })
})
