// Token breakdown — a single stacked bar showing how the four token buckets
// (input / output / cache creation / cache read) compose the total, plus the
// legend with absolute + share, and a footer line for request count and cache
// hit rate. Replaces the flat 8-card stat grid with one information-dense card.

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { useQueryUsageStatsQuery } from "@/features/usage/api"
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
}

const SEGMENTS = [
  { key: "input_tokens", label: "输入", color: "var(--chart-1)" },
  { key: "output_tokens", label: "输出", color: "var(--chart-2)" },
  { key: "cache_read_tokens", label: "缓存命中", color: "var(--chart-3)" },
  { key: "cache_creation_tokens", label: "缓存创建", color: "var(--chart-4)" },
] as const

export function TokenBreakdownBar({ filter }: { filter: UsageFilter }) {
  const { data: stats } = useQueryUsageStatsQuery(filter, {
    pollingInterval: 30_000,
  })
  const s = stats ?? ZERO
  const total = s.total_tokens || 1

  return (
    <Card>
      <CardHeader>
        <CardTitle>Token 构成</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex h-2.5 w-full overflow-hidden rounded-full">
          {SEGMENTS.map((seg) => {
            const v = s[seg.key]
            const pct = (v / total) * 100
            return (
              <div
                key={seg.key}
                style={{ width: `${pct}%`, backgroundColor: seg.color }}
                title={`${seg.label} · ${formatTokens(v)}`}
              />
            )
          })}
        </div>

        <div className="grid grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-4">
          {SEGMENTS.map((seg) => {
            const v = s[seg.key]
            const pct = s.total_tokens ? (v / s.total_tokens) * 100 : 0
            return (
              <div key={seg.key} className="flex flex-col gap-0.5">
                <span className="text-muted-foreground flex items-center gap-1.5 text-xs">
                  <span
                    className="size-2 rounded-full"
                    style={{ backgroundColor: seg.color }}
                  />
                  {seg.label}
                </span>
                <span className="text-sm font-medium tabular-nums">
                  {formatTokens(v)}
                </span>
                <span className="text-muted-foreground text-[10px] tabular-nums">
                  {pct.toFixed(1)}%
                </span>
              </div>
            )
          })}
        </div>

        <div className="text-muted-foreground flex items-center gap-5 border-t pt-2 text-xs">
          <span>
            请求{" "}
            <span className="text-foreground font-medium tabular-nums">
              {formatInt(s.request_count)}
            </span>
          </span>
          <span>
            缓存命中率{" "}
            <span className="text-foreground font-medium tabular-nums">
              {formatPct(s.cache_hit_rate)}
            </span>
          </span>
        </div>
      </CardContent>
    </Card>
  )
}
