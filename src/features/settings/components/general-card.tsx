// General preferences (ADR-0012): background-collect interval + window close
// behavior. Content-only — rendered inside SettingsView's 通用 section card,
// so no Card wrapper of its own.

import { useEffect, useState } from "react"
import { toast } from "sonner"
import {
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
} from "@/app/store/api"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import type { CloseBehavior } from "@/types/preferences"

/** Pull a human-readable reason out of an RTK Query mutation error. */
function describeError(e: unknown): string {
  if (e && typeof e === "object") {
    const m = e as Record<string, unknown>
    if (typeof m.message === "string") return m.message
    if (typeof m.data === "string") return m.data
    if (typeof m.error === "string") return m.error
  }
  return "未知原因"
}

const CLOSE_OPTIONS: ReadonlyArray<[CloseBehavior, string]> = [
  ["ask", "每次询问"],
  ["minimize", "最小化到托盘"],
  ["quit", "退出"],
]

export function GeneralCard() {
  const { data: prefs, error: prefsError } = usePreferencesQuery()
  const [setCloseBehavior, { isLoading: savingClose }] =
    useSetCloseBehaviorMutation()
  const [setCollectInterval, { isLoading: savingInterval }] =
    useSetCollectIntervalMutation()
  const [intervalMin, setIntervalMin] = useState("")

  useEffect(() => {
    if (prefs) {
      setIntervalMin(
        String(Math.max(1, Math.round(prefs.collect_interval_secs / 60))),
      )
    }
  }, [prefs])

  const onSaveInterval = async () => {
    const mins = Number(intervalMin)
    if (!Number.isFinite(mins) || mins < 1) {
      toast.error("请输入有效的分钟数（≥1）")
      return
    }
    const r = await setCollectInterval(Math.round(mins * 60))
    if ("error" in r)
      toast.error("保存失败", { description: describeError(r.error) })
    else toast.success(`采集间隔已设为 ${Math.round(mins)} 分钟`)
  }

  return (
    <div className="flex flex-col gap-5">
      {prefsError ? (
        <p className="border-destructive/40 bg-destructive/5 text-destructive rounded-md border p-2 text-xs leading-relaxed">
          无法读取设置：{describeError(prefsError)}
          。若刚更新过后端，请完全重启应用——Rust 后端不会随前端热重载。
        </p>
      ) : null}
      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">
          后台采集间隔（分钟）
        </Label>
        <div className="flex items-center gap-2">
          <Input
            type="number"
            min={1}
            max={60}
            value={intervalMin}
            onChange={(e) => setIntervalMin(e.target.value)}
            className="w-28"
          />
          <Button size="sm" disabled={savingInterval} onClick={onSaveInterval}>
            保存
          </Button>
        </div>
        <p className="text-muted-foreground text-xs">
          每隔几分钟读取一次本机记录。开启多设备同步后，每次读取也会顺带把新数据推到云端。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">关闭窗口时</Label>
        <div className="flex flex-wrap gap-2">
          {CLOSE_OPTIONS.map(([value, label]) => (
            <Button
              key={value}
              size="sm"
              variant={prefs?.close_behavior === value ? "default" : "outline"}
              disabled={savingClose}
              onClick={async () => {
                const r = await setCloseBehavior(value)
                if ("error" in r)
                  toast.error("保存失败", {
                    description: describeError(r.error),
                  })
              }}
            >
              {label}
            </Button>
          ))}
        </div>
        <p className="text-muted-foreground text-xs">
          选「最小化」或「退出」后，可在关闭确认框里勾选「不再提示」记住选择。
        </p>
      </div>
    </div>
  )
}
