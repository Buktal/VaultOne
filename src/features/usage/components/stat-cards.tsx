// Stat cards (BLUEPRINT 使用统计): total tokens, four-pack, cache-hit rate,
// request count, total cost. 30s polling fallback (ADR-0005).

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { useQueryUsageStatsQuery } from "@/features/usage/api"
import { formatCost, formatInt, formatPct, formatTokens } from "@/lib/format"

import type { UsageFilter } from "@/types/generated/bindings"

const ZERO = {
  request_count: 0,
  total_tokens: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_creation_tokens: 0,
  cache_read_tokens: 0,
  cache_hit_rate: 0,
  total_cost_usd: 0,
} as const

export function StatCards({ filter }: { filter: UsageFilter }) {
  const { data } = useQueryUsageStatsQuery(filter, {
    pollingInterval: 30_000,
  })
  const s = data ?? ZERO

  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-4 xl:grid-cols-8">
      <Stat
        title="真实消耗 Tokens"
        value={formatTokens(s.total_tokens)}
        hint={formatInt(s.total_tokens)}
        accent
      />
      <Stat title="输入" value={formatTokens(s.input_tokens)} />
      <Stat title="输出" value={formatTokens(s.output_tokens)} />
      <Stat title="缓存创建" value={formatTokens(s.cache_creation_tokens)} />
      <Stat title="缓存命中" value={formatTokens(s.cache_read_tokens)} />
      <Stat title="缓存命中率" value={formatPct(s.cache_hit_rate)} />
      <Stat title="请求次数" value={formatInt(s.request_count)} />
      <Stat title="累计成本" value={formatCost(s.total_cost_usd)} accent />
    </div>
  )
}

function Stat({
  title,
  value,
  hint,
  accent,
}: {
  title: string
  value: string
  hint?: string
  accent?: boolean
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-muted-foreground text-xs font-medium">
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent className="pt-0">
        <div
          className={`text-2xl font-semibold tabular-nums ${accent ? "text-primary" : ""}`}
        >
          {value}
        </div>
        {hint ? (
          <div className="text-muted-foreground mt-1 text-xs tabular-nums">
            {hint}
          </div>
        ) : null}
      </CardContent>
    </Card>
  )
}
