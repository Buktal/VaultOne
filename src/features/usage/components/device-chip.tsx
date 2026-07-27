// DeviceChip — chip-styled device picker for the control card / bar. Same
// filter.device_scope axis as DeviceScopeControl (which the lightweight card
// keeps using), but rendered as an h-8 chip to match DateRangeChip / ModelChip.
// Single-device renders null (the control only matters with peers).

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

import { useDeviceOptions } from "../use-device-options"

/** base-ui Select rejects an empty-string value; sentinel = "all devices". */
const ALL = "__all__"

export function DeviceChip({ align = "start" }: { align?: "start" | "end" }) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const scope = useAppSelector((s) => s.filter.filter.device_scope)
  const options = useDeviceOptions()

  if (options.length === 0) return null

  // Selected device vanished from the list (e.g. a peer reset) → fall back to
  // "all", with no per-device item highlighted.
  const active = options.some((o) => o.id === scope) ? scope : ""

  return (
    <Select
      value={active || ALL}
      onValueChange={(v) =>
        dispatch(patchFilter({ device_scope: v && v !== ALL ? v : "" }))
      }
    >
      <SelectTrigger
        className="border-border bg-card hover:bg-muted/60 h-8 w-36 rounded-md"
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
      {/* align=end in the right-column card so the menu opens flush to the
          trigger's right edge and grows leftward, never off the viewport. */}
      <SelectContent alignItemWithTrigger={false} align={align}>
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
