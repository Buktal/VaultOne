// Tests for the sessions browser hook module.
//
// useSessionsBrowser just wires the pure derivations in ./derive (covered
// directly in derive.test.ts) to React state + RTK Query. vitest runs in a pure
// node environment (see vitest.config.ts — no DOM), so renderHook is out of
// scope; what we guard here is that the module imports cleanly in node (it
// pulls the tauri-specta API + RTK Query hooks) — a regression that moved a
// Tauri handle fetch to module top level would otherwise make the hook
// un-importable and zero-tested, the same failure mode that once hid the
// shell-hooks bug (mirrors use-library-browser.test.ts).

import { describe, expect, it } from "vitest"

describe("useSessionsBrowser imports in a non-Tauri (node) environment", () => {
  it("imports without throwing and exports a function", async () => {
    const mod = await import("./use-sessions-browser")
    expect(typeof mod.useSessionsBrowser).toBe("function")
  })
})
