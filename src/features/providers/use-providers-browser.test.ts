// Tests for the providers browser.
//
// The move math (reorderIds) and the form↔settingsConfig mapping (derive.ts)
// are pure functions, covered by their own suites. useProvidersBrowser wires
// those to RTK Query + a mutation, so vitest (pure node, no DOM) guards only
// that the hook module imports cleanly — the same failure mode the library
// browser guards against (a regression that hoists a Tauri handle to module
// top level would make the hook un-importable and zero-tested).

import { describe, expect, it } from "vitest"

describe("useProvidersBrowser imports in a non-Tauri (node) environment", () => {
  it("imports without throwing and exports a function", async () => {
    const mod = await import("./use-providers-browser")
    expect(typeof mod.useProvidersBrowser).toBe("function")
  })
})
