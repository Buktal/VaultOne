// DeviceScopeControl — 设备维度筛选 (ADR-0011 device_scope)。
//
// 单一下拉选择器，full dashboard 与 lightweight expanded 共用。读 listDevices()
// + 读写 filter.device_scope (filterSlice，不持久化——启动恒为"全部")。设备再多
// 也只是下拉里的一个选项，不会撑爆布局，故统一用 Select（不再按设备数切
// 分段 / 下拉）。单设备 (Standalone 仅本机) 无切换意义，整体不渲染。
//
// 统一控制: device_scope 在全局 filter，故 logs / lightweight 的 todayFilter 一并
// 跟随——lightweight-card 已把 todayFilter.device_scope 并入此值。

import { useTranslation } from "react-i18next"

import { useDevicesQuery } from "@/app/store/api"
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

/** base-ui Select 不接受空字符串 value，用哨兵代表「全部」。 */
const ALL = "__all__"

type Option = { id: string; label: string }

export function DeviceScopeControl({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const scope = useAppSelector((s) => s.filter.filter.device_scope)
  const { data: devices = [] } = useDevicesQuery()

  // 单设备 (Standalone 仅本机一台): 无切换意义，不渲染。
  if (devices.length <= 1) return null

  // 本机不显示默认设备名(如 Device-C), 直接写「本机」——用户一眼识别当前
  // 机器; 对端显示 display_name。trigger 选中本机时同样显示「本机」。
  const options: Option[] = devices.map((d) => ({
    id: d.device_id,
    label: d.is_self
      ? t("devices.thisDevice")
      : d.display_name || t("common.unnamed"),
  }))

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
          compact ? "h-8 w-32 text-[11px]" : "h-8 w-36",
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
          trigger 底), 而非 base-ui 默认把"选中项"贴齐 trigger 导致列表上下错位。 */}
      <SelectContent alignItemWithTrigger={false}>
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
