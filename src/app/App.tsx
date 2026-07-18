// Root app: providers + shell + view switch (ADR-0007).

import { useAppSelector } from "@/app/store/hooks"
import { PricingView } from "@/features/pricing/components/pricing-view"
import { SettingsView } from "@/features/settings/components/settings-view"
import { DashboardView } from "@/features/usage/components/dashboard-view"
import { Shell } from "./shell/shell"

export default function App() {
  const view = useAppSelector((s) => s.view.view)

  return (
    <Shell>
      {view === "dashboard" ? <DashboardView /> : null}
      {view === "pricing" ? <PricingView /> : null}
      {view === "settings" ? <SettingsView /> : null}
    </Shell>
  )
}
