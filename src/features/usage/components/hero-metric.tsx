// Hero metric — a single oversized KPI with a sparkline and a window-over-window
// delta. Two of these anchor the cockpit: total tokens (left) and total cost
// (right). Reuses the same UsageStats + UsageTrend caches as the rest of the
// dashboard (RTK Query dedupes by filter), so adding it is free of extra requests.
//
// Delta semantics: last point vs first point *in the current filter window*
// (honest about what's selected). Suppressed when fewer than 2 trend points.

import { Area, AreaChart, ResponsiveContainer } from "recharts"

import { Card, CardContent } from "@/components/ui/card"
import {
  useQueryUsageStatsQuery,
  useQueryUsageTrendQuery,
} from "@/features/usage/api"
import { formatCost, formatTokens } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

export function HeroMetric({
  filter,
  metric,
}: {
  filter: UsageFilter
  metric: "tokens" | "cost"
}) {
  const { data: stats } = useQueryUsageStatsQuery(filter, {
    pollingInterval: 30_000,
  })
  const { data: trend = [] } = useQueryUsageTrendQuery(filter, {
    pollingInterval: 30_000,
  })

  const isCost = metric === "cost"
  const value = isCost
    ? (stats?.total_cost_usd ?? 0)
    : (stats?.total_tokens ?? 0)
  const title = isCost ? "累计成本" : "真实消耗 Tokens"
  const fmt = isCost ? formatCost : formatTokens
  const color = isCost ? "var(--chart-5)" : "var(--chart-1)"

  const pick = (p: { total_tokens: number; total_cost_usd: number | null }) =>
    isCost ? Number(p.total_cost_usd ?? 0) : Number(p.total_tokens)

  let deltaPct: number | null = null
  if (trend.length >= 2) {
    const first = pick(trend[0])
    const last = pick(trend[trend.length - 1])
    if (first > 0) deltaPct = (last - first) / first
  }
  const spark = trend.map((p) => ({ v: pick(p) }))

  return (
    <Card>
      <CardContent className="flex items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
            {title}
          </span>
          <span className="text-foreground text-3xl font-semibold tabular-nums">
            {fmt(value)}
          </span>
          {deltaPct !== null ? (
            <span
              className={`text-xs font-medium tabular-nums ${
                deltaPct >= 0 ? "text-emerald-500" : "text-destructive"
              }`}
            >
              {deltaPct >= 0 ? "↑" : "↓"} {Math.abs(deltaPct * 100).toFixed(1)}%
              <span className="text-muted-foreground font-normal">
                {" "}
                vs 窗首
              </span>
            </span>
          ) : null}
        </div>
        {spark.length > 1 ? (
          <div className="h-12 w-28 shrink-0">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart
                data={spark}
                margin={{ top: 4, right: 0, bottom: 0, left: 0 }}
              >
                <defs>
                  <linearGradient
                    id={`grad-${metric}`}
                    x1="0"
                    y1="0"
                    x2="0"
                    y2="1"
                  >
                    <stop offset="0%" stopColor={color} stopOpacity={0.35} />
                    <stop offset="100%" stopColor={color} stopOpacity={0} />
                  </linearGradient>
                </defs>
                <Area
                  type="monotone"
                  dataKey="v"
                  stroke={color}
                  strokeWidth={1.5}
                  fill={`url(#grad-${metric})`}
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        ) : null}
      </CardContent>
    </Card>
  )
}
