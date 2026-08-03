// ControlCard / ControlBar — shared meta-controls for the
// data views. Time range · model · refresh, plus the primary action with a
// data-freshness hint. Solid flat (no glass / no glow) — Pixso dark.
//
// Two layouts over the same controls + action:
//   - <ControlCard/>  纵向卡片 (dashboard 右栏): label+值 三行 + 分隔 + 主按钮.
//   - <ControlBar/>   横向条   (logs 顶部): chip 横排 + 主按钮居右.
// The primary action is mode-adaptive: Standalone ⇒ 「采集」 (local collect),
// Synced ⇒ 「同步」 (collect + pull + push — the full align). The run mode
// decides what it means; the button is always "refresh my data".
//
// 横排 ControlBar 的 chip 走 bar (纯值 + 选中「全部」时显全称「全部模型 /
// 全部来源 / 全部设备」自带身份, 与库一致), 纵卡 ControlCard 靠左 Row label,
// chip 只显「全部」. 来源 (source) 维度在多来源 (sources.length > 0) 时才出现
// —— 采到任意来源就显示, 与设备维度同理.

import { Activity, CalendarRange, ChevronDown } from "lucide-react"
import type { ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { DataFreshness } from "@/app/shell/data-freshness"
import {
  useCollectMutation,
  useDistinctModelsQuery,
  useDistinctSourcesQuery,
} from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import {
  type Preset,
  patchFilter,
  presetDays,
} from "@/app/store/slices/filterSlice"
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
import { useMutateWithToast } from "@/hooks/use-toast-mutation"
import { usePersistedState } from "@/lib/persistence"
import { cn } from "@/lib/utils"
import { sourceLabel } from "../source-labels"
import { useDeviceOptions } from "../use-device-options"
import { DeviceScopeControl } from "./device-scope-control"

const ALL = "__all__"

const CONTROL_COLLAPSE_KEY = "vaultone:control-collapsed"

/** Selectable presets in the popover — "custom" is implied by manual date entry,
 *  never shown as a button. */
type SelectablePreset = Exclude<Preset, "custom">

const PRESETS: Array<{ value: SelectablePreset; key: string }> = [
  { value: "today", key: "usage.control.today" },
  { value: "7d", key: "usage.control.last7d" },
  { value: "30d", key: "usage.control.last30d" },
  { value: "all", key: "usage.control.all" },
]

/** 主动作 (collectNow = align: Standalone ⇒ collect; Synced ⇒ collect + sync).
 *  触发 → 失效缓存 → 刷新新鲜度 → toast. 文案/反馈按模式自适应. */
function useCollectAction(multiDevice: boolean) {
  const { t } = useTranslation()
  const { markCollected } = useFreshness()
  const [collect, { isLoading: collecting }] = useCollectMutation()
  const runWithToast = useMutateWithToast()
  async function onCollect() {
    const ok = await runWithToast(collect, undefined, {
      success: {
        message: (data) =>
          t(multiDevice ? "usage.collect.doneSync" : "usage.collect.done", {
            rows: data.collected.rows_inserted ?? 0,
            files: data.collected.files_scanned ?? 0,
          }),
      },
      failed: { key: "usage.collect.failed" },
    })
    if (ok) markCollected()
  }
  return { onCollect, collecting }
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1">
      <span className="text-muted-foreground shrink-0 text-xs">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  )
}

function DateRangeChip({ align = "end" }: { align?: "start" | "end" }) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const { range_preset, from_day, to_day } = filter
  const label =
    range_preset === "all"
      ? t("usage.control.allTime")
      : range_preset !== "custom"
        ? t(
            PRESETS.find((p) => p.value === range_preset)?.key ??
              "usage.control.dateRange",
          )
        : from_day || to_day
          ? from_day === to_day
            ? from_day || "…"
            : `${from_day || "…"} → ${to_day || "…"}`
          : t("usage.control.allTime")

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="border-border bg-card hover:bg-muted/60 flex h-8 max-w-full min-w-0 items-center gap-1.5 rounded-md border px-3 text-sm whitespace-nowrap"
          >
            <CalendarRange className="text-muted-foreground size-3.5 shrink-0" />
            <span className="min-w-0 truncate">{label}</span>
          </button>
        }
      />
      <PopoverContent align={align} className="w-72">
        <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
          {PRESETS.map((p) => (
            <button
              key={p.value}
              type="button"
              onClick={() =>
                dispatch(
                  patchFilter({
                    range_preset: p.value,
                    ...presetDays(p.value),
                  }),
                )
              }
              className={cn(
                "focus-visible:ring-ring/40 rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors outline-none focus-visible:ring-2",
                range_preset === p.value
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
              value={from_day}
              onChange={(e) =>
                dispatch(
                  patchFilter({
                    range_preset: "custom",
                    from_day: e.target.value,
                  }),
                )
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
              value={to_day}
              onChange={(e) =>
                dispatch(
                  patchFilter({
                    range_preset: "custom",
                    to_day: e.target.value,
                  }),
                )
              }
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function ModelChip({
  align = "start",
  bar = false,
}: {
  align?: "start" | "end"
  /** 横排 ControlBar: 选中「全部」时显全称「全部模型」自带身份。 */
  bar?: boolean
}) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const { data: models = [] } = useDistinctModelsQuery()
  const allLabel = bar ? t("usage.control.allModel") : t("usage.control.all")
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
          // 模型名最长且不可控 → 横排 (bar) 给最宽。
          bar && "w-48",
        )}
        aria-label={t("usage.control.model")}
      >
        <SelectValue className="min-w-0">
          {(value: string) => (value === ALL ? allLabel : value)}
        </SelectValue>
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align={align}>
        <SelectItem value={ALL}>{allLabel}</SelectItem>
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
  bar = false,
}: {
  align?: "start" | "end"
  /** 横排 ControlBar: 选中「全部」时显全称「全部来源」自带身份。 */
  bar?: boolean
}) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const { data: sources = [] } = useDistinctSourcesQuery()
  const allLabel = bar ? t("usage.control.allSource") : t("usage.control.all")
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
          // 来源值固定短 (Claude Code / Gemini CLI) → 横排 (bar) w-40。
          bar && "w-40",
        )}
        aria-label={t("usage.control.source")}
      >
        <SelectValue className="min-w-0">
          {(value: string) => (value === ALL ? allLabel : sourceLabel(value))}
        </SelectValue>
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align={align}>
        <SelectItem value={ALL}>{allLabel}</SelectItem>
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
  const multiDevice = useDeviceOptions().length > 0
  const { onCollect, collecting } = useCollectAction(multiDevice)
  const { data: sources = [] } = useDistinctSourcesQuery()
  const hasSources = sources.length > 0
  // Collapse persists across restarts (debounced write, flushed on unmount).
  const [collapsed, setCollapsed] = usePersistedState<boolean>(
    CONTROL_COLLAPSE_KEY,
    false,
  )
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
              ? t(
                  multiDevice
                    ? "usage.collect.syncing"
                    : "usage.collect.collecting",
                )
              : t(multiDevice ? "usage.collect.sync" : "usage.collect.collect")}
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
  const multiDevice = useDeviceOptions().length > 0
  const { onCollect, collecting } = useCollectAction(multiDevice)
  const { data: sources = [] } = useDistinctSourcesQuery()
  const hasSources = sources.length > 0
  return (
    <div className="flex flex-wrap items-center gap-2">
      <DateRangeChip align="start" />
      {hasSources ? <SourceChip bar /> : null}
      <ModelChip bar />
      <DeviceScopeControl bar />
      <div className="flex-1" />
      <DataFreshness />
      <Button size="sm" disabled={collecting} onClick={onCollect}>
        <Activity />
        {collecting
          ? t(
              multiDevice
                ? "usage.collect.syncing"
                : "usage.collect.collecting",
            )
          : t(multiDevice ? "usage.collect.sync" : "usage.collect.collect")}
      </Button>
    </div>
  )
}
