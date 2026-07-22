// App shell (ADR-0011 / ADR-0013): collapsible sidebar nav + scrollable content.
// View switching via viewSlice (no react-router); the active view is rendered
// by App. 顶栏 (CommandBar) 已移除 — 筛选/采集收敛进各 view 的 ControlCard /
// ControlBar，主题切换移入左下角设备卡片 (ADR-0013 v3，统一入口)，视图标题由导航
// 选中态表达。Sidebar collapse persists to localStorage. 左栏视觉对齐原型 v10
// (递减三色 mark / 绿字灰底选中 / 设备 pill)，main 区去掉 max-w 让看板与日志
// 在宽屏铺满贴边 (窄内容如 settings 各自内部 max-w 居中)。

import {
  Gauge,
  List,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  Tags,
} from "lucide-react"
import { useEffect, useState } from "react"
import { useAppInfoQuery } from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { setView, type ViewId } from "@/app/store/slices/viewSlice"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { TitleBar } from "./title-bar"

const NAV: Array<{ id: ViewId; label: string; icon: typeof Gauge }> = [
  { id: "dashboard", label: "数据看板", icon: Gauge },
  { id: "logs", label: "请求日志", icon: List },
  { id: "pricing", label: "成本定价", icon: Tags },
  { id: "settings", label: "设置", icon: Settings },
]

const COLLAPSE_KEY = "vaultone:sidebar-collapsed"

// Logo: the VaultOne mark as a contrast badge — the dark sidebar gets the
// cream badge, the light sidebar gets the cool-ink badge, so the mark
// always stands off its surface. Same [] + 1 mark as the app/tray icon.
function Logo({ collapsed }: { collapsed: boolean }) {
  return (
    <div className="flex items-center gap-2.5">
      <img
        src="/vaultone-cream.svg"
        alt=""
        className="hidden dark:block size-6 shrink-0"
      />
      <img
        src="/vaultone.svg"
        alt=""
        className="block dark:hidden size-6 shrink-0"
      />
      {collapsed ? null : (
        <div className="flex flex-col leading-tight">
          <span className="text-sm font-semibold">VaultOne</span>
          <span className="text-muted-foreground text-[10px]">用量监控</span>
        </div>
      )}
    </div>
  )
}

function NavItem({
  item,
  active,
  collapsed,
  onClick,
}: {
  item: { id: ViewId; label: string; icon: typeof Gauge }
  active: boolean
  collapsed: boolean
  onClick: () => void
}) {
  const Icon = item.icon
  const button = (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex items-center rounded-lg text-sm transition-colors",
        collapsed ? "size-9 justify-center" : "w-full gap-2.5 px-3 py-2",
        active
          ? "bg-accent-tint font-medium text-accent-brand-strong shadow-[inset_2px_0_0_var(--accent-brand)]"
          : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
      )}
    >
      <Icon className="size-4 shrink-0" />
      {collapsed ? null : item.label}
    </button>
  )
  if (!collapsed) return button
  return (
    <Tooltip>
      <TooltipTrigger render={button} />
      <TooltipContent side="right">{item.label}</TooltipContent>
    </Tooltip>
  )
}

export function Shell({ children }: { children: React.ReactNode }) {
  const dispatch = useAppDispatch()
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useAppInfoQuery(undefined, { pollingInterval: 0 })
  const synced = info?.mode === "synced"

  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage === "undefined") return false
    return localStorage.getItem(COLLAPSE_KEY) === "1"
  })
  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0")
  }, [collapsed])

  return (
    <div className="bg-background text-foreground flex h-screen w-screen flex-col overflow-hidden">
      <TitleBar />
      <div className="flex min-h-0 flex-1 items-stretch gap-4 overflow-hidden pb-4 pl-4">
        <aside
          className={cn(
            "bg-card border-border flex shrink-0 flex-col rounded-2xl border transition-[width] duration-200",
            collapsed ? "w-16" : "w-56",
          )}
        >
          <div
            className={cn(
              "flex items-center",
              collapsed
                ? "flex-col gap-2 px-2 py-3"
                : "justify-between px-4 py-4",
            )}
          >
            <Logo collapsed={collapsed} />
            <Button
              variant="ghost"
              size="icon"
              className="text-muted-foreground size-7"
              onClick={() => setCollapsed((c) => !c)}
              aria-label={collapsed ? "展开菜单" : "收起菜单"}
            >
              {collapsed ? (
                <PanelLeftOpen className="size-4" />
              ) : (
                <PanelLeftClose className="size-4" />
              )}
            </Button>
          </div>

          <Separator />

          {collapsed ? null : (
            <div className="text-muted-foreground/60 px-5 pt-3 pb-1 text-[10px] font-medium tracking-wide">
              导航
            </div>
          )}
          <nav
            className={cn(
              "flex flex-col gap-1.5",
              collapsed ? "items-center p-2" : "p-2",
            )}
          >
            {NAV.map((item) => (
              <NavItem
                key={item.id}
                item={item}
                active={view === item.id}
                collapsed={collapsed}
                onClick={() => dispatch(setView(item.id))}
              />
            ))}
          </nav>

          <div className="mt-auto p-3">
            <Separator className="mb-3" />
            {collapsed ? (
              <div className="flex flex-col items-center gap-2">
                <ThemeToggle />
                <span
                  className={cn(
                    "size-2 rounded-full",
                    synced ? "bg-primary" : "bg-muted-foreground/40",
                  )}
                  title={`${synced ? "已同步" : "单机"} · ${info?.display_name || "未命名"}`}
                />
              </div>
            ) : (
              <div className="flex flex-col gap-1 px-1 text-xs">
                <div className="text-muted-foreground/60 text-[10px] tracking-wide">
                  本机设备
                </div>
                <div className="truncate">
                  <span className="text-muted-foreground">设备名：</span>
                  <span className="font-medium">
                    {info?.display_name || "未命名"}
                  </span>
                </div>
                <div className="text-muted-foreground truncate">
                  <span>设备号：</span>
                  <span className="font-mono text-[11px]">
                    {info?.device_id ?? "—"}
                  </span>
                </div>
                <div className="mt-1 flex items-center gap-2">
                  <span
                    className={cn(
                      "inline-flex items-center rounded-full px-2.5 py-0.5 text-[11px] font-medium",
                      synced
                        ? "bg-muted text-primary"
                        : "bg-muted text-muted-foreground",
                    )}
                  >
                    {synced ? "已同步" : "单机"}
                  </span>
                  {info?.version ? (
                    <span className="text-muted-foreground text-[10px]">
                      v{info.version}
                    </span>
                  ) : null}
                </div>
                <div className="mt-2 flex items-center justify-between border-border/60 border-t pt-2">
                  <span className="text-muted-foreground text-[11px]">
                    主题
                  </span>
                  <ThemeToggle />
                </div>
              </div>
            )}
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex-1 overflow-auto">
            <div className="w-full pt-4 pr-4">{children}</div>
          </div>
        </main>
      </div>
    </div>
  )
}
