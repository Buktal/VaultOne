// 请求日志视图 (ADR-0011 / ADR-0013). 顶部 ControlBar (时间/模型/刷新/采集)
// + 全宽日志表 (固定时间倒序)。查询条件与看板共享同一 filterSlice。

import { useMemo } from "react"

import { useAppSelector } from "@/app/store/hooks"
import { toFilter } from "@/app/store/slices/filterSlice"

import { ControlBar } from "./control-card"
import { RequestLogTable } from "./request-log-table"

export function LogsView() {
  const filter = useAppSelector((s) => s.filter.filter)
  const usageFilter = useMemo(() => toFilter(filter), [filter])

  return (
    <div className="flex flex-col gap-4">
      <ControlBar />
      <RequestLogTable filter={usageFilter} />
    </div>
  )
}
