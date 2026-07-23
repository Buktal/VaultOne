// Usage trend chart (ADR-0011, amended): multi-line chart of the four token
// buckets (input / output / cache creation / cache read). Each bucket is its
// own INDEPENDENT line with a solid dot per data point — no stacked area, no
// fill under the line. The point is to compare each bucket's trend over time,
// not to read cumulative composition (the stacked-area story the original
// ADR-0011 told; daily totals now come from the tooltip's total row + the KPI
// strip).
//
// Built on the shadcn Chart primitive (ChartContainer / ChartConfig /
// ChartLegend — see src/components/ui/chart.tsx). Colors flow straight from
// the semantic B-tier chart tokens (--chart-input / -output / -cache-create /
// -cache-read), so a skin swap changes the mood, never the meaning.
//
// NOTE: the spec's efficiency sub-charts (avg turn duration / request·turn by
// day) need per-day turn aggregates that TrendPoint does not carry today —
// only the global UsageStats has them. Daily turn trends require a backend
// change (extend TrendPoint); tracked in backlog.

import dayjs from "dayjs"
import { useTranslation } from "react-i18next"
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts"
import { useTrendQuery } from "@/app/store/api"
import { QueryState } from "@/components/query-state"
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  type ChartConfig,
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
} from "@/components/ui/chart"
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

// Order matches the KPI strip / token hero: input → output → cache creation →
// cache read. Each line keeps its hue family across skins (ADR-0013).
const BUCKETS: Bucket[] = [
  {
    key: "input_tokens",
    name: "usage.tokens.input",
    color: "var(--chart-input)",
  },
  {
    key: "output_tokens",
    name: "usage.tokens.output",
    color: "var(--chart-output)",
  },
  {
    key: "cache_creation_tokens",
    name: "usage.tokens.cacheCreation",
    color: "var(--chart-cache-create)",
  },
  {
    key: "cache_read_tokens",
    name: "usage.tokens.cacheRead",
    color: "var(--chart-cache-read)",
  },
]

/** Hour bucket key `YYYY-MM-DDTHH` → `HH:00` for the axis / tooltip. */
function formatHour(key: string): string {
  return `${key.slice(11, 13)}:00`
}

export function UsageTrendChart({ filter }: { filter: UsageFilter }) {
  const { t } = useTranslation()
  // A single local-day range collapses per-day resolution to one bar, so zoom
  // to hourly; anything wider stays per-day. A UTC+8 "today" maps to a 24h UTC
  // window that still falls on one local day, so isSame("day") catches it.
  const hourly =
    !!filter.from_ts &&
    !!filter.to_ts &&
    dayjs(filter.from_ts).isSame(filter.to_ts, "day")
  const bucket: TrendBucket = hourly ? "Hour" : "Day"
  const { data = [], isLoading, error } = useTrendQuery({ filter, bucket })

  // ChartConfig keys MUST equal the dataKeys (input_tokens …) so the shadcn
  // legend helper resolves label + color from payload.dataKey. stroke / dot
  // use the bucket's own color directly (var(--chart-*)), not the
  // ChartStyle-injected --color-<key> — keeps the source of truth in BUCKETS.
  const chartConfig = {
    input_tokens: {
      label: t("usage.tokens.input"),
      color: "var(--chart-input)",
    },
    output_tokens: {
      label: t("usage.tokens.output"),
      color: "var(--chart-output)",
    },
    cache_creation_tokens: {
      label: t("usage.tokens.cacheCreation"),
      color: "var(--chart-cache-create)",
    },
    cache_read_tokens: {
      label: t("usage.tokens.cacheRead"),
      color: "var(--chart-cache-read)",
    },
  } satisfies ChartConfig

  return (
    <Card interactive>
      <CardHeader>
        <CardTitle>{t("usage.trend.title")}</CardTitle>
        <CardAction>
          <span className="text-muted-foreground text-xs tabular-nums">
            {data.length > 0
              ? hourly
                ? t("usage.trend.lastHours", { n: data.length })
                : t("usage.trend.lastDays", { n: data.length })
              : t("usage.trend.noData")}
          </span>
        </CardAction>
      </CardHeader>
      <CardContent>
        <QueryState
          isLoading={isLoading}
          error={error}
          isEmpty={data.length === 0}
          emptyLabel={t("usage.trend.empty")}
          emptyDescription={t("usage.trend.emptyDesc")}
        >
          <ChartContainer config={chartConfig} className="h-72 w-full">
            <LineChart
              data={data}
              margin={{ top: 8, right: 16, bottom: 0, left: 0 }}
            >
              <CartesianGrid
                vertical={false}
                strokeDasharray="3 3"
                stroke="var(--border)"
              />
              <XAxis
                dataKey="day"
                tickLine={false}
                axisLine={false}
                tickFormatter={(v) =>
                  hourly ? formatHour(String(v)) : formatDay(String(v))
                }
                fontSize={12}
                stroke="var(--muted-foreground)"
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                tickFormatter={(v) => formatTokens(Number(v))}
                fontSize={12}
                stroke="var(--muted-foreground)"
              />
              <ChartTooltip content={<TrendTooltip hourly={hourly} />} />
              <ChartLegend content={<ChartLegendContent />} />
              {BUCKETS.map((b) => (
                <Line
                  key={b.key}
                  type="monotone"
                  dataKey={b.key}
                  name={t(b.name)}
                  stroke={b.color}
                  strokeWidth={2}
                  dot={{ r: 3, fill: b.color, strokeWidth: 0 }}
                  activeDot={{ r: 5, fill: b.color, strokeWidth: 0 }}
                  isAnimationActive={false}
                />
              ))}
            </LineChart>
          </ChartContainer>
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
  const { t } = useTranslation()
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
        <span>{t("usage.trend.total")}</span>
        <span className="tabular-nums">{formatTokens(total)}</span>
      </div>
    </div>
  )
}

export type { TrendPoint }
