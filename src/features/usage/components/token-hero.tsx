// Token hero — token-first Tier 1 anchor (ADR-0011). 总消耗 headline + delta
// (vs 窗首) + daily avg + 四桶堆叠 composition bar + legend (label/value 行) +
// 缓存命中率 footer.
//
// 右栏窄布局 (ADR-0013): 纵向，无 sparkline — 中栏已有大趋势图，此处只留当前
// 窗口的数值快照。颜色全部走 CSS 变量，换主题不改本件。

import { useStatsQuery, useTrendQuery, ZERO_STATS } from "@/app/store/api"
import { Card, CardContent } from "@/components/ui/card"
import { usePollingInterval } from "@/hooks/use-polling-interval"
import { formatInt, formatPct, formatTokens } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

const SEGMENTS = [
  { key: "input_tokens", label: "输入", color: "var(--chart-input)" },
  { key: "output_tokens", label: "输出", color: "var(--chart-output)" },
  {
    key: "cache_creation_tokens",
    label: "缓存创建",
    color: "var(--chart-cache-create)",
  },
  {
    key: "cache_read_tokens",
    label: "缓存命中",
    color: "var(--chart-cache-read)",
  },
] as const

export function TokenHero({ filter }: { filter: UsageFilter }) {
  const pollingInterval = usePollingInterval()
  const { data: stats } = useStatsQuery(filter, { pollingInterval })
  const { data: trend = [] } = useTrendQuery(filter, { pollingInterval })
  const s = stats ?? ZERO_STATS
  const total = s.total_tokens || 1

  // delta = 末日 vs 窗首 (trend 已按日升序); 日均 = 总量 / 窗口天数.
  const points = trend.map((p) => ({ v: Number(p.total_tokens ?? 0) }))
  let deltaPct: number | null = null
  if (points.length >= 2) {
    const first = points[0].v
    const last = points[points.length - 1].v
    if (first > 0) deltaPct = (last - first) / first
  }
  const days = trend.length
  const dailyAvg = days > 0 ? s.total_tokens / days : 0

  return (
    <Card>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-muted-foreground text-xs">总消耗</span>
          <span className="text-4xl font-semibold leading-none tabular-nums">
            {formatTokens(s.total_tokens)}
          </span>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            {deltaPct !== null ? (
              <span
                className={`inline-flex items-center gap-1 text-xs font-medium tabular-nums ${
                  deltaPct >= 0 ? "text-primary" : "text-destructive"
                }`}
              >
                {deltaPct >= 0 ? "↑" : "↓"}{" "}
                {Math.abs(deltaPct * 100).toFixed(1)}%
                <span className="text-muted-foreground font-normal">
                  较起始
                </span>
              </span>
            ) : null}
            <span className="text-muted-foreground text-xs tabular-nums">
              日均 {formatTokens(dailyAvg)}
            </span>
          </div>
        </div>

        <div className="bg-muted flex h-2 w-full overflow-hidden rounded-full">
          {SEGMENTS.map((seg) => {
            const v = Number(s[seg.key] ?? 0)
            const pct = (v / total) * 100
            return (
              <div
                key={seg.key}
                className="h-full"
                style={{ width: `${pct}%`, backgroundColor: seg.color }}
              />
            )
          })}
        </div>

        <div className="flex flex-col gap-2">
          {SEGMENTS.map((seg) => {
            const v = Number(s[seg.key] ?? 0)
            const pct = (v / total) * 100
            return (
              <div
                key={seg.key}
                className="flex items-center justify-between text-xs"
              >
                <span className="flex items-center gap-1.5">
                  <span
                    className="inline-block size-2 rounded-sm"
                    style={{ backgroundColor: seg.color }}
                  />
                  <span className="text-muted-foreground">{seg.label}</span>
                </span>
                <span className="tabular-nums">
                  {formatTokens(v)} · {pct.toFixed(0)}%
                </span>
              </div>
            )
          })}
        </div>

        <div className="text-muted-foreground flex items-center justify-between border-border/60 border-t pt-2.5 text-xs">
          <span className="tabular-nums">
            {formatInt(s.request_count)} 次请求
          </span>
          <span className="tabular-nums">
            缓存命中率 {formatPct(s.cache_hit_rate)}
          </span>
        </div>
      </CardContent>
    </Card>
  )
}
