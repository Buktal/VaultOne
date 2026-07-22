// KPI 概览 — token-first Tier 2 (ADR-0011). 右栏列表式卡片：平均时长 /
// 请求·轮 / 总请求数 / 总成本。Token 总量在 TokenHero 锚点；缓存命中率在
// 锚点 footer。成本从旧 CostHero 下调至此 (ADR-0010 superseded)。
//
// 右栏窄布局 (ADR-0013): 单列 label+value 行，替代旧 2×4 卡片网格。

import { useStatsQuery, ZERO_STATS } from "@/app/store/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { formatCost, formatInt } from "@/lib/format"
import { cn } from "@/lib/utils"

import type { UsageFilter } from "@/types/generated/bindings"

/** Milliseconds → `12.3s` / `1m05s`. Em-dash when absent. */
function formatDuration(ms: number | null | undefined): string {
  const v = Number(ms ?? 0)
  if (!Number.isFinite(v) || v <= 0) return "—"
  if (v < 60_000) return `${(v / 1000).toFixed(1)}s`
  const m = Math.floor(v / 60_000)
  const sec = Math.round((v % 60_000) / 1000)
  return `${m}m${sec.toString().padStart(2, "0")}s`
}

export function KpiStrip({ filter }: { filter: UsageFilter }) {
  const { data: stats } = useStatsQuery(filter)
  const s = stats ?? ZERO_STATS

  const perTurn =
    s.turn_count > 0 ? (s.request_count / s.turn_count).toFixed(1) : "—"

  const rows: Array<{ label: string; value: string; accent?: string }> = [
    { label: "平均时长", value: formatDuration(s.avg_turn_duration_ms) },
    { label: "请求 / 轮", value: perTurn },
    { label: "总请求数", value: formatInt(s.request_count) },
    {
      label: "总成本",
      value: formatCost(s.total_cost_usd),
      accent: "var(--metric-cost)",
    },
  ]

  return (
    <Card size="sm" interactive>
      <CardHeader>
        <CardTitle>概览</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col">
        {rows.map((r, i) => (
          <div
            key={r.label}
            className={cn(
              "flex items-baseline justify-between py-2",
              i > 0 && "border-border/60 border-t",
            )}
          >
            <span className="text-muted-foreground text-xs">{r.label}</span>
            <span
              className="text-lg font-semibold tabular-nums"
              style={r.accent ? { color: r.accent } : undefined}
            >
              {r.value}
            </span>
          </div>
        ))}
      </CardContent>
    </Card>
  )
}
