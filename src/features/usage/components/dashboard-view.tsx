// Dashboard view — cost-first LLM Usage Cockpit.
//
// Layout (top → bottom): quick filters + advanced-filter entry → active-filter
// chips → Cost Hero (full-width anchor) → secondary KPI strip → usage trend
// (full-width) → model distribution + token composition (2-up) → request log
// (full-width). Cost leads because it's VaultOne's first metric (ADR-0009);
// tokens / cache-hit / avg-turn / request·turn serve as supporting KPIs.
//
// Reads the Local Store with a 30s polling fallback (ADR-0005 event-driven
// refresh also invalidates via providers).

import { SlidersHorizontal } from "lucide-react"
import { useMemo, useState } from "react"

import { Button } from "@/components/ui/button"
import type { UsageFilter } from "@/types/generated/bindings"

import { CostHero } from "./cost-hero"
import { FilterSheet } from "./filter-sheet"
import { FilterSummary } from "./filter-summary"
import { KpiStrip } from "./kpi-strip"
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
  const [sheetOpen, setSheetOpen] = useState(false)
  const usageFilter = useMemo(() => toFilter(filter), [filter])

  const clearKey = (key: keyof FilterState) =>
    setFilter((f) => ({ ...f, [key]: "" }))

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <QuickFilters filter={filter} onChange={setFilter} />
        <Button variant="outline" size="sm" onClick={() => setSheetOpen(true)}>
          <SlidersHorizontal />
          高级筛选
        </Button>
      </div>

      <FilterSummary filter={filter} onClear={clearKey} />

      <CostHero filter={usageFilter} />

      <KpiStrip filter={usageFilter} />

      <UsageTrendChart filter={usageFilter} />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <ModelDistribution
          filter={usageFilter}
          onPickModel={(m) => setFilter((f) => ({ ...f, model: m }))}
        />
        <TokenBreakdownBar filter={usageFilter} />
      </div>

      <RequestLogTable filter={usageFilter} />

      <FilterSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        filter={filter}
        onChange={setFilter}
      />
    </div>
  )
}
