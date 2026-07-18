// Dashboard view (BLUEPRINT 数据看板; ADR-0006). Owns the filter state and
// composes the filter bar, stat cards, trend chart and request-log table.
// Reads refresh locally from SQLite (30s polling fallback, ADR-0005).

import { useMemo, useState } from "react"
import type { UsageFilter } from "@/types/generated/bindings"
import { FilterBar } from "./filter-bar"
import { RequestLogTable } from "./request-log-table"
import { StatCards } from "./stat-cards"
import { UsageTrendChart } from "./usage-trend-chart"

/** Local filter state (empty string = "no constraint"); converted to UsageFilter. */
export interface FilterState {
  from_day: string
  to_day: string
  model: string
  source: string
  device_scope: string
}

const EMPTY_FILTER: FilterState = {
  from_day: "",
  to_day: "",
  model: "",
  source: "",
  device_scope: "",
}

function toFilter(s: FilterState): UsageFilter {
  return {
    from_day: s.from_day || null,
    to_day: s.to_day || null,
    model: s.model || null,
    source: s.source || null,
    device_scope: s.device_scope || null,
  }
}

export function DashboardView() {
  const [filter, setFilter] = useState<FilterState>(EMPTY_FILTER)
  const usageFilter = useMemo(() => toFilter(filter), [filter])

  return (
    <div className="flex flex-col gap-6">
      <FilterBar filter={filter} onChange={setFilter} />
      <StatCards filter={usageFilter} />
      <UsageTrendChart filter={usageFilter} />
      <RequestLogTable filter={usageFilter} />
    </div>
  )
}
