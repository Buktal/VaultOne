// Dashboard view — token-first LLM Usage Cockpit (ADR-0011, supersedes
// ADR-0009/0010 cost-first).
//
// 三栏布局 (ADR-0013): 左导航(Shell) · 中图表(趋势 / 模型分布 / 近期请求) ·
// 右数值(控制卡 / 总消耗锚点 / 概览)。筛选与采集从旧顶部 UsageToolbar +
// CommandBar 收敛进右栏 ControlCard；高级 FilterSheet 暂移除 (控制卡已覆盖
// 时间/模型/刷新/采集，source/device_scope 留待 backlog)。

import { useMemo } from "react"

import { useDevicesQuery } from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { patchFilter, toFilter } from "@/app/store/slices/filterSlice"

import { ControlCard } from "./control-card"
import { DeviceScopeControl } from "./device-scope-control"
import { KpiStrip } from "./kpi-strip"
import { ModelDistribution } from "./model-distribution"
import { RecentRequests } from "./recent-requests"
import { TokenHero } from "./token-hero"
import { UsageTrendChart } from "./usage-trend-chart"

export function DashboardView() {
  const dispatch = useAppDispatch()
  const filter = useAppSelector((s) => s.filter.filter)
  const usageFilter = useMemo(() => toFilter(filter), [filter])
  const { data: devices = [] } = useDevicesQuery()

  return (
    <div className="flex flex-col gap-4">
      {/* 设备视角切换 · 顶部右上 (ADR-0011 device_scope)。单设备时控件自渲染为
          null，此行也跳过避免空行；justify-end 呼应 TitleBar 靠右设计语言。 */}
      {devices.length > 1 ? (
        <div className="flex justify-end">
          <DeviceScopeControl />
        </div>
      ) : null}
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_320px]">
        {/* 中栏 · 可视化 */}
        <div className="flex flex-col gap-4">
          <UsageTrendChart filter={usageFilter} />
          <ModelDistribution
            filter={usageFilter}
            onPickModel={(m) => dispatch(patchFilter({ model: m }))}
          />
          <RecentRequests />
        </div>
        {/* 右栏 · 数值 */}
        <aside className="flex flex-col gap-4">
          <ControlCard />
          <TokenHero filter={usageFilter} />
          <KpiStrip filter={usageFilter} />
        </aside>
      </div>
    </div>
  )
}
