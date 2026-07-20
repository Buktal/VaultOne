// Data freshness indicator: a live pulse + relative "采集于 3 分钟前". Synced
// mode appends "· 同步 12 分钟前". Degrades to a first-run hint when no collect
// has ever landed. The relative string is recomputed on each render; callers
// that want it to tick over time can re-render on a timer (the CommandBar's
// queries poll, so it refreshes often enough).

import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"

import "dayjs/locale/zh-cn"

import { useFreshness } from "@/hooks/use-freshness"

dayjs.extend(relativeTime)
dayjs.locale("zh-cn")

export function DataFreshness() {
  const { state } = useFreshness()
  const collect = state.lastCollectAt
  const sync = state.lastSyncAt

  if (!collect) {
    return (
      <span className="text-muted-foreground text-xs">
        首次使用 — 点「采集」导入本地日志
      </span>
    )
  }

  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="relative flex size-1.5">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-emerald-400 opacity-75" />
        <span className="relative inline-flex size-1.5 rounded-full bg-emerald-400" />
      </span>
      <span className="text-muted-foreground">
        采集于
        <span className="text-foreground/80">{dayjs(collect).fromNow()}</span>
        {sync ? (
          <>
            {" · 同步 "}
            <span className="text-foreground/80">{dayjs(sync).fromNow()}</span>
          </>
        ) : null}
      </span>
    </div>
  )
}
