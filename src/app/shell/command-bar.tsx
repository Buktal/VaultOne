// Header bar (ADR-0014 D2/D6). Left: active view title. Right action:
// [采集 · dashboard only（旁挂数据新鲜度小字）][同步 · synced only][主题].
// Stitch 稿的 Export/Bell/Logout 已弃——VaultOne 无账号/通知/导出报告。
// 数据新鲜度从原命令栏中央挪到采集按钮旁，与采集动作同区（ADR-0013 动机保留）。

import { Activity, RefreshCw } from "lucide-react"
import { toast } from "sonner"
import {
  useAppInfoQuery,
  useCollectMutation,
  useSyncMutation,
} from "@/app/store/api"
import { useAppSelector } from "@/app/store/hooks"
import type { ViewId } from "@/app/store/slices/viewSlice"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { useFreshness } from "@/hooks/use-freshness"

import { DataFreshness } from "./data-freshness"

const VIEW_TITLES: Record<ViewId, string> = {
  dashboard: "数据看板",
  pricing: "成本定价",
  settings: "设置",
}

export function CommandBar() {
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useAppInfoQuery(undefined, { pollingInterval: 0 })
  const { markCollected, markSynced } = useFreshness()
  const [collect, { isLoading: collecting }] = useCollectMutation()
  const [syncNow, { isLoading: syncing }] = useSyncMutation()
  const synced = info?.mode === "synced"

  async function onCollect() {
    const res = await collect()
    if ("error" in res) {
      toast.error("采集失败")
      return
    }
    markCollected()
    const r = res.data
    toast.success(
      `采集完成：新增 ${r?.rows_inserted ?? 0} 条（扫描 ${r?.files_scanned ?? 0} 文件）`,
    )
  }

  async function onSync() {
    const res = await syncNow()
    if ("error" in res) {
      toast.error("同步失败")
      return
    }
    markSynced()
    const r = res.data
    toast.success(
      `已同步（导入 ${r?.imported ?? 0} 行${r?.pushed ? "，已推送" : ""}）`,
    )
  }

  return (
    <header className="bg-background/80 sticky top-0 z-30 flex h-14 shrink-0 items-center gap-3 border-b px-4 backdrop-blur">
      <h1 className="text-base font-semibold">{VIEW_TITLES[view]}</h1>
      <div className="flex-1" />
      <div className="flex items-center gap-2">
        {view === "dashboard" ? (
          <>
            <Button size="sm" disabled={collecting} onClick={onCollect}>
              <Activity />
              {collecting ? "采集中…" : "采集"}
            </Button>
            <DataFreshness />
          </>
        ) : null}
        {synced ? (
          <Button
            size="sm"
            variant="outline"
            disabled={syncing}
            onClick={onSync}
          >
            <RefreshCw />
            {syncing ? "同步中…" : "同步"}
          </Button>
        ) : null}
        <ThemeToggle />
      </div>
    </header>
  )
}
