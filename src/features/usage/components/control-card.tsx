// ControlCard / ControlBar (ADR-0011 / ADR-0013) — shared meta-controls for the
// data views. Time range · model · refresh, plus the 采集 primary action with a
// data-freshness hint. Solid flat (no glass / no glow) — Pixso dark.
//
// Two layouts over the same controls + action:
//   - <ControlCard/>  纵向卡片 (dashboard 右栏): label+值 三行 + 分隔 + 采集.
//   - <ControlBar/>   横向条   (logs 顶部): chip 横排 + 采集按钮居右.
// Replaces the old <UsageToolbar/> + the collect/sync buttons in <CommandBar/>.
// Sync stays in Settings (config concern); collect lives here (data-refresh).

import dayjs from "dayjs"
import { Activity, CalendarRange, ChevronDown, RefreshCw } from "lucide-react"
import { type ReactNode, useEffect, useState } from "react"
import { toast } from "sonner"
import { DataFreshness } from "@/app/shell/data-freshness"
import { useCollectMutation, useDistinctModelsQuery } from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { type FilterState, patchFilter } from "@/app/store/slices/filterSlice"
import {
  REFRESH_OPTIONS,
  type RefreshInterval,
  setRefreshInterval,
} from "@/app/store/slices/uiSlice"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useFreshness } from "@/hooks/use-freshness"
import { cn } from "@/lib/utils"

const ALL = "__all__"

const CONTROL_COLLAPSE_KEY = "vaultone:control-collapsed"

type Preset = "today" | "7d" | "30d" | "all"

const PRESETS: Array<{ value: Preset; label: string }> = [
  { value: "today", label: "今天" },
  { value: "7d", label: "7天" },
  { value: "30d", label: "30天" },
  { value: "all", label: "全部" },
]

function dayStr(offset = 0) {
  return dayjs().add(offset, "day").format("YYYY-MM-DD")
}

function derivePreset(f: FilterState): Preset | null {
  const today = dayStr()
  if (!f.from_day && !f.to_day) return "all"
  if (f.from_day === today && f.to_day === today) return "today"
  if (f.from_day === dayStr(-6) && f.to_day === today) return "7d"
  if (f.from_day === dayStr(-29) && f.to_day === today) return "30d"
  return null
}

function applyPreset(p: Preset): Pick<FilterState, "from_day" | "to_day"> {
  switch (p) {
    case "today":
      return { from_day: dayStr(), to_day: dayStr() }
    case "7d":
      return { from_day: dayStr(-6), to_day: dayStr() }
    case "30d":
      return { from_day: dayStr(-29), to_day: dayStr() }
    default:
      return { from_day: "", to_day: "" }
  }
}

/** 采集动作 (触发 collectNow → 失效缓存 → 刷新新鲜度 → toast). */
function useCollectAction() {
  const { markCollected } = useFreshness()
  const [collect, { isLoading: collecting }] = useCollectMutation()
  async function onCollect() {
    const res = await collect()
    if ("error" in res) {
      toast.error("采集失败")
      return
    }
    markCollected()
    const r = res.data
    toast.success(
      `采集完成：新增 ${r?.rows_inserted ?? 0} 条（扫描 ${r?.files_scanned ?? 0} 文件）`,
    )
  }
  return { onCollect, collecting }
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1">
      <span className="text-muted-foreground text-xs">{label}</span>
      {children}
    </div>
  )
}

function DateRangeChip() {
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const preset = derivePreset(filter)
  const label = preset
    ? (PRESETS.find((p) => p.value === preset)?.label ?? "时间范围")
    : filter.from_day || filter.to_day
      ? `${filter.from_day || "…"} → ${filter.to_day || "…"}`
      : "全部时间"

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="bg-muted hover:bg-muted/70 flex h-8 items-center gap-1.5 rounded-md px-3 text-sm"
          >
            <CalendarRange className="text-muted-foreground size-3.5" />
            {label}
          </button>
        }
      />
      <PopoverContent align="end" className="w-72">
        <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
          {PRESETS.map((p) => (
            <button
              key={p.value}
              type="button"
              onClick={() => dispatch(patchFilter(applyPreset(p.value)))}
              className={cn(
                "focus-visible:ring-ring/40 rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors outline-none focus-visible:ring-2",
                preset === p.value
                  ? "bg-background text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {p.label}
            </button>
          ))}
        </div>
        <div className="mt-3 flex flex-col gap-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">开始</span>
            <input
              type="date"
              value={filter.from_day}
              onChange={(e) =>
                dispatch(patchFilter({ from_day: e.target.value }))
              }
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">结束</span>
            <input
              type="date"
              value={filter.to_day}
              onChange={(e) =>
                dispatch(patchFilter({ to_day: e.target.value }))
              }
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function ModelChip() {
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const { data: models = [] } = useDistinctModelsQuery()
  return (
    <Select
      value={filter.model || ALL}
      onValueChange={(v) =>
        dispatch(patchFilter({ model: v && v !== ALL ? v : "" }))
      }
    >
      <SelectTrigger
        className="bg-muted hover:bg-muted/70 h-8 w-28 rounded-md border-transparent"
        aria-label="模型"
      >
        <SelectValue>
          {(value: string) => (value === ALL ? "全部" : value)}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL}>全部</SelectItem>
        {models.map((m) => (
          <SelectItem key={m} value={m}>
            {m}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function RefreshChip() {
  const dispatch = useAppDispatch()
  const value = useAppSelector((s) => s.ui.refreshInterval)
  return (
    <Select
      value={String(value)}
      onValueChange={(v) =>
        dispatch(setRefreshInterval(Number(v) as RefreshInterval))
      }
    >
      <SelectTrigger
        className="bg-muted hover:bg-muted/70 h-8 w-28 rounded-md border-transparent"
        aria-label="自动刷新"
      >
        <RefreshCw className="text-muted-foreground size-3.5" />
        <SelectValue>
          {(value: string) =>
            REFRESH_OPTIONS.find((o) => String(o.value) === value)?.label ??
            "关闭"
          }
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        {REFRESH_OPTIONS.map((o) => (
          <SelectItem key={o.value} value={String(o.value)}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** 纵向卡片版 — 看板右栏。标题行带主题切换 + 折叠 (ADR-0013 v2)。 */
export function ControlCard() {
  const { onCollect, collecting } = useCollectAction()
  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage === "undefined") return false
    return localStorage.getItem(CONTROL_COLLAPSE_KEY) === "1"
  })
  useEffect(() => {
    localStorage.setItem(CONTROL_COLLAPSE_KEY, collapsed ? "1" : "0")
  }, [collapsed])
  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle>控制</CardTitle>
        <CardAction>
          <div className="flex items-center gap-0.5">
            <ThemeToggle />
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={collapsed ? "展开控制" : "收起控制"}
              onClick={() => setCollapsed((c) => !c)}
            >
              <ChevronDown
                className={cn(
                  "size-4 transition-transform",
                  collapsed && "-rotate-90",
                )}
              />
            </Button>
          </div>
        </CardAction>
      </CardHeader>
      {collapsed ? null : (
        <CardContent className="flex flex-col gap-0">
          <Row label="时间范围">
            <DateRangeChip />
          </Row>
          <Row label="模型">
            <ModelChip />
          </Row>
          <Row label="刷新">
            <RefreshChip />
          </Row>
          <div className="bg-border my-2 h-px" />
          <Button className="w-full" disabled={collecting} onClick={onCollect}>
            <Activity />
            {collecting ? "采集中…" : "采集"}
          </Button>
          <div className="mt-3">
            <DataFreshness />
          </div>
        </CardContent>
      )}
    </Card>
  )
}

/** 横向条版 — 日志页顶部。 */
export function ControlBar() {
  const { onCollect, collecting } = useCollectAction()
  return (
    <div className="flex flex-wrap items-center gap-2">
      <DateRangeChip />
      <ModelChip />
      <RefreshChip />
      <div className="flex-1" />
      <ThemeToggle />
      <DataFreshness />
      <Button size="sm" disabled={collecting} onClick={onCollect}>
        <Activity />
        {collecting ? "采集中…" : "采集"}
      </Button>
    </div>
  )
}
