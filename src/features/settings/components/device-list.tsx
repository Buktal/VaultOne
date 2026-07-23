// 多设备列表（ADR-0011）。列出所有已知设备；本机由 is_self 标记；可给其他
// 设备起显示名（setDeviceDisplayName，后端早已就绪、前端此前未渲染）。
//
// Content-only — 渲染在 SettingsView 的「设备」分区卡片内，不再自带 Card 壳。

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"

import {
  useDevicesQuery,
  useSetDeviceDisplayNameMutation,
} from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"

export function DeviceList() {
  const { t } = useTranslation()
  const { data: devices = [] } = useDevicesQuery()
  const [setName, { isLoading }] = useSetDeviceDisplayNameMutation()
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState("")

  if (devices.length === 0) {
    return (
      <span className="text-muted-foreground text-sm">
        {t("devices.empty")}
      </span>
    )
  }

  return (
    <div className="flex flex-col">
      {devices.map((d, i) => (
        <div
          key={d.device_id}
          className={`flex items-center justify-between gap-3 py-2 ${i === devices.length - 1 ? "" : "border-b"}`}
        >
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="flex items-center gap-2">
              <span className="truncate font-medium">
                {d.display_name || t("common.unnamed")}
              </span>
              {d.is_self ? (
                <Badge variant="secondary">{t("devices.thisDevice")}</Badge>
              ) : null}
            </span>
            <span className="text-muted-foreground truncate font-mono text-xs">
              {d.device_id}
            </span>
          </div>
          {d.is_self ? null : editing === d.device_id ? (
            <div className="flex items-center gap-2">
              <Input
                className="h-8 w-32"
                placeholder={t("devices.displayNamePlaceholder")}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
              />
              <Button
                size="sm"
                disabled={isLoading || !draft.trim()}
                onClick={async () => {
                  const r = await setName({
                    deviceId: d.device_id,
                    displayName: draft.trim(),
                  })
                  if ("error" in r)
                    toast.error(t("settings.toast.renameFailed"))
                  else {
                    toast.success(t("settings.toast.displayNameUpdated"))
                    setEditing(null)
                    setDraft("")
                  }
                }}
              >
                {t("common.save")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setEditing(null)
                  setDraft("")
                }}
              >
                {t("common.cancel")}
              </Button>
            </div>
          ) : (
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setEditing(d.device_id)
                setDraft(d.display_name ?? "")
              }}
            >
              {t("devices.rename")}
            </Button>
          )}
        </div>
      ))}
    </div>
  )
}
