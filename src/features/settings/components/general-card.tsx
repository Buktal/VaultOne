// General preferences card (ADR-0012): background-collect interval + window
// close behavior. Mounted at the top of the Settings view.

import { useEffect, useState } from "react"
import { toast } from "sonner"
import {
  usePreferencesQuery,
  useSetCloseBehaviorMutation,
  useSetCollectIntervalMutation,
} from "@/app/store/api"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import type { CloseBehavior } from "@/types/preferences"

const CLOSE_OPTIONS: ReadonlyArray<[CloseBehavior, string]> = [
  ["ask", "每次询问"],
  ["minimize", "最小化到托盘"],
  ["quit", "退出"],
]

export function GeneralCard() {
  const { data: prefs } = usePreferencesQuery()
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
    if ("error" in r) toast.error("保存失败")
    else toast.success(`采集间隔已设为 ${Math.round(mins)} 分钟`)
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>通用</CardTitle>
        <CardDescription>
          后台采集与窗口行为（ADR-0012）。应用常驻托盘，关窗默认最小化、后台继续定时采集。
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
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
            采集是本地动作（零网络）；Synced
            模式下每次采集后链式推送，故该间隔也是同步仓库的 commit/push 频率。
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
                  if ("error" in r) toast.error("保存失败")
                }}
              >
                {label}
              </Button>
            ))}
          </div>
          <p className="text-muted-foreground text-xs">
            「最小化」/「退出」都可在关闭弹窗里勾选「不再提示」永久记住。
          </p>
        </div>
      </CardContent>
    </Card>
  )
}
