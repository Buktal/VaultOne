import { describe, expect, it } from "vitest"

import {
  dateInputToDay,
  formatCost,
  formatDay,
  formatDuration,
  formatInt,
  formatPct,
  formatTokens,
} from "@/lib/format"

describe("formatTokens", () => {
  it("treats nullish as 0", () => {
    expect(formatTokens(null)).toBe("0")
    expect(formatTokens(undefined)).toBe("0")
  })

  it("treats non-finite as 0", () => {
    expect(formatTokens(Number.NaN)).toBe("0")
    expect(formatTokens(Number.POSITIVE_INFINITY)).toBe("0")
  })

  it("leaves sub-1K numbers plain (no thousands grouping under 1000)", () => {
    expect(formatTokens(0)).toBe("0")
    expect(formatTokens(856)).toBe("856")
    expect(formatTokens(999)).toBe("999")
  })

  it("compacts to K/M/B and trims trailing zeros", () => {
    expect(formatTokens(1200)).toBe("1.2K")
    expect(formatTokens(3_610_000)).toBe("3.61M")
    expect(formatTokens(1_500_000_000)).toBe("1.5B")
  })
})

describe("formatCost", () => {
  it("nullish / non-finite → $0.0000", () => {
    expect(formatCost(null)).toBe("$0.0000")
    expect(formatCost(undefined)).toBe("$0.0000")
    expect(formatCost(Number.NaN)).toBe("$0.0000")
  })

  it("formats USD with 4 decimals", () => {
    expect(formatCost(1.7564)).toBe("$1.7564")
    expect(formatCost(0)).toBe("$0.0000")
  })
})

describe("formatInt", () => {
  it("truncates and groups thousands", () => {
    expect(formatInt(1234567)).toBe("1,234,567")
    expect(formatInt(12.9)).toBe("12")
    expect(formatInt(null)).toBe("0")
  })
})

describe("formatPct", () => {
  it("maps a [0,1] ratio to a percent string", () => {
    expect(formatPct(0.902)).toBe("90.2%")
    expect(formatPct(0)).toBe("0.0%")
    expect(formatPct(null)).toBe("0.0%")
    expect(formatPct(Number.NaN)).toBe("0%")
  })
})

describe("formatDuration", () => {
  it("em-dash for nullish / non-positive / non-finite", () => {
    expect(formatDuration(null)).toBe("—")
    expect(formatDuration(undefined)).toBe("—")
    expect(formatDuration(0)).toBe("—")
    expect(formatDuration(-5)).toBe("—")
    expect(formatDuration(Number.NaN)).toBe("—")
  })

  it("sub-minute → seconds with one decimal", () => {
    expect(formatDuration(12_300)).toBe("12.3s")
    expect(formatDuration(999)).toBe("1.0s")
  })

  it(">= 1 minute → mSS format, zero-padded seconds", () => {
    expect(formatDuration(65_000)).toBe("1m05s")
    expect(formatDuration(3_602_000)).toBe("60m02s")
  })
})

describe("formatDay", () => {
  it("renders an ISO day as MM/DD", () => {
    expect(formatDay("2026-07-28")).toBe("07/28")
  })

  it("null / invalid → placeholder or raw", () => {
    expect(formatDay(null)).toBe("—")
    expect(formatDay("not-a-day")).toBe("not-a-day")
  })
})

describe("dateInputToDay", () => {
  it("trims a date input to the day, or null when blank", () => {
    expect(dateInputToDay("2026-07-28")).toBe("2026-07-28")
    expect(dateInputToDay("  2026-07-28  ")).toBe("2026-07-28")
    expect(dateInputToDay("")).toBeNull()
    expect(dateInputToDay("   ")).toBeNull()
  })
})
