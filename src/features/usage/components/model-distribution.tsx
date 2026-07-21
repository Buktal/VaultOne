// Model distribution — top-N models by cost or tokens, with an "其他" aggregate.
// Clicking a row narrows the dashboard filter to that model (onPickModel), which
// re-runs every Usage-tagged query including this one (providesTags is
// filter-scoped, so the list itself refreshes too).

import { useState } from "react"
import { useModelsQuery } from "@/app/store/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { formatCost, formatTokens } from "@/lib/format"

import type { ModelStatsRow, UsageFilter } from "@/types/generated/bindings"

const TOP_N = 5

export function ModelDistribution({
  filter,
  onPickModel,
}: {
  filter: UsageFilter
  onPickModel: (model: string) => void
}) {
  const { data: rows = [] } = useModelsQuery(filter, {
    pollingInterval: 30_000,
  })
  const [metric, setMetric] = useState<"cost" | "tokens">("cost")

  const metricValue = (r: ModelStatsRow) =>
    metric === "cost" ? Number(r.total_cost_usd ?? 0) : Number(r.total_tokens)
  const fmt = metric === "cost" ? formatCost : formatTokens

  const sorted = [...rows].sort((a, b) => metricValue(b) - metricValue(a))
  const topRows = sorted.slice(0, TOP_N)
  const rest = sorted.slice(TOP_N)
  const restSum = rest.reduce((sum, r) => sum + metricValue(r), 0)
  const total = sorted.reduce((sum, r) => sum + metricValue(r), 0) || 1

  const items: Array<{ label: string; value: number; model: string | null }> = [
    ...topRows.map((r) => ({
      label: r.model,
      value: metricValue(r),
      model: r.model,
    })),
    ...(rest.length > 0
      ? [{ label: `其他 (${rest.length})`, value: restSum, model: null }]
      : []),
  ]

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>模型分布</CardTitle>
        <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
          {(["cost", "tokens"] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => setMetric(m)}
              className={`rounded-[5px] px-2 py-0.5 text-xs font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/40 ${
                metric === m
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {m === "cost" ? "成本" : "Tokens"}
            </button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {items.length === 0 ? (
          <span className="text-muted-foreground text-sm">无模型数据</span>
        ) : (
          items.map((it) => {
            const pct = (it.value / total) * 100
            return (
              <button
                key={it.label}
                type="button"
                disabled={!it.model}
                onClick={() => it.model && onPickModel(it.model)}
                className="group flex flex-col gap-1 text-left disabled:cursor-default"
              >
                <div className="flex items-center justify-between gap-2 text-xs">
                  <span className="text-foreground truncate font-mono group-hover:text-primary">
                    {it.label}
                  </span>
                  <span className="text-muted-foreground shrink-0 tabular-nums">
                    {fmt(it.value)} · {pct.toFixed(1)}%
                  </span>
                </div>
                <div className="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                  <div
                    className="bg-primary h-full rounded-full transition-all group-hover:bg-primary/80"
                    style={{ width: `${Math.max(pct, 2)}%` }}
                  />
                </div>
              </button>
            )
          })
        )}
      </CardContent>
    </Card>
  )
}
