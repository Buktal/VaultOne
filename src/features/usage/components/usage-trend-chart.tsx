// Usage trend chart (BLUEPRINT 使用趋势): dual Y-axis ComposedChart — tokens on
// the left axis, cost on the right. The total-tokens series is an area; the
// remaining line series carry distinct strokeDasharray patterns as a
// color-blind-redundant cue (chart lightness alone is too close across the
// cold palette). Multiple series toggled via the legend.

import {
  Area,
  CartesianGrid,
  ComposedChart,
  Legend,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts"
import { useTrendQuery } from "@/app/store/api"
import { QueryState } from "@/components/query-state"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { formatCost, formatDay, formatTokens } from "@/lib/format"

import type { TrendPoint, UsageFilter } from "@/types/generated/bindings"

type SeriesDef = {
  key: keyof TrendPoint
  name: string
  color: string
  axis: "left" | "right"
  type: "area" | "line"
  /** dash pattern for color-blind-redundant encoding on line series. */
  dash?: string
}

const SERIES: SeriesDef[] = [
  {
    key: "total_tokens",
    name: "Tokens",
    color: "var(--chart-1)",
    axis: "left",
    type: "area",
  },
  {
    key: "input_tokens",
    name: "输入",
    color: "var(--chart-2)",
    axis: "left",
    type: "line",
  },
  {
    key: "output_tokens",
    name: "输出",
    color: "var(--chart-3)",
    axis: "left",
    type: "line",
    dash: "5 4",
  },
  {
    key: "cache_read_tokens",
    name: "缓存命中",
    color: "var(--chart-4)",
    axis: "left",
    type: "line",
    dash: "1 3",
  },
  {
    key: "total_cost_usd",
    name: "成本",
    color: "var(--chart-5)",
    axis: "right",
    type: "line",
    dash: "6 3",
  },
]

export function UsageTrendChart({ filter }: { filter: UsageFilter }) {
  const {
    data = [],
    isLoading,
    error,
  } = useTrendQuery(filter, {
    pollingInterval: 30_000,
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle>使用趋势</CardTitle>
      </CardHeader>
      <CardContent>
        <QueryState
          isLoading={isLoading}
          error={error}
          isEmpty={data.length === 0}
          emptyLabel="无趋势数据"
          emptyDescription="采集本地日志后，按日聚合的用量将显示在此。"
        >
          <div className="h-72 w-full">
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart
                data={data}
                margin={{ top: 8, right: 16, bottom: 0, left: 0 }}
              >
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis
                  dataKey="day"
                  tickFormatter={formatDay}
                  fontSize={12}
                  stroke="var(--muted-foreground)"
                />
                <YAxis
                  yAxisId="left"
                  tickFormatter={(v) => formatTokens(Number(v))}
                  fontSize={12}
                  stroke="var(--muted-foreground)"
                />
                <YAxis
                  yAxisId="right"
                  orientation="right"
                  tickFormatter={(v) => formatCost(Number(v))}
                  fontSize={12}
                  stroke="var(--muted-foreground)"
                />
                <Tooltip content={<TrendTooltip />} />
                <Legend />
                {SERIES.map((s) =>
                  s.type === "area" ? (
                    <Area
                      key={s.key}
                      yAxisId={s.axis}
                      type="monotone"
                      dataKey={s.key}
                      name={s.name}
                      stroke={s.color}
                      fill={s.color}
                      fillOpacity={0.08}
                      strokeWidth={2}
                      isAnimationActive={false}
                    />
                  ) : (
                    <Line
                      key={s.key}
                      yAxisId={s.axis}
                      type="monotone"
                      dataKey={s.key}
                      name={s.name}
                      stroke={s.color}
                      dot={false}
                      strokeWidth={1.5}
                      strokeDasharray={s.dash}
                      isAnimationActive={false}
                    />
                  ),
                )}
              </ComposedChart>
            </ResponsiveContainer>
          </div>
        </QueryState>
      </CardContent>
    </Card>
  )
}

function TrendTooltip(props: {
  active?: boolean
  payload?: Array<{
    dataKey: string
    value: number | null
    name: string
    color: string
  }>
  label?: string
}) {
  const { active, payload, label } = props
  if (!active || !payload?.length) return null
  return (
    <div className="bg-popover rounded-md border p-2 text-xs shadow-sm">
      <div className="mb-1 font-medium">{label ? formatDay(label) : ""}</div>
      {payload.map((p) => (
        <div
          key={p.dataKey}
          className="flex items-center justify-between gap-4"
        >
          <span className="flex items-center gap-1">
            <span
              className="inline-block size-2 rounded-full"
              style={{ backgroundColor: p.color }}
            />
            {p.name}
          </span>
          <span className="tabular-nums">
            {p.dataKey === "total_cost_usd"
              ? formatCost(p.value)
              : formatTokens(Number(p.value))}
          </span>
        </div>
      ))}
    </div>
  )
}

export type { TrendPoint }
