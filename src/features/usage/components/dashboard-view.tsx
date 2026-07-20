// Dashboard view (BLUEPRINT 数据看板; ADR-0006). Owns the filter state and
// composes the cockpit layout: quick filters + summary chips + advanced sheet,
// then the hero metrics (tokens / cost), token-breakdown card, trend chart,
// model distribution and request log. Reads refresh locally from SQLite (30s
// polling fallback, ADR-0005).

import { useMemo, useState } from "react"

import type { UsageFilter } from "@/types/generated/bindings"

import { FilterSheet } from "./filter-sheet"
import { FilterSummary } from "./filter-summary"
import { HeroMetric } from "./hero-metric"
import { ModelDistribution } from "./model-distribution"
import { QuickFilters } from "./quick-filters"
import { RequestLogTable } from "./request-log-table"
import { TokenBreakdownBar } from "./token-breakdown-bar"
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
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const usageFilter = useMemo(() => toFilter(filter), [filter])

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-3">
        <QuickFilters
          filter={filter}
          onChange={setFilter}
          onOpenAdvanced={() => setAdvancedOpen(true)}
        />
        <FilterSummary
          filter={filter}
          onClear={(k) => setFilter({ ...filter, [k]: "" })}
        />
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <HeroMetric filter={usageFilter} metric="tokens" />
        <HeroMetric filter={usageFilter} metric="cost" />
      </div>

      <TokenBreakdownBar filter={usageFilter} />
      <UsageTrendChart filter={usageFilter} />
      <ModelDistribution
        filter={usageFilter}
        onPickModel={(m) => setFilter((prev) => ({ ...prev, model: m }))}
      />
      <RequestLogTable filter={usageFilter} />

      <FilterSheet
        open={advancedOpen}
        onOpenChange={setAdvancedOpen}
        filter={filter}
        onChange={setFilter}
      />
    </div>
  )
}
