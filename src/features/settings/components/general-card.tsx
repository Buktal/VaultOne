// General preferences (ADR-0012 / ADR-0014): background-collect interval,
// push-to-sync interval, and window-close behavior. Content-only — rendered
// inside SettingsView's 通用 section card, so no Card wrapper of its own.
//
// Collect and push are DECOUPLED (ADR-0014): collect is a short seconds-level
// local cadence, push is a longer minutes-level Git cadence (Synced only).
// Both are discrete presets (Select, instant-effect) — no save button.

import { toast } from "sonner"
import {
  useAppInfoQuery,
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
  useSetPushIntervalMutation,
} from "@/app/store/api"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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

/** Collect presets (ADR-0014): seconds-level, local-only. */
const COLLECT_OPTIONS: ReadonlyArray<[number, string]> = [
  [10, "10 秒"],
  [30, "30 秒"],
  [60, "60 秒"],
]

/** Push presets (ADR-0014): minutes-level, Git, Synced only. */
const PUSH_OPTIONS: ReadonlyArray<[number, string]> = [
  [300, "5 分钟"],
  [600, "10 分钟"],
  [900, "15 分钟"],
  [1800, "30 分钟"],
  [3600, "60 分钟"],
]

export function GeneralCard() {
  const { data: prefs, error: prefsError } = usePreferencesQuery()
  const { data: info } = useAppInfoQuery()
  const synced = info?.mode === "synced"
  const [setCloseBehavior, { isLoading: savingClose }] =
    useSetCloseBehaviorMutation()
  const [setCollectInterval, { isLoading: savingCollect }] =
    useSetCollectIntervalMutation()
  const [setPushInterval, { isLoading: savingPush }] =
    useSetPushIntervalMutation()

  return (
    <div className="flex flex-col gap-5">
      {prefsError ? (
        <p className="border-destructive/40 bg-destructive/5 text-destructive rounded-md border p-2 text-xs leading-relaxed">
          无法读取设置：{describeError(prefsError)}
          。若刚更新过后端，请完全重启应用——Rust 后端不会随前端热重载。
        </p>
      ) : null}

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">后台采集间隔</Label>
        <Select
          value={prefs ? String(prefs.collect_interval_secs) : undefined}
          onValueChange={async (v) => {
            const r = await setCollectInterval(Number(v))
            if ("error" in r)
              toast.error("保存失败", { description: describeError(r.error) })
          }}
        >
          <SelectTrigger className="w-36" disabled={savingCollect}>
            <SelectValue placeholder="—" />
          </SelectTrigger>
          <SelectContent>
            {COLLECT_OPTIONS.map(([v, label]) => (
              <SelectItem key={v} value={String(v)}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-muted-foreground text-xs">
          每隔几秒读取一次本机记录（纯本地，不联网）。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-muted-foreground text-xs">同步上报间隔</Label>
        {synced ? (
          <Select
            value={prefs ? String(prefs.push_interval_secs) : undefined}
            onValueChange={async (v) => {
              const r = await setPushInterval(Number(v))
              if ("error" in r)
                toast.error("保存失败", { description: describeError(r.error) })
            }}
          >
            <SelectTrigger className="w-36" disabled={savingPush}>
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {PUSH_OPTIONS.map(([v, label]) => (
                <SelectItem key={v} value={String(v)}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <p className="text-muted-foreground text-xs">
            需先在「同步」里开启多设备同步。
          </p>
        )}
        <p className="text-muted-foreground text-xs">
          每隔几分钟把新数据推送到 Git（仅多设备同步模式）。
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
