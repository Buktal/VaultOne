// Smoke test for the two most-used shell hooks.
//
// Both useWindowMode and useLightweightTuck previously called Tauri's
// getCurrentWindow() at module top level. In vitest's default node environment
// (see vitest.config.ts — pure-function tests, no DOM) that call throws on
// import, because there is no Tauri runtime to hand back a window handle. The
// throw made these hooks un-importable, which was the root cause of them having
// zero tests. The refactor moves the handle fetch inside each effect's seam
// (mirroring use-tuck-drag.ts); this test mocks the window module and asserts
// both hook modules now import cleanly and expose their expected exports,
// unlocking further testing.

import { describe, expect, it, vi } from "vitest"

// A faithful stub of the Window instance used across both hooks' effects. The
// body of neither hook runs during a bare import, so these methods are never
// exercised here — but providing the full surface keeps the mock reusable for a
// future render-level test (which would need @testing-library/react + jsdom,
// deliberately out of scope for this node-only suite).
vi.mock("@tauri-apps/api/window", () => {
  const win = {
    setAlwaysOnTop: vi.fn().mockResolvedValue(undefined),
    setSkipTaskbar: vi.fn().mockResolvedValue(undefined),
    setResizable: vi.fn().mockResolvedValue(undefined),
    maximize: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    outerPosition: vi.fn().mockResolvedValue({ x: 0, y: 0 }),
    outerSize: vi.fn().mockResolvedValue({ width: 0, height: 0 }),
    onMoved: vi.fn().mockResolvedValue(vi.fn()),
    onResized: vi.fn().mockResolvedValue(vi.fn()),
    startDragging: vi.fn().mockResolvedValue(undefined),
  }
  return {
    getCurrentWindow: () => win,
    // monitorForWindow (in lightweight-geometry) falls back to these; stubbed
    // for the same reusability reason.
    availableMonitors: vi.fn().mockResolvedValue([]),
    currentMonitor: vi.fn().mockResolvedValue(null),
  }
})

describe("shell hooks import in a non-Tauri (node) environment", () => {
  it("useWindowMode imports without throwing and exports a function", async () => {
    const mod = await import("./use-window-mode")
    expect(typeof mod.useWindowMode).toBe("function")
  })

  it("useLightweightTuck imports without throwing and exports a function", async () => {
    const mod = await import("./use-lightweight-tuck")
    expect(typeof mod.useLightweightTuck).toBe("function")
  })
})
