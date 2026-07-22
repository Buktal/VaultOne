// RecentRequests (ADR-0011) — dashboard middle-column footer. Latest N request
// rows as a compact list + a "全部 →" link into the logs view. Doubles as a
// height-filler so the middle column tracks the right column, and as a quick
// path from the dashboard into the full ledger. Polls with the shared interval.

import { ArrowRight } from "lucide-react"
import { useMemo } from "react"
import { useCountQuery, useLogsQuery } from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { toFilter } from "@/app/store/slices/filterSlice"
import { setView } from "@/app/store/slices/viewSlice"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { usePollingInterval } from "@/hooks/use-polling-interval"
import { formatCost, formatInt, formatTime } from "@/lib/format"
import { cn } from "@/lib/utils"

import type { UsageLogRow } from "@/types/generated/bindings"

const LIMIT = 5

function tokenTotal(r: UsageLogRow): number {
  return (
    r.tokens.input +
    r.tokens.output +
    r.tokens.cache_creation +
    r.tokens.cache_read
  )
}

export function RecentRequests() {
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const usageFilter = useMemo(() => toFilter(filter), [filter])
  const pollingInterval = usePollingInterval()
  const { data: rows = [] } = useLogsQuery(
    { filter: usageFilter, limit: LIMIT, offset: 0 },
    { pollingInterval },
  )
  const { data: total = 0 } = useCountQuery(usageFilter, { pollingInterval })

  return (
    <Card interactive>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>近期请求</CardTitle>
        <button
          type="button"
          onClick={() => dispatch(setView("logs"))}
          className="text-primary hover:text-primary/80 inline-flex items-center gap-1 text-xs"
        >
          全部
          <ArrowRight className="size-3" />
          {total > 0 ? (
            <span className="text-muted-foreground">({formatInt(total)})</span>
          ) : null}
        </button>
      </CardHeader>
      <CardContent className="flex flex-col">
        {rows.length === 0 ? (
          <span className="text-muted-foreground py-6 text-center text-xs">
            暂无请求记录
          </span>
        ) : (
          rows.map((r, i) => (
            <div
              key={r.uuid}
              className={cn(
                "flex items-center gap-2 py-2.5",
                i > 0 && "border-border/60 border-t",
              )}
            >
              <span className="truncate font-mono text-xs font-medium">
                {r.model}
              </span>
              <div className="ml-auto flex shrink-0 items-center gap-3">
                <span className="text-foreground text-sm font-semibold tabular-nums">
                  {formatInt(tokenTotal(r))}
                  <span className="text-muted-foreground ml-1 text-[10px] font-normal">
                    tok
                  </span>
                </span>
                <span className="text-muted-foreground flex items-center gap-2 text-[11px] tabular-nums">
                  <span>入 {formatInt(r.tokens.input)}</span>
                  <span aria-hidden="true">·</span>
                  <span>出 {formatInt(r.tokens.output)}</span>
                  <span aria-hidden="true">·</span>
                  <span>{formatCost(r.total_cost_usd)}</span>
                  <span aria-hidden="true">·</span>
                  <span>{formatTime(r.timestamp)}</span>
                </span>
              </div>
            </div>
          ))
        )}
      </CardContent>
    </Card>
  )
}
