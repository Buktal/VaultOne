// App shell (ADR-0007): sidebar nav + status footer. View switching via
// viewSlice (no react-router). The active view is rendered by App.

import { Gauge, Settings, Tags } from "lucide-react"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { setView, type ViewId } from "@/app/store/slices/viewSlice"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { useGetAppInfoQuery } from "@/features/settings/api"

const NAV: Array<{ id: ViewId; label: string; icon: typeof Gauge }> = [
  { id: "dashboard", label: "数据看板", icon: Gauge },
  { id: "pricing", label: "成本定价", icon: Tags },
  { id: "settings", label: "设置", icon: Settings },
]

export function Shell({ children }: { children: React.ReactNode }) {
  const dispatch = useAppDispatch()
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useGetAppInfoQuery(undefined, { pollingInterval: 0 })

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar">
        <div className="flex items-center gap-2 px-5 py-5">
          <div className="bg-primary text-primary-foreground flex size-8 items-center justify-center rounded-md font-bold">
            V
          </div>
          <div className="flex flex-col">
            <span className="text-sm font-semibold">VaultOne</span>
            <span className="text-muted-foreground text-[10px]">
              LLM Usage Dashboard
            </span>
          </div>
        </div>

        <Separator />

        <nav className="flex flex-col gap-1 p-3">
          {NAV.map((item) => {
            const Icon = item.icon
            const active = view === item.id
            return (
              <Button
                key={item.id}
                variant={active ? "secondary" : "ghost"}
                className="justify-start"
                onClick={() => dispatch(setView(item.id))}
              >
                <Icon className="size-4" />
                {item.label}
              </Button>
            )
          })}
        </nav>

        <div className="mt-auto p-3">
          <Separator className="mb-3" />
          <div className="flex flex-col gap-1 px-2 text-xs">
            <div className="text-muted-foreground">设备</div>
            <div className="truncate font-mono">{info?.device_id ?? "—"}</div>
            <div className="mt-2">
              <Badge
                variant={info?.mode === "synced" ? "default" : "secondary"}
              >
                {info?.mode === "synced" ? "Synced" : "Standalone"}
              </Badge>
            </div>
          </div>
        </div>
      </aside>

      <main className="flex-1 overflow-auto">
        <div className="mx-auto max-w-7xl p-6">{children}</div>
      </main>
    </div>
  )
}
