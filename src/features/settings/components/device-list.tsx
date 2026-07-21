// 多设备列表（ADR-0011）。列出所有已知设备；本机由 is_self 标记；可给其他
// 设备起显示名（setDeviceDisplayName，后端早已就绪、前端此前未渲染）。

import { useState } from "react"
import { toast } from "sonner"

import {
  useDevicesQuery,
  useSetDeviceDisplayNameMutation,
} from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"

export function DeviceList() {
  const { data: devices = [] } = useDevicesQuery()
  const [setName, { isLoading }] = useSetDeviceDisplayNameMutation()
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState("")

  return (
    <Card>
      <CardHeader>
        <CardTitle>设备列表</CardTitle>
        <CardDescription>
          所有曾同步的设备（ADR-0002）。可给其他设备起显示名，便于多设备间辨认。
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col">
        {devices.length === 0 ? (
          <span className="text-muted-foreground text-sm">暂无设备</span>
        ) : (
          devices.map((d, i) => (
            <div
              key={d.device_id}
              className={`flex items-center justify-between gap-3 py-2 ${i === devices.length - 1 ? "" : "border-b"}`}
            >
              <div className="flex min-w-0 flex-col gap-0.5">
                <span className="flex items-center gap-2">
                  <span className="truncate font-medium">
                    {d.display_name || "未命名"}
                  </span>
                  {d.is_self ? <Badge variant="secondary">本机</Badge> : null}
                </span>
                <span className="text-muted-foreground truncate font-mono text-xs">
                  {d.device_id}
                </span>
              </div>
              {d.is_self ? null : editing === d.device_id ? (
                <div className="flex items-center gap-2">
                  <Input
                    className="h-8 w-32"
                    placeholder="显示名"
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
                      if ("error" in r) toast.error("重命名失败")
                      else {
                        toast.success("已更新显示名")
                        setEditing(null)
                        setDraft("")
                      }
                    }}
                  >
                    保存
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                      setEditing(null)
                      setDraft("")
                    }}
                  >
                    取消
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
                  命名
                </Button>
              )}
            </div>
          ))
        )}
      </CardContent>
    </Card>
  )
}
