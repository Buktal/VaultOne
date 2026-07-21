// Root app: providers + shell + view switch (ADR-0011). View routing via a
// Record keyed by ViewId — adding a view is one entry, no ternary chain.

import type { ComponentType } from "react"
import { useAppSelector } from "@/app/store/hooks"
import type { ViewId } from "@/app/store/slices/viewSlice"
import { PricingView } from "@/features/pricing/components/pricing-view"
import { SettingsView } from "@/features/settings/components/settings-view"
import { DashboardView } from "@/features/usage/components/dashboard-view"
import { LogsView } from "@/features/usage/components/logs-view"
import { Shell } from "./shell/shell"

const VIEWS: Record<ViewId, ComponentType> = {
  dashboard: DashboardView,
  logs: LogsView,
  pricing: PricingView,
  settings: SettingsView,
}

export default function App() {
  const view = useAppSelector((s) => s.view.view)
  const Active = VIEWS[view]
  return (
    <Shell>
      <Active />
    </Shell>
  )
}
