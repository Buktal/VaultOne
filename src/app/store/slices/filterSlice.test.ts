import { describe, expect, it } from "vitest"

import {
  EMPTY_FILTER,
  type FilterState,
  toFilter,
} from "@/app/store/slices/filterSlice"

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
})
