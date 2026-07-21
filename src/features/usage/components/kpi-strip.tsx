// KPI strip — the cost-first secondary metrics row, read from the single
// `useStatsQuery` aggregate (RTK Query dedupes by filter, so this is free).
// Token total / cache-hit rate / average turn duration / request·turn counts.
//
// `avg_turn_duration_ms` + `turn_count` are the new per-turn aggregates
// (TurnDuration entity) surfaced alongside the per-call totals in UsageStats.

import { useStatsQuery } from "@/app/store/api"
import { Card, CardContent } from "@/components/ui/card"
import { formatInt, formatPct, formatTokens } from "@/lib/format"

import type { UsageFilter, UsageStats } from "@/types/generated/bindings"

const ZERO: UsageStats = {
  request_count: 0,
  total_tokens: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_creation_tokens: 0,
  cache_read_tokens: 0,
  cache_hit_rate: 0,
  total_cost_usd: 0,
  turn_count: 0,
  avg_turn_duration_ms: 0,
}

/** Milliseconds → `12.3s` / `1m05s`. Em-dash when absent. */
function formatDuration(ms: number | null | undefined): string {
  const v = Number(ms ?? 0)
  if (!Number.isFinite(v) || v <= 0) return "—"
  if (v < 60_000) return `${(v / 1000).toFixed(1)}s`
  const m = Math.floor(v / 60_000)
  const s = Math.round((v % 60_000) / 1000)
  return `${m}m${s.toString().padStart(2, "0")}s`
}

export function KpiStrip({ filter }: { filter: UsageFilter }) {
  const { data: stats } = useStatsQuery(filter, { pollingInterval: 30_000 })
  const s = stats ?? ZERO

  const cards: Array<{ label: string; value: string }> = [
    { label: "Token 总量", value: formatTokens(s.total_tokens) },
    { label: "缓存命中率", value: formatPct(s.cache_hit_rate) },
    { label: "平均 turn 时长", value: formatDuration(s.avg_turn_duration_ms) },
    {
      label: "请求 / Turn",
      value: `${formatInt(s.request_count)} / ${formatInt(s.turn_count)}`,
    },
  ]

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      {cards.map((c) => (
        <Card key={c.label} size="sm">
          <CardContent className="flex flex-col gap-1">
            <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
              {c.label}
            </span>
            <span className="text-xl font-semibold tabular-nums">
              {c.value}
            </span>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
