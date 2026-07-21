// Cost hero — the cost-first anchor of the dashboard. Cost is VaultOne's first
// metric (ADR-0009), so it gets its own full-width oversized Hero rather than
// sharing a 2-up grid with tokens. Carries the in-window trend sparkline, the
// vs-window-head delta, and a daily-average sub-stat.
//
// Delta semantics: last trend point vs first point *in the current filter
// window* (honest about what's selected). Suppressed when < 2 trend points.

import { Area, AreaChart, ResponsiveContainer } from "recharts"

import { useStatsQuery, useTrendQuery } from "@/app/store/api"
import { Card, CardContent } from "@/components/ui/card"
import { formatCost } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

const COST_COLOR = "var(--metric-cost)"

export function CostHero({ filter }: { filter: UsageFilter }) {
  const { data: stats } = useStatsQuery(filter, { pollingInterval: 30_000 })
  const { data: trend = [] } = useTrendQuery(filter, {
    pollingInterval: 30_000,
  })

  const cost = Number(stats?.total_cost_usd ?? 0)
  const points = trend.map((p) => ({ v: Number(p.total_cost_usd ?? 0) }))

  let deltaPct: number | null = null
  if (points.length >= 2) {
    const first = points[0].v
    const last = points[points.length - 1].v
    if (first > 0) deltaPct = (last - first) / first
  }

  const days = trend.length
  const dailyAvg = days > 0 ? cost / days : 0

  return (
    <Card>
      <CardContent className="flex items-center justify-between gap-6">
        <div className="flex flex-col gap-1.5">
          <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
            累计成本
          </span>
          <span
            className="text-4xl font-semibold tabular-nums"
            style={{ color: COST_COLOR }}
          >
            {formatCost(cost)}
          </span>
          <div className="flex items-center gap-3">
            {deltaPct !== null ? (
              <span
                className={`bg-muted inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium tabular-nums ${
                  deltaPct >= 0 ? "text-emerald-500" : "text-destructive"
                }`}
              >
                {deltaPct >= 0 ? "↑" : "↓"}{" "}
                {Math.abs(deltaPct * 100).toFixed(1)}%
                <span className="text-muted-foreground font-normal">
                  vs 窗首
                </span>
              </span>
            ) : null}
            <span className="text-muted-foreground text-xs tabular-nums">
              日均 {formatCost(dailyAvg)}
            </span>
          </div>
        </div>
        {points.length > 1 ? (
          <div className="h-16 w-48 shrink-0">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart
                data={points}
                margin={{ top: 4, right: 0, bottom: 0, left: 0 }}
              >
                <defs>
                  <linearGradient id="grad-cost" x1="0" y1="0" x2="0" y2="1">
                    <stop
                      offset="0%"
                      stopColor={COST_COLOR}
                      stopOpacity={0.35}
                    />
                    <stop
                      offset="100%"
                      stopColor={COST_COLOR}
                      stopOpacity={0}
                    />
                  </linearGradient>
                </defs>
                <Area
                  type="monotone"
                  dataKey="v"
                  stroke={COST_COLOR}
                  strokeWidth={1.5}
                  fill="url(#grad-cost)"
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
