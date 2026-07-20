// Request log table (BLUEPRINT 请求日志; ADR-0003 columns): Time / Provider /
// Billed Model / 输入 / 输出 / 缓存创建 / 缓存命中 / Cost / Source.
// No latency / TTFT / status columns (ADR-0003: cut — no source). Paginated.
// Rows highlight on hover; the empty state offers an inline 采集 CTA so the
// user isn't bounced to the command bar to seed the first rows.

import { FileText } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

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
import {
  useCollectNowMutation,
  useCountUsageLogsQuery,
  useQueryUsageLogsQuery,
} from "@/features/usage/api"
import { formatCost, formatInt, formatTime } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

const PAGE_SIZE = 50

export function RequestLogTable({ filter }: { filter: UsageFilter }) {
  const [offset, setOffset] = useState(0)
  const {
    data: rows = [],
    isLoading,
    error,
  } = useQueryUsageLogsQuery({
    filter,
    limit: PAGE_SIZE,
    offset,
  })
  const { data: total = 0 } = useCountUsageLogsQuery(filter)
  const [collect, { isLoading: collecting }] = useCollectNowMutation()

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
                <TableHead className="text-right">成本</TableHead>
                <TableHead>来源</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow
                  key={r.uuid}
                  className="transition-colors hover:bg-muted/40"
                >
                  <TableCell className="tabular-nums whitespace-nowrap">
                    {formatTime(r.timestamp)}
                  </TableCell>
                  <TableCell>{providerLabel(r.source)}</TableCell>
                  <TableCell className="font-mono text-xs">{r.model}</TableCell>
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
                  <TableCell className="text-right tabular-nums">
                    {formatCost(r.total_cost_usd)}
                  </TableCell>
                  <TableCell className="text-muted-foreground text-xs">
                    {r.source}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
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
