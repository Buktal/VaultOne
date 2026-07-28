// ControlCard / ControlBar — shared meta-controls for the
// data views. Time range · model · refresh, plus the 采集 primary action with a
// data-freshness hint. Solid flat (no glass / no glow) — Pixso dark.
//
// Two layouts over the same controls + action:
//   - <ControlCard/>  纵向卡片 (dashboard 右栏): label+值 三行 + 分隔 + 采集.
//   - <ControlBar/>   横向条   (logs 顶部): chip 横排 + 采集按钮居右.
// Replaces the old <UsageToolbar/> + the collect/sync buttons in <CommandBar/>.
// Sync stays in Settings (config concern); collect lives here (data-refresh).
//
// 横排 ControlBar 的 chip 走 labeled (内嵌「label · 值」自带身份), 纵卡
// ControlCard 靠左 Row label, chip 只显值. 来源 (source) 维度在多来源
// (sources.length > 0) 时才出现 —— 采到任意来源就显示, 与设备维度同理.

import dayjs from "dayjs"
import { Activity, CalendarRange, ChevronDown } from "lucide-react"
import { type ReactNode, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { DataFreshness } from "@/app/shell/data-freshness"
import {
  useCollectMutation,
  useDistinctModelsQuery,
  useDistinctSourcesQuery,
} from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { type FilterState, patchFilter } from "@/app/store/slices/filterSlice"
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
import { sourceLabel } from "../source-labels"
import { useDeviceOptions } from "../use-device-options"
import { DeviceScopeControl } from "./device-scope-control"

const ALL = "__all__"

const CONTROL_COLLAPSE_KEY = "vaultone:control-collapsed"

type Preset = "today" | "7d" | "30d" | "all"

const PRESETS: Array<{ value: Preset; key: string }> = [
  { value: "today", key: "usage.control.today" },
  { value: "7d", key: "usage.control.last7d" },
  { value: "30d", key: "usage.control.last30d" },
  { value: "all", key: "usage.control.all" },
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
  const { t } = useTranslation()
  const { markCollected } = useFreshness()
  const [collect, { isLoading: collecting }] = useCollectMutation()
  async function onCollect() {
    const res = await collect()
    if ("error" in res) {
      toast.error(t("usage.collect.failed"))
      return
    }
    markCollected()
    const r = res.data
    toast.success(
      t("usage.collect.done", {
        rows: r?.rows_inserted ?? 0,
        files: r?.files_scanned ?? 0,
      }),
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

function DateRangeChip({ align = "end" }: { align?: "start" | "end" }) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const preset = derivePreset(filter)
  const label = preset
    ? t(
        PRESETS.find((p) => p.value === preset)?.key ??
          "usage.control.dateRange",
      )
    : filter.from_day || filter.to_day
      ? `${filter.from_day || "…"} → ${filter.to_day || "…"}`
      : t("usage.control.allTime")

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="border-border bg-card hover:bg-muted/60 flex h-8 items-center gap-1.5 rounded-md border px-3 text-sm"
          >
            <CalendarRange className="text-muted-foreground size-3.5" />
            {label}
          </button>
        }
      />
      <PopoverContent align={align} className="w-72">
        <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
          {PRESETS.map((p) => (
            <button
              key={p.value}
              type="button"
              onClick={() => dispatch(patchFilter(applyPreset(p.value)))}
              className={cn(
                "focus-visible:ring-ring/40 rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors outline-none focus-visible:ring-2",
                preset === p.value
                  ? "bg-accent-tint text-accent-brand-strong"
                  : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
              )}
            >
              {t(p.key)}
            </button>
          ))}
        </div>
        <div className="mt-3 flex flex-col gap-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">
              {t("usage.control.start")}
            </span>
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
            <span className="text-muted-foreground">
              {t("usage.control.end")}
            </span>
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

/**
 * 内嵌「label · value」的 trigger 内容 — 横排 ControlBar 没有外置 Row label,
 * 靠它让每个下拉自带身份 (模型 / 来源 / 设备). 未选 (=全部) 时值用 muted, 选了
 * 具体值转 foreground, 一眼看出哪个维度被激活. 纵卡 ControlCard 有左 Row label,
 * 不走这条.
 */
function LabeledSelectValue({
  label,
  value,
  isAll,
}: {
  label: string
  value: string
  isAll: boolean
}) {
  return (
    <>
      <span className="text-muted-foreground">{label}</span>
      <span className="text-muted-foreground">·</span>
      <span
        className={cn(
          "truncate",
          isAll ? "text-muted-foreground" : "text-foreground",
        )}
      >
        {value}
      </span>
    </>
  )
}

function ModelChip({
  align = "start",
  labeled = false,
}: {
  align?: "start" | "end"
  labeled?: boolean
}) {
  const { t } = useTranslation()
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
        className={cn(
          "border-border bg-card hover:bg-muted/60 h-8 w-36 rounded-md",
          // 模型名最长且不可控 → labeled 时给最宽; 三个 chip 按维度差异化
          // (模型 w-48 > 来源 w-40 > 设备 w-36), 贴合各自典型内容长度.
          labeled && "w-48",
        )}
        aria-label={t("usage.control.model")}
      >
        <SelectValue className="min-w-0">
          {(value: string) => {
            const isAll = value === ALL
            const display = isAll ? t("usage.control.all") : value
            return labeled ? (
              <LabeledSelectValue
                label={t("usage.control.model")}
                value={display}
                isAll={isAll}
              />
            ) : (
              display
            )
          }}
        </SelectValue>
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align={align}>
        <SelectItem value={ALL}>{t("usage.control.all")}</SelectItem>
        {models.map((m) => (
          <SelectItem key={m} value={m}>
            {m}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** 来源 (provider) 维度筛选 — 与 ModelChip 对称, 选项来自 queryDistinctSources. */
function SourceChip({
  align = "start",
  labeled = false,
}: {
  align?: "start" | "end"
  labeled?: boolean
}) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const { data: sources = [] } = useDistinctSourcesQuery()
  return (
    <Select
      value={filter.source || ALL}
      onValueChange={(v) =>
        dispatch(patchFilter({ source: v && v !== ALL ? v : "" }))
      }
    >
      <SelectTrigger
        className={cn(
          "border-border bg-card hover:bg-muted/60 h-8 w-36 rounded-md",
          // 来源值固定短 (Claude Code / Gemini CLI) → labeled 时 w-40.
          labeled && "w-40",
        )}
        aria-label={t("usage.control.source")}
      >
        <SelectValue className="min-w-0">
          {(value: string) => {
            const isAll = value === ALL
            const display = isAll ? t("usage.control.all") : sourceLabel(value)
            return labeled ? (
              <LabeledSelectValue
                label={t("usage.control.source")}
                value={display}
                isAll={isAll}
              />
            ) : (
              display
            )
          }}
        </SelectValue>
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align={align}>
        <SelectItem value={ALL}>{t("usage.control.all")}</SelectItem>
        {sources.map((s) => (
          <SelectItem key={s} value={s}>
            {sourceLabel(s)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** 纵向卡片版 — 看板右栏。标题行带主题切换 + 折叠。 */
export function ControlCard() {
  const { t } = useTranslation()
  const { onCollect, collecting } = useCollectAction()
  const multiDevice = useDeviceOptions().length > 0
  const { data: sources = [] } = useDistinctSourcesQuery()
  const hasSources = sources.length > 0
  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage === "undefined") return false
    return localStorage.getItem(CONTROL_COLLAPSE_KEY) === "1"
  })
  useEffect(() => {
    localStorage.setItem(CONTROL_COLLAPSE_KEY, collapsed ? "1" : "0")
  }, [collapsed])
  return (
    <Card size="sm" interactive>
      <CardHeader>
        <CardTitle>{t("usage.control.title")}</CardTitle>
        <CardAction>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={
              collapsed
                ? t("usage.control.expand")
                : t("usage.control.collapse")
            }
            onClick={() => setCollapsed((c) => !c)}
          >
            <ChevronDown
              className={cn(
                "size-4 transition-transform",
                collapsed && "-rotate-90",
              )}
            />
          </Button>
        </CardAction>
      </CardHeader>
      {collapsed ? null : (
        <CardContent className="flex flex-col gap-0">
          <Row label={t("usage.control.dateRange")}>
            <DateRangeChip />
          </Row>
          {hasSources ? (
            <Row label={t("usage.control.source")}>
              <SourceChip align="end" />
            </Row>
          ) : null}
          <Row label={t("usage.control.model")}>
            <ModelChip align="end" />
          </Row>
          {multiDevice ? (
            <Row label={t("usage.deviceScope.label")}>
              <DeviceScopeControl align="end" />
            </Row>
          ) : null}
          <div className="bg-border my-2 h-px" />
          <Button className="w-full" disabled={collecting} onClick={onCollect}>
            <Activity />
            {collecting
              ? t("usage.collect.collecting")
              : t("usage.collect.collect")}
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
  const { t } = useTranslation()
  const { onCollect, collecting } = useCollectAction()
  const { data: sources = [] } = useDistinctSourcesQuery()
  const hasSources = sources.length > 0
  return (
    <div className="flex flex-wrap items-center gap-2">
      <DateRangeChip align="start" />
      {hasSources ? <SourceChip labeled /> : null}
      <ModelChip labeled />
      <DeviceScopeControl labeled />
      <div className="flex-1" />
      <DataFreshness />
      <Button size="sm" disabled={collecting} onClick={onCollect}>
        <Activity />
        {collecting
          ? t("usage.collect.collecting")
          : t("usage.collect.collect")}
      </Button>
    </div>
  )
}
