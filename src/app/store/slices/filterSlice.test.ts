import dayjs from "dayjs"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import {
  EMPTY_FILTER,
  FILTER_STORAGE_KEY,
  type FilterState,
  loadPersistedFilter,
  todayFilter,
  toFilter,
} from "@/app/store/slices/filterSlice"
import { dayStr } from "@/lib/date-range"

const base = (over: Partial<FilterState> = {}): FilterState => ({
  ...EMPTY_FILTER,
  ...over,
})

describe("toFilter", () => {
  it("maps an empty FilterState to all-null UsageFilter fields", () => {
    expect(toFilter(base())).toEqual({
      from_ts: null,
      to_ts: null,
      model: null,
      source: null,
      device_scope: null,
    })
  })

  it("passes non-empty model / source / device_scope through", () => {
    const f = toFilter(
      base({
        model: "claude-3-5-sonnet",
        source: "claude_code",
        device_scope: "abc123def456",
      }),
    )
    expect(f.model).toBe("claude-3-5-sonnet")
    expect(f.source).toBe("claude_code")
    expect(f.device_scope).toBe("abc123def456")
  })

  it("converts a local day range to ISO timestamp bounds ordered from <= to", () => {
    // dayjs formats in the local zone, so assert on ordering, not exact instants.
    const f = toFilter(base({ from_day: "2026-07-01", to_day: "2026-07-28" }))
    expect(f.from_ts).not.toBeNull()
    expect(f.to_ts).not.toBeNull()
    expect(new Date(f.from_ts as string).getTime()).toBeLessThanOrEqual(
      new Date(f.to_ts as string).getTime(),
    )
  })

  it("omits the timestamp bound when the day is blank", () => {
    expect(toFilter(base({ to_day: "" })).to_ts).toBeNull()
    expect(toFilter(base({ from_day: "" })).from_ts).toBeNull()
  })

  it("recomputes a 'today' preset at query time, ignoring stale stored dates", () => {
    // Cross-midnight: preset picked yesterday, app still running — the query
    // bounds must roll to the current day (relative assertion, no hardcoded
    // dates — same style as the existing local-day test).
    const f = toFilter(
      base({
        range_preset: "today",
        from_day: "1999-01-01",
        to_day: "1999-01-01",
      }),
    )
    const expectedFrom = dayjs(dayStr()).startOf("day").toISOString()
    const expectedTo = dayjs(dayStr()).endOf("day").toISOString()
    expect(f.from_ts).toBe(expectedFrom)
    expect(f.to_ts).toBe(expectedTo)
  })
})

describe("todayFilter", () => {
  it("scopes to the given day + device, blanking model/source", () => {
    const f = todayFilter("abc123def456", "2026-07-28")
    expect(f.device_scope).toBe("abc123def456")
    expect(f.model).toBeNull()
    expect(f.source).toBeNull()
    // local-day → ISO timestamp bounds (same path as toFilter).
    expect(f.from_ts).not.toBeNull()
    expect(f.to_ts).not.toBeNull()
  })

  it("treats an empty device scope as all-devices (null)", () => {
    expect(todayFilter("", "2026-07-28").device_scope).toBeNull()
  })
})

describe("loadPersistedFilter", () => {
  // Node test env has no localStorage — stub an in-memory one (no jsdom dep).
  const store = new Map<string, string>()
  beforeEach(() => {
    store.clear()
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it("re-expands a 'today' preset to today's bounds, ignoring stale stored dates", () => {
    // The bug: a snapshot of "today" read back as "yesterday" after midnight.
    localStorage.setItem(
      FILTER_STORAGE_KEY,
      JSON.stringify({
        range_preset: "today",
        from_day: "1999-01-01",
        to_day: "1999-01-01",
      }),
    )
    const f = loadPersistedFilter()
    expect(f.range_preset).toBe("today")
    expect(f.from_day).toBe(dayStr())
    expect(f.to_day).toBe(dayStr())
  })

  it("re-expands '7d' to the last 7 days ending today", () => {
    localStorage.setItem(
      FILTER_STORAGE_KEY,
      JSON.stringify({ range_preset: "7d" }),
    )
    const f = loadPersistedFilter()
    expect(f.range_preset).toBe("7d")
    expect(f.from_day).toBe(dayStr(-6))
    expect(f.to_day).toBe(dayStr())
  })

  it("'all' preset yields no day bounds", () => {
    localStorage.setItem(
      FILTER_STORAGE_KEY,
      JSON.stringify({ range_preset: "all" }),
    )
    const f = loadPersistedFilter()
    expect(f.range_preset).toBe("all")
    expect(f.from_day).toBe("")
    expect(f.to_day).toBe("")
  })

  it("treats legacy data (no range_preset) as custom, keeping literal dates", () => {
    localStorage.setItem(
      FILTER_STORAGE_KEY,
      JSON.stringify({
        from_day: "2026-01-15",
        to_day: "2026-01-20",
        model: "gpt-4",
      }),
    )
    const f = loadPersistedFilter()
    expect(f.range_preset).toBe("custom")
    expect(f.from_day).toBe("2026-01-15")
    expect(f.to_day).toBe("2026-01-20")
    expect(f.model).toBe("gpt-4")
  })

  it("falls back to the empty filter on corrupt JSON", () => {
    localStorage.setItem(FILTER_STORAGE_KEY, "{not json")
    expect(loadPersistedFilter()).toEqual(EMPTY_FILTER)
  })
})
