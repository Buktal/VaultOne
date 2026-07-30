// Model distribution — top-N models by cost or tokens, with an "其他" aggregate.
// Clicking a row narrows the dashboard filter to that model (onPickModel), which
// re-runs every Usage-tagged query including this one (providesTags is
// filter-scoped, so the list itself refreshes too).

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useModelsQuery } from "@/app/store/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { topNModels } from "@/features/usage/derive"
import { formatCost, formatTokens } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

const TOP_N = 5

export function ModelDistribution({
  filter,
  onPickModel,
}: {
  filter: UsageFilter
  onPickModel: (model: string) => void
}) {
  const { t } = useTranslation()
  const { data: rows = [] } = useModelsQuery(filter)
  const [metric, setMetric] = useState<"cost" | "tokens">("cost")

  const fmt = metric === "cost" ? formatCost : formatTokens
  const { top, rest, total } = topNModels(rows, metric, TOP_N)
  const items: Array<{ label: string; value: number; model: string | null }> = [
    ...top.map((row) => ({
      label: row.model,
      value: row.value,
      model: row.model,
    })),
    ...(rest.count > 0
      ? [
          {
            label: t("usage.models.others", { n: rest.count }),
            value: rest.sum,
            model: null,
          },
        ]
      : []),
  ]

  return (
    <Card interactive>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>{t("usage.models.title")}</CardTitle>
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
              {m === "cost" ? t("usage.models.cost") : t("usage.models.tokens")}
            </button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {items.length === 0 ? (
          <span className="text-muted-foreground text-sm">
            {t("usage.models.empty")}
          </span>
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
