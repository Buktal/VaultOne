// Usage trend chart (ADR-0011, token-first): stacked area of the four token
// buckets (input / output / cache creation / cache read). The stacked total
// IS total consumption — the chart shows how each day's tokens compose.
// Single token axis; cost is demoted to the KPI strip (token-first).
//
// NOTE: the spec's efficiency sub-charts (avg turn duration / request·turn by
// day) need per-day turn aggregates that TrendPoint does not carry today —
// only the global UsageStats has them. Daily turn trends require a backend
// change (extend TrendPoint); tracked in backlog.

import dayjs from "dayjs"
import {
  Area,
  AreaChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts"
import { useTrendQuery } from "@/app/store/api"
import { QueryState } from "@/components/query-state"
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { usePollingInterval } from "@/hooks/use-polling-interval"
import { formatDay, formatTokens } from "@/lib/format"

import type {
  TrendBucket,
  TrendPoint,
  UsageFilter,
} from "@/types/generated/bindings"

type Bucket = {
  key: keyof TrendPoint
  name: string
  color: string
}

// Stack order bottom → top: input (largest) at the base.
const BUCKETS: Bucket[] = [
  { key: "input_tokens", name: "输入", color: "var(--chart-input)" },
  { key: "output_tokens", name: "输出", color: "var(--chart-output)" },
  {
    key: "cache_creation_tokens",
    name: "缓存创建",
    color: "var(--chart-cache-create)",
  },
  {
    key: "cache_read_tokens",
    name: "缓存命中",
    color: "var(--chart-cache-read)",
  },
]

/** Hour bucket key `YYYY-MM-DDTHH` → `HH:00` for the axis / tooltip. */
function formatHour(key: string): string {
  return `${key.slice(11, 13)}:00`
}

export function UsageTrendChart({ filter }: { filter: UsageFilter }) {
  const pollingInterval = usePollingInterval()
  // A single local-day range collapses per-day resolution to one bar, so zoom
  // to hourly; anything wider stays per-day. A UTC+8 "today" maps to a 24h UTC
  // window that still falls on one local day, so isSame("day") catches it.
  const hourly =
    !!filter.from_ts &&
    !!filter.to_ts &&
    dayjs(filter.from_ts).isSame(filter.to_ts, "day")
  const bucket: TrendBucket = hourly ? "Hour" : "Day"
  const {
    data = [],
    isLoading,
    error,
  } = useTrendQuery({ filter, bucket }, { pollingInterval })

  return (
    <Card interactive>
      <CardHeader>
        <CardTitle>使用趋势</CardTitle>
        <CardAction>
          <span className="text-muted-foreground text-xs tabular-nums">
            {data.length > 0
              ? hourly
                ? `近 ${data.length} 小时`
                : `近 ${data.length} 天`
              : "无数据"}
          </span>
        </CardAction>
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
              <AreaChart
                data={data}
                margin={{ top: 8, right: 16, bottom: 0, left: 0 }}
              >
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis
                  dataKey="day"
                  tickFormatter={(v) =>
                    hourly ? formatHour(String(v)) : formatDay(String(v))
                  }
                  fontSize={12}
                  stroke="var(--muted-foreground)"
                />
                <YAxis
                  tickFormatter={(v) => formatTokens(Number(v))}
                  fontSize={12}
                  stroke="var(--muted-foreground)"
                />
                <Tooltip content={<TrendTooltip hourly={hourly} />} />
                <Legend />
                {BUCKETS.map((b) => (
                  <Area
                    key={b.key}
                    type="monotone"
                    dataKey={b.key}
                    name={b.name}
                    stackId="tokens"
                    stroke={b.color}
                    fill={b.color}
                    fillOpacity={0.75}
                    strokeWidth={1}
                    isAnimationActive={false}
                  />
                ))}
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </QueryState>
      </CardContent>
    </Card>
  )
}

type TooltipPayload = {
  dataKey: string
  value: number | null
  name: string
  color: string
}

function TrendTooltip(props: {
  active?: boolean
  payload?: TooltipPayload[]
  label?: string
  hourly?: boolean
}) {
  const { active, payload, label, hourly } = props
  if (!active || !payload?.length) return null
  const total = payload.reduce((sum, p) => sum + Number(p.value ?? 0), 0)
  return (
    <div className="bg-popover rounded-md border p-2 text-xs shadow-sm">
      <div className="mb-1 font-medium">
        {label ? (hourly ? formatHour(label) : formatDay(label)) : ""}
      </div>
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
          <span className="tabular-nums">{formatTokens(Number(p.value))}</span>
        </div>
      ))}
      <div className="mt-1 flex items-center justify-between gap-4 border-t pt-1 font-medium">
        <span>合计</span>
        <span className="tabular-nums">{formatTokens(total)}</span>
      </div>
    </div>
  )
}

export type { TrendPoint }
