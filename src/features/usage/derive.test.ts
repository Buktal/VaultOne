import dayjs from "dayjs"
import { describe, expect, it } from "vitest"

import {
  modelMetricValue,
  stopReasonTone,
  tokenSnapshot,
  topNModels,
  zeroFillTrend,
  zeroTrendPoint,
} from "@/features/usage/derive"

import type {
  ModelStatsRow,
  TrendPoint,
  UsageStats,
} from "@/types/generated/bindings"

function trend(day: string, total: number): TrendPoint {
  return {
    day,
    total_tokens: total,
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_tokens: 0,
    cache_read_tokens: 0,
    total_cost_usd: 0,
  }
}

function stats(totalTokens: number): UsageStats {
  return {
    request_count: 0,
    total_tokens: totalTokens,
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_tokens: 0,
    cache_read_tokens: 0,
    cache_hit_rate: 0,
    total_cost_usd: 0,
    turn_count: 0,
    avg_turn_duration_ms: 0,
  }
}

function modelRow(model: string, tokens: number, cost: number): ModelStatsRow {
  return { model, request_count: 1, total_tokens: tokens, total_cost_usd: cost }
}

describe("zeroFillTrend", () => {
  it("returns input unchanged when empty (caller keeps its empty state)", () => {
    expect(zeroFillTrend([], dayjs("2026-07-30T15:30"))).toEqual([])
  })

  it("pads 00:00 → current hour, preserving real records", () => {
    const now = dayjs("2026-07-30T15:30")
    const filled = zeroFillTrend([trend("2026-07-30T15", 999)], now)
    // 00:00 … 15:00 inclusive = 16 buckets.
    expect(filled).toHaveLength(16)
    expect(filled[0].day).toBe("2026-07-30T00")
    expect(filled[0].total_tokens).toBe(0)
    expect(filled[15].day).toBe("2026-07-30T15")
    expect(filled[15].total_tokens).toBe(999)
  })

  it("fills every gap with a zero point of the right shape", () => {
    const now = dayjs("2026-07-30T02:30")
    const filled = zeroFillTrend([trend("2026-07-30T02", 5)], now)
    expect(filled).toHaveLength(3)
    expect(filled[0]).toEqual(zeroTrendPoint("2026-07-30T00"))
  })
})

describe("tokenSnapshot", () => {
  it("delta = last vs first when both present and start > 0", () => {
    const snap = tokenSnapshot(stats(300), [trend("d1", 100), trend("d2", 200)])
    expect(snap.deltaPct).toBe(1)
    expect(snap.dailyAvg).toBe(150)
  })

  it("delta is null with fewer than two points", () => {
    expect(tokenSnapshot(stats(50), [trend("d1", 50)]).deltaPct).toBeNull()
    expect(tokenSnapshot(stats(0), []).deltaPct).toBeNull()
  })

  it("delta is null when the start point is zero (avoid div-by-zero)", () => {
    expect(
      tokenSnapshot(stats(0), [trend("d1", 0), trend("d2", 50)]).deltaPct,
    ).toBeNull()
  })

  it("daily average is 0 over an empty window", () => {
    expect(tokenSnapshot(stats(0), []).dailyAvg).toBe(0)
  })
})

describe("topNModels", () => {
  it("keeps the top-N by metric and aggregates the rest", () => {
    const rows = [
      modelRow("a", 10, 1),
      modelRow("b", 30, 3),
      modelRow("c", 20, 2),
      modelRow("d", 5, 0.5),
    ]
    const res = topNModels(rows, "tokens", 2)
    expect(res.top.map((t) => t.model)).toEqual(["b", "c"])
    expect(res.rest).toEqual({ count: 2, sum: 15 })
    expect(res.total).toBe(65)
  })

  it("switches metric to cost", () => {
    const rows = [modelRow("a", 10, 1), modelRow("b", 30, 3)]
    expect(topNModels(rows, "cost", 1).top[0].model).toBe("b")
  })

  it("no remainder when rows <= topN", () => {
    const res = topNModels([modelRow("a", 1, 1)], "tokens", 5)
    expect(res.rest).toEqual({ count: 0, sum: 0 })
  })

  it("total is >= 1 over empty input so callers can divide safely", () => {
    expect(topNModels([], "tokens", 5).total).toBe(1)
  })
})

describe("modelMetricValue", () => {
  it("treats null cost as 0", () => {
    expect(
      modelMetricValue(
        { model: "x", request_count: 0, total_tokens: 0, total_cost_usd: null },
        "cost",
      ),
    ).toBe(0)
  })
})

describe("stopReasonTone", () => {
  it("maps known reasons to tones", () => {
    expect(stopReasonTone("end_turn")).toBe("success")
    expect(stopReasonTone("tool_use")).toBe("tool")
    expect(stopReasonTone("max_tokens")).toBe("warn")
    expect(stopReasonTone("context_window_exceeded")).toBe("warn")
    expect(stopReasonTone("refusal")).toBe("error")
  })

  it("is case-insensitive", () => {
    expect(stopReasonTone("END_TURN")).toBe("success")
  })

  it("returns null for empty / unknown", () => {
    expect(stopReasonTone("")).toBeNull()
    expect(stopReasonTone("something_new")).toBeNull()
  })
})
