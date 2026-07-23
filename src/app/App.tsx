// Root app: providers + shell + view switch (ADR-0011). View routing via a
// Record keyed by ViewId — adding a view is one entry, no ternary chain.
// Lightweight mode (ADR-0015): when mode === "lightweight" the same window
// drops the Shell and renders the glance card instead; the OS window itself
// (size / always-on-top) is morphed by useWindowMode, mounted here in App.

import type { ComponentType } from "react"
import { useAppSelector } from "@/app/store/hooks"
import type { ViewId } from "@/app/store/slices/viewSlice"
import { PricingView } from "@/features/pricing/components/pricing-view"
import { SettingsView } from "@/features/settings/components/settings-view"
import { DashboardView } from "@/features/usage/components/dashboard-view"
import { LightweightCard } from "@/features/usage/components/lightweight-card"
import { LogsView } from "@/features/usage/components/logs-view"
import { Shell } from "./shell/shell"
import { useUpdateCheck } from "./shell/use-update-check"
import { useWindowMode } from "./shell/use-window-mode"

const VIEWS: Record<ViewId, ComponentType> = {
  dashboard: DashboardView,
  logs: LogsView,
  pricing: PricingView,
  settings: SettingsView,
}

export default function App() {
  // Morph the OS window to match the mode (ADR-0015). Mounted in App so it is
  // always under the Redux store, regardless of which skin renders below.
  useWindowMode()
  // Startup update probe (ADR-0017): fires once app-wide via the hook's guard,
  // regardless of full vs lightweight skin — lightweight just doesn't render
  // the indicator.
  useUpdateCheck()
  const mode = useAppSelector((s) => s.view.mode)
  const view = useAppSelector((s) => s.view.view)

  // Same window, two skins (ADR-0015): lightweight drops the Shell entirely.
  if (mode === "lightweight") return <LightweightCard />

  const Active = VIEWS[view]
  return (
    <Shell>
      <Active />
    </Shell>
  )
}
