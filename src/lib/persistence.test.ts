import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import {
  debouncedLocalStorageWrite,
  flushPendingWrites,
} from "@/lib/persistence"

// Minimal in-memory localStorage stub (node test env has none). Stores the
// raw string the way the real one does, so JSON.stringify in the writer and
// JSON.parse here round-trip identically.
function makeStorage() {
  const map = new Map<string, string>()
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => {
      map.set(k, v)
    },
    removeItem: (k: string) => {
      map.delete(k)
    },
    clear: () => map.clear(),
  }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal("localStorage", makeStorage())
})

afterEach(() => {
  // Drain anything still pending so it can't leak into the next test, then
  // restore real timers + drop the stub.
  flushPendingWrites()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe("debouncedLocalStorageWrite", () => {
  it("writes the value after the debounce window elapses", () => {
    debouncedLocalStorageWrite("k", { a: 1 })
    expect(localStorage.getItem("k")).toBeNull()
    vi.advanceTimersByTime(299)
    expect(localStorage.getItem("k")).toBeNull()
    vi.advanceTimersByTime(1)
    expect(localStorage.getItem("k")).toBe('{"a":1}')
  })

  it("coalesces a burst into one write of the LAST value", () => {
    for (let i = 0; i < 50; i++) debouncedLocalStorageWrite("k", i)
    expect(localStorage.getItem("k")).toBeNull()
    vi.advanceTimersByTime(300)
    // Only the final value lands — the 49 earlier setItems never hit disk.
    expect(localStorage.getItem("k")).toBe("49")
  })

  it("resets the timer on each call within the window", () => {
    debouncedLocalStorageWrite("k", 1)
    vi.advanceTimersByTime(250) // 250ms in, not yet flushed
    debouncedLocalStorageWrite("k", 2) // resets the 300ms clock
    vi.advanceTimersByTime(299) // 549ms total — still not flushed (reset)
    expect(localStorage.getItem("k")).toBeNull()
    vi.advanceTimersByTime(1) // 550ms since the last call → flush
    expect(localStorage.getItem("k")).toBe("2")
  })

  it("keeps independent keys on independent timers", () => {
    debouncedLocalStorageWrite("a", 1)
    vi.advanceTimersByTime(150)
    debouncedLocalStorageWrite("b", 2)
    vi.advanceTimersByTime(150) // a is at 300ms → flushes; b at 150ms → not yet
    expect(localStorage.getItem("a")).toBe("1")
    expect(localStorage.getItem("b")).toBeNull()
    vi.advanceTimersByTime(150)
    expect(localStorage.getItem("b")).toBe("2")
  })

  it("is a no-op for an undefined value (nothing meaningful to persist)", () => {
    debouncedLocalStorageWrite("k", undefined)
    vi.advanceTimersByTime(300)
    expect(localStorage.getItem("k")).toBeNull()
  })

  it("survives a failing localStorage (best-effort, never throws)", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota")
      },
      removeItem: () => {},
      clear: () => {},
    })
    expect(() => {
      debouncedLocalStorageWrite("k", 1)
      vi.advanceTimersByTime(300)
    }).not.toThrow()
  })
})

describe("flushPendingWrites", () => {
  it("commits a pending value immediately and clears the timer", () => {
    debouncedLocalStorageWrite("k", "pending")
    expect(localStorage.getItem("k")).toBeNull()
    flushPendingWrites()
    expect(localStorage.getItem("k")).toBe('"pending"')
    // No double-write later when the original timer would have fired.
    vi.advanceTimersByTime(300)
    expect(localStorage.getItem("k")).toBe('"pending"')
  })

  it("is idempotent — calling with nothing pending is a no-op", () => {
    expect(() => flushPendingWrites()).not.toThrow()
    expect(localStorage.getItem("k")).toBeNull()
  })

  it("flushes every key with a pending write", () => {
    debouncedLocalStorageWrite("a", 1)
    debouncedLocalStorageWrite("b", 2)
    debouncedLocalStorageWrite("c", 3)
    flushPendingWrites()
    expect(localStorage.getItem("a")).toBe("1")
    expect(localStorage.getItem("b")).toBe("2")
    expect(localStorage.getItem("c")).toBe("3")
  })

  it("preserves the latest value when a burst is flushed mid-flight", () => {
    // Simulates the drag/close race: many writes, then a flush before the
    // timer fires — the last geometry must land, nothing lost.
    for (let i = 0; i < 100; i++) debouncedLocalStorageWrite("geom", i)
    flushPendingWrites()
    expect(localStorage.getItem("geom")).toBe("99")
  })
})
