// DateRangeChip — 共享时间范围 popover: 一排预设按钮 (今天 / 7天 / 30天 /
// 全部) + 两个手填日期框. 纯展示: 调用方拥有值 (preset / fromDay / toDay) 与
// 回调 (onPreset / onFromDay / onToDay), 所以这一份组件同时支撑 usage 的
// ControlBar (值经 Redux filterSlice 读写) 与 sessions 工具栏 (本地 hook
// state). i18n key 由调用方传入, 各视图保留自己的翻译命名空间 —— JSX 与
// 标签拼装逻辑只此一份 (此前两处各抄一份).
//
// `onPreset` 只回传 preset 值本身; 由调用方负责同时落具体 day 边界
// (presetDays) —— 本组件不碰任何状态, 只上报点击.

import { CalendarRange } from "lucide-react"
import { useTranslation } from "react-i18next"
import { effectiveDays, type Preset } from "@/app/store/slices/filterSlice"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { cn } from "@/lib/utils"

/** 可选预设 —— "custom" 由手填日期隐式触发, 永不作为按钮出现. */
export type SelectablePreset = Exclude<Preset, "custom">

/** 一个预设按钮: 值 + 其 i18n key. 各视图自带 key 列表注入. */
export interface DateRangePreset {
  value: SelectablePreset
  key: string
}

export interface DateRangeChipProps {
  preset: Preset
  fromDay: string
  toDay: string
  onPreset: (p: Preset) => void
  onFromDay: (d: string) => void
  onToDay: (d: string) => void
  /** 预设按钮 (值 + i18n key) —— 按给定顺序渲染. */
  presets: ReadonlyArray<DateRangePreset>
  /** chip 标签的「全部时间」回退 i18n key. */
  allTimeKey: string
  /** 当前 preset 在 presets 里找不到匹配项时回退的 i18n key. */
  dateRangeKey: string
  /** 起始日期框标签的 i18n key. */
  startKey: string
  /** 结束日期框标签的 i18n key. */
  endKey: string
  /** popover 相对触发器的对齐. */
  align?: "start" | "end"
}

export function DateRangeChip({
  preset,
  fromDay,
  toDay,
  onPreset,
  onFromDay,
  onToDay,
  presets,
  allTimeKey,
  dateRangeKey,
  startKey,
  endKey,
  align = "start",
}: DateRangeChipProps) {
  const { t } = useTranslation()
  // 日期框显示 EFFECTIVE 天 —— 动态预设 (如昨天点的「今天」) 渲染当天, 而非
  // 冻结的存储值.
  const { from_day: effFrom, to_day: effTo } = effectiveDays({
    range_preset: preset,
    from_day: fromDay,
    to_day: toDay,
  })
  const label =
    preset === "all"
      ? t(allTimeKey)
      : preset !== "custom"
        ? t(presets.find((p) => p.value === preset)?.key ?? dateRangeKey)
        : fromDay || toDay
          ? fromDay === toDay
            ? fromDay || "…"
            : `${fromDay || "…"} → ${toDay || "…"}`
          : t(allTimeKey)

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
          {presets.map((p) => (
            <button
              key={p.value}
              type="button"
              onClick={() => onPreset(p.value)}
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
            <span className="text-muted-foreground">{t(startKey)}</span>
            <input
              type="date"
              value={effFrom}
              onChange={(e) => onFromDay(e.target.value)}
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">{t(endKey)}</span>
            <input
              type="date"
              value={effTo}
              onChange={(e) => onToDay(e.target.value)}
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
        </div>
      </PopoverContent>
    </Popover>
  )
}
