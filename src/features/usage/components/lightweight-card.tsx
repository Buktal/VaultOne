// Lightweight glance card (ADR-0015): the same main window morphed into a
// small, always-on-top, edge-dockable "today" snapshot. Read-only — total
// tokens + four buckets + cache-hit rate, same口径 as the dashboard's
// TokenHero (same toFilter path, same cache_hit_rate pass-through, same
// var(--chart-*) palette). No recent-activity list (that's the dashboard /
// future session layer).
//
// Refresh is free: providers.tsx invalidates the Usage tags on every
// `usage_changed`, and this query's providesTags key (filterId) matches the
// dashboard's "today" — so no polling here.

import dayjs from "dayjs"
import { Maximize2 } from "lucide-react"
import { useMemo } from "react"
import { useLightweightTuck } from "@/app/shell/use-lightweight-tuck"
import { useStatsQuery, ZERO_STATS } from "@/app/store/api"
import { useAppDispatch } from "@/app/store/hooks"
import { toFilter } from "@/app/store/slices/filterSlice"
import { setMode } from "@/app/store/slices/viewSlice"
import { Button } from "@/components/ui/button"
import { formatPct, formatTokens } from "@/lib/format"
import type { UsageStats } from "@/types/generated/bindings"

type BucketKey =
  | "input_tokens"
  | "output_tokens"
  | "cache_creation_tokens"
  | "cache_read_tokens"

// Same segments/order/palette as TokenHero — keep the two views visually
// identical so the glance and the dashboard read as one system.
const SEGMENTS: Array<{ key: BucketKey; label: string; color: string }> = [
  { key: "input_tokens", label: "输入", color: "var(--chart-input)" },
  { key: "output_tokens", label: "输出", color: "var(--chart-output)" },
  {
    key: "cache_creation_tokens",
    label: "缓存创建",
    color: "var(--chart-cache-create)",
  },
  {
    key: "cache_read_tokens",
    label: "缓存命中",
    color: "var(--chart-cache-read)",
  },
]

export function LightweightCard() {
  const dispatch = useAppDispatch()
  const { tucked, expand, scheduleTuck, cancelTuck } = useLightweightTuck()

  // 今日 · 全部设备 — reuses toFilter (local-day → UTC timestamp bounds) so the
  // 口径 is identical to the dashboard's "today" preset. Recomputed when the
  // local day rolls over (dep on `today`), not every render.
  const today = dayjs().format("YYYY-MM-DD")
  const todayFilter = useMemo(
    () =>
      toFilter({
        from_day: today,
        to_day: today,
        model: "",
        source: "",
        device_scope: "",
      }),
    [today],
  )

  const { data: stats } = useStatsQuery(todayFilter)
  const s: UsageStats = stats ?? ZERO_STATS
  const total = s.total_tokens || 1

  // Tucked "half-icon" (ADR-0015 灵动岛): a tiny mark docked at the right edge;
  // hover expands it back to the card.
  if (tucked) {
    return (
      <button
        type="button"
        className="bg-background flex h-screen w-screen cursor-default items-center justify-center border-0 p-0"
        onMouseEnter={expand}
        onClick={expand}
        aria-label="展开今日用量速览"
      >
        <img
          src="/vaultone-cream.svg"
          alt=""
          className="hidden dark:block size-9"
        />
        <img
          src="/vaultone-ink.svg"
          alt=""
          className="block dark:hidden size-9"
        />
      </button>
    )
  }

  return (
    <div
      className="bg-background text-foreground flex h-screen w-screen flex-col overflow-hidden"
      role="dialog"
      aria-label="今日用量速览"
      onMouseEnter={cancelTuck}
      onMouseLeave={scheduleTuck}
    >
      <div
        data-tauri-drag-region
        className="text-muted-foreground flex h-8 shrink-0 items-center justify-center text-xs select-none"
      >
        VaultOne · 今日
      </div>

      <div className="flex flex-col gap-3 p-3">
        <div className="flex flex-col gap-0.5">
          <span className="text-muted-foreground text-[11px]">总消耗</span>
          <span className="text-2xl font-semibold leading-none tabular-nums">
            {formatTokens(s.total_tokens)}
          </span>
        </div>

        <div className="bg-muted flex h-2 w-full overflow-hidden rounded-full">
          {SEGMENTS.map((seg) => {
            const v = Number(s[seg.key] ?? 0)
            const pct = (v / total) * 100
            return (
              <div
                key={seg.key}
                className="h-full"
                style={{ width: `${pct}%`, backgroundColor: seg.color }}
              />
            )
          })}
        </div>

        <div className="flex flex-col gap-1.5">
          {SEGMENTS.map((seg) => {
            const v = Number(s[seg.key] ?? 0)
            const pct = (v / total) * 100
            return (
              <div
                key={seg.key}
                className="flex items-center justify-between text-xs"
              >
                <span className="flex items-center gap-1.5">
                  <span
                    className="inline-block size-2 rounded-sm"
                    style={{ backgroundColor: seg.color }}
                  />
                  <span className="text-muted-foreground">{seg.label}</span>
                </span>
                <span className="tabular-nums">
                  {formatTokens(v)} · {pct.toFixed(0)}%
                </span>
              </div>
            )
          })}
        </div>

        <div className="flex items-center justify-between text-xs">
          <span className="text-muted-foreground">缓存命中率</span>
          <span className="tabular-nums font-medium">
            {formatPct(s.cache_hit_rate)}
          </span>
        </div>
      </div>

      <div className="mt-auto p-3 pt-0">
        <Button
          variant="outline"
          size="sm"
          className="w-full"
          onClick={() => dispatch(setMode("full"))}
        >
          <Maximize2 className="size-3.5" />
          展开完整
        </Button>
      </div>
    </div>
  )
}
