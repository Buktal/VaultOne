// Request log table — per-API-call ledger. Columns: Time / Provider / Billed
// Model / 输入 / 输出 / 缓存创建 / 缓存命中 / 总 Token / Cost / 停止原因 /
// Source / Device. `stop_reason` (end_turn / tool_use / max_tokens …) is the
// per-call end semantic. No latency / TTFT / HTTP-status columns (ADR-0003).
// Fixed time-desc (no sort UI); paginated; empty state offers an inline 采集
// CTA so the user isn't bounced to the command bar to seed the first rows.

import { FileText } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import {
  useCollectMutation,
  useCountQuery,
  useLogsQuery,
} from "@/app/store/api"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { formatCost, formatInt, formatTime } from "@/lib/format"
import { cn } from "@/lib/utils"

import type { UsageFilter, UsageLogRow } from "@/types/generated/bindings"

const PAGE_SIZE = 50

function tokenTotal(r: UsageLogRow): number {
  return (
    r.tokens.input +
    r.tokens.output +
    r.tokens.cache_creation +
    r.tokens.cache_read
  )
}

export function RequestLogTable({ filter }: { filter: UsageFilter }) {
  const [offset, setOffset] = useState(0)
  const {
    data: rows = [],
    isLoading,
    error,
  } = useLogsQuery({
    filter,
    limit: PAGE_SIZE,
    offset,
  })
  const { data: total = 0 } = useCountQuery(filter)
  const [collect, { isLoading: collecting }] = useCollectMutation()

  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))
  const page = Math.floor(offset / PAGE_SIZE) + 1

  async function onCollect() {
    const res = await collect()
    if ("error" in res) {
      toast.error("采集失败")
      return
    }
    toast.success(`采集完成：新增 ${res.data?.rows_inserted ?? 0} 条`)
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>请求日志</CardTitle>
      </CardHeader>
      <CardContent>
        <QueryState
          isLoading={isLoading}
          error={error}
          isEmpty={!isLoading && rows.length === 0}
          emptyIcon={FileText}
          emptyLabel="暂无请求记录"
          emptyAction={{
            label: collecting ? "采集中…" : "采集本地日志",
            onClick: onCollect,
            disabled: collecting,
          }}
        >
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>时间</TableHead>
                  <TableHead>供应商</TableHead>
                  <TableHead>计费模型</TableHead>
                  <TableHead className="text-right">输入</TableHead>
                  <TableHead className="text-right">输出</TableHead>
                  <TableHead className="text-right">缓存创建</TableHead>
                  <TableHead className="text-right">缓存命中</TableHead>
                  <TableHead className="text-right">总 Token</TableHead>
                  <TableHead className="text-right">成本</TableHead>
                  <TableHead>停止原因</TableHead>
                  <TableHead>来源</TableHead>
                  <TableHead>设备</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r) => (
                  <TableRow key={r.uuid}>
                    <TableCell className="tabular-nums whitespace-nowrap">
                      {formatTime(r.timestamp)}
                    </TableCell>
                    <TableCell>{providerLabel(r.source)}</TableCell>
                    <TableCell className="font-mono text-xs">
                      {r.model}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatInt(r.tokens.input)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatInt(r.tokens.output)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatInt(r.tokens.cache_creation)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatInt(r.tokens.cache_read)}
                    </TableCell>
                    <TableCell className="text-right font-medium tabular-nums">
                      {formatInt(tokenTotal(r))}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatCost(r.total_cost_usd)}
                    </TableCell>
                    <TableCell>
                      <StopReasonCell value={r.stop_reason} />
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {r.source}
                    </TableCell>
                    <TableCell
                      className="text-muted-foreground font-mono text-xs"
                      title={r.device_id || undefined}
                    >
                      {r.device_id ? r.device_id.slice(0, 8) : "—"}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </QueryState>

        <div className="text-muted-foreground mt-3 flex items-center justify-between text-xs">
          <span>
            第 {page} / {totalPages} 页 · 共 {formatInt(total)} 条
          </span>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
            >
              上一页
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={offset + PAGE_SIZE >= total}
              onClick={() => setOffset(offset + PAGE_SIZE)}
            >
              下一页
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function providerLabel(source: string): string {
  switch (source) {
    case "claude_code":
      return "Claude (Session)"
    default:
      return source || "—"
  }
}

/**
 * stop_reason → semantic tone. Free-form string from the source log, matched
 * by prefix/contains. Color signals outcome: normal completion / tool call
 * stay calm (green/blue), while hitting a limit (amber) or a refusal/error
 * (red) draws the eye. Unknown values fall back to neutral text — no chip.
 */
function stopReasonTone(
  value: string,
): "success" | "tool" | "warn" | "error" | null {
  const v = value.toLowerCase()
  if (!v) return null
  if (v === "end_turn") return "success"
  if (v.includes("tool_use")) return "tool"
  if (
    v.includes("max_tokens") ||
    v.includes("exceeded") ||
    v.includes("context_window")
  )
    return "warn"
  if (v.includes("refusal") || v.includes("error")) return "error"
  return null
}

function StopReasonCell({ value }: { value: string }) {
  if (!value) return <span className="text-muted-foreground">—</span>
  const tone = stopReasonTone(value)
  if (!tone)
    return (
      <span className="text-muted-foreground font-mono text-xs">{value}</span>
    )
  return <span className={cn("sr-chip font-mono", `sr-${tone}`)}>{value}</span>
}
