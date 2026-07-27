// DeviceScopeControl — 设备维度筛选。
//
// 单一下拉选择器，lightweight expanded 小窗用。读 listDevices() (经共享
// useDeviceOptions) + 读写 filter.device_scope。设备再多也只是下拉里的一
// 个选项，不会撑爆布局，故统一用 Select。单设备 (Standalone 仅本机) 无切
// 换意义，整体不渲染。
//
// 统一控制: device_scope 在全局 filter，故 logs / lightweight 的 todayFilter
// 一并跟随——lightweight-card 已把 todayFilter.device_scope 并入此值。
// 大窗口的设备切换已挪进 ControlCard 的 DeviceChip (本组件仅小窗使用)。

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

export function DeviceScopeControl({ compact = false }: { compact?: boolean }) {
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
          "border-border bg-card hover:bg-muted/60 rounded-md",
          compact ? "h-8 w-36 text-[11px]" : "h-8 w-36 text-[13px]",
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
          text size 跟随 trigger (compact 11px), 否则下拉字号会比选择器还大。 */}
      <SelectContent
        alignItemWithTrigger={false}
        align="start"
        className={compact ? "text-[11px]" : "text-[13px]"}
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
