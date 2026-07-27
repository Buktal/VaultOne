// Shared device-picker source. One place decides how a device id becomes a
// label, so the dashboard chip, the lightweight picker, the recent-requests
// list and the logs table all agree. Single-device (Standalone only has this
// machine) returns [] / empty map so callers render nothing and stay quiet.

import { useTranslation } from "react-i18next"

import { useDevicesQuery } from "@/app/store/api"

export interface DeviceOption {
  id: string
  label: string
  is_self: boolean
}

/**
 * Devices as picker options. `is_self` → localized "This device"; a peer → its
 * display name (or "Unnamed"). Returns `[]` when there is ≤1 device, so a
 * single-machine Standalone setup renders no device UI at all.
 */
export function useDeviceOptions(): DeviceOption[] {
  const { t } = useTranslation()
  const { data: devices = [] } = useDevicesQuery()
  if (devices.length <= 1) return []
  return devices.map((d) => ({
    id: d.device_id,
    label: d.is_self
      ? t("devices.thisDevice")
      : d.display_name || t("common.unnamed"),
    is_self: d.is_self,
  }))
}

/** id → label lookup for tables / lists. Empty when single-device (no noise). */
export function useDeviceLabelMap(): Map<string, string> {
  const options = useDeviceOptions()
  return new Map(options.map((o) => [o.id, o.label]))
}
