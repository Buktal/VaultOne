// App shell (ADR-0007): sidebar nav + command bar + scrollable content. View
// switching via viewSlice (no react-router); the active view is rendered by
// App. The command bar carries the global collect / sync / theme actions.

import { Gauge, Settings, Tags } from "lucide-react"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { setView, type ViewId } from "@/app/store/slices/viewSlice"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { useGetAppInfoQuery } from "@/features/settings/api"

import { CommandBar } from "./command-bar"

const NAV: Array<{ id: ViewId; label: string; icon: typeof Gauge }> = [
  { id: "dashboard", label: "数据看板", icon: Gauge },
  { id: "pricing", label: "成本定价", icon: Tags },
  { id: "settings", label: "设置", icon: Settings },
]

function NavItem({
  item,
  active,
  onClick,
}: {
  item: { id: ViewId; label: string; icon: typeof Gauge }
  active: boolean
  onClick: () => void
}) {
  const Icon = item.icon
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={`group relative flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors ${
        active
          ? "bg-sidebar-accent text-sidebar-accent-foreground"
          : "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
      }`}
    >
      {active ? (
        <span
          className="bg-sidebar-primary absolute top-1/2 left-0 h-4 -translate-y-1/2 rounded-full"
          style={{ width: 2 }}
        />
      ) : null}
      <Icon className="size-4 shrink-0" />
      {item.label}
    </button>
  )
}

export function Shell({ children }: { children: React.ReactNode }) {
  const dispatch = useAppDispatch()
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useGetAppInfoQuery(undefined, { pollingInterval: 0 })
  const synced = info?.mode === "synced"

  return (
    <div className="bg-background text-foreground flex h-screen w-screen overflow-hidden">
      <aside className="bg-sidebar flex w-56 shrink-0 flex-col">
        <div className="flex items-center gap-2.5 px-4 py-4">
          <div className="bg-primary text-primary-foreground flex size-8 items-center justify-center rounded-md font-bold">
            V
          </div>
          <div className="flex flex-col leading-tight">
            <span className="text-sm font-semibold">VaultOne</span>
            <span className="text-muted-foreground text-[10px]">
              LLM Usage Cockpit
            </span>
          </div>
        </div>

        <Separator />

        <nav className="flex flex-col gap-0.5 p-2">
          {NAV.map((item) => (
            <NavItem
              key={item.id}
              item={item}
              active={view === item.id}
              onClick={() => dispatch(setView(item.id))}
            />
          ))}
        </nav>

        <div className="mt-auto p-3">
          <Separator className="mb-3" />
          <div className="flex flex-col gap-1.5 px-1 text-xs">
            <div className="text-muted-foreground text-[10px] tracking-wide uppercase">
              本机设备
            </div>
            <div className="truncate font-medium">
              {info?.display_name || "未命名"}
            </div>
            <div className="text-muted-foreground truncate font-mono text-[10px]">
              {info?.device_id ?? "—"}
            </div>
            <div className="mt-1 flex items-center gap-2">
              <Badge variant={synced ? "default" : "secondary"}>
                {synced ? "Synced" : "Standalone"}
              </Badge>
              {info?.version ? (
                <span className="text-muted-foreground text-[10px]">
                  v{info.version}
                </span>
              ) : null}
            </div>
          </div>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <CommandBar />
        <div className="flex-1 overflow-auto">
          <div className="mx-auto max-w-7xl p-6">{children}</div>
        </div>
      </main>
    </div>
  )
}
