// DeviceScopeControl — 设备维度筛选的单一下拉。
//
// 读 listDevices() (经共享 useDeviceOptions) + 读写 filter.device_scope。
// 设备再多也只是下拉里的一个选项，不会撑爆布局，故统一用 Select。单设备
// (Standalone 仅本机) 无切换意义，整体不渲染。
//
// 同一组件两种形态:
//  - ControlCard / ControlBar (control-card.tsx): 默认字号; align 跟随布局
//    (卡片右栏 align="end" 让菜单向左生长不溢出视口, 条形 align="start")。
//  - LightweightCard expanded (lightweight-card.tsx): compact (11px) 适配小窗。
//
// device_scope 在全局 filter, 故 logs / lightweight 的 todayFilter 一并跟随。

import { useTranslation } from "react-i18next"

import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { patchFilter } from "@/app/store/slices/filterSlice"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { cn } from "@/lib/utils"

import { useDeviceOptions } from "../use-device-options"

/** base-ui Select 不接受空字符串 value，用哨兵代表「全部」。 */
const ALL = "__all__"

export function DeviceScopeControl({
  compact = false,
  align = "start",
}: {
  compact?: boolean
  align?: "start" | "end"
}) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const scope = useAppSelector((s) => s.filter.filter.device_scope)
  const options = useDeviceOptions()

  // 单设备 (Standalone 仅本机一台): 无切换意义，不渲染。
  if (options.length === 0) return null

  // 选中设备从列表消失 (如对端重置) → 回退「全部」，无设备项高亮。
  const active = options.some((o) => o.id === scope) ? scope : ""

  return (
    <Select
      value={active || ALL}
      onValueChange={(v) =>
        dispatch(patchFilter({ device_scope: v && v !== ALL ? v : "" }))
      }
    >
      <SelectTrigger
        className={cn(
          "border-border bg-card hover:bg-muted/60 h-8 w-36 rounded-md",
          compact && "text-[11px]",
        )}
        aria-label={t("usage.deviceScope.label")}
      >
        <SelectValue className="min-w-0">
          {(value: string) =>
            value === ALL
              ? t("usage.control.all")
              : (options.find((o) => o.id === value)?.label ?? value)
          }
        </SelectValue>
      </SelectTrigger>
      {/* alignItemWithTrigger=false: 弹出层从 trigger 底部往下展开(列表顶对齐
          trigger 底), 而非 base-ui 默认把"选中项"贴齐 trigger 导致列表上下错位。
          compact 时下拉字号跟随 trigger (11px), 否则用默认。align=end 让右栏
          卡片的菜单向左生长, 不溢出视口。 */}
      <SelectContent
        alignItemWithTrigger={false}
        align={align}
        className={cn(compact && "text-[11px]")}
      >
        <SelectItem value={ALL}>{t("usage.control.all")}</SelectItem>
        {options.map((o) => (
          <SelectItem key={o.id} value={o.id}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
