// Command bar — the cockpit's always-on top strip. Left: active view title.
// Center: DataFreshness. Right: [采集 · dashboard only][同步 · synced only]
// [theme toggle]. The collect/sync actions live here (not buried in FilterBar
// or Settings) because they are the second-most-frequent action after reading.

import { Activity, RefreshCw } from "lucide-react"
import { toast } from "sonner"
import { useAppSelector } from "@/app/store/hooks"
import type { ViewId } from "@/app/store/slices/viewSlice"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { useGetAppInfoQuery, useSyncNowMutation } from "@/features/settings/api"
import { useCollectNowMutation } from "@/features/usage/api"
import { useFreshness } from "@/hooks/use-freshness"

import { DataFreshness } from "./data-freshness"

const VIEW_TITLES: Record<ViewId, string> = {
  dashboard: "数据看板",
  pricing: "成本定价",
  settings: "设置",
}

export function CommandBar() {
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useGetAppInfoQuery(undefined, { pollingInterval: 0 })
  const { markCollected, markSynced } = useFreshness()
  const [collect, { isLoading: collecting }] = useCollectNowMutation()
  const [syncNow, { isLoading: syncing }] = useSyncNowMutation()
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
    <header className="bg-background/80 sticky top-0 z-30 flex h-12 shrink-0 items-center gap-3 border-b px-4 backdrop-blur">
      <h1 className="text-sm font-semibold">{VIEW_TITLES[view]}</h1>
      <div className="flex flex-1 items-center">
        <DataFreshness />
      </div>
      <div className="flex items-center gap-1.5">
        {view === "dashboard" ? (
          <Button size="sm" disabled={collecting} onClick={onCollect}>
            <Activity />
            {collecting ? "采集中…" : "采集"}
          </Button>
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
