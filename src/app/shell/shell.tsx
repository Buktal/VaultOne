// App shell (ADR-0011 / ADR-0013): collapsible sidebar nav + scrollable content.
// View switching via viewSlice (no react-router); the active view is rendered
// by App. 顶栏 (CommandBar) 已移除 — 筛选/采集收敛进各 view 的 ControlCard /
// ControlBar，主题切换与折叠 toggle 统一收进左下角 footer 控制台 (ADR-0013 v3，
// 统一入口)，顶部仅留 logo 作品牌锚点，视图标题由导航
// 选中态表达。Sidebar collapse persists to localStorage. 左栏视觉对齐原型 v10
// (递减三色 mark / 绿字灰底选中 / 设备 pill)，main 区去掉 max-w 让看板与日志
// 在宽屏铺满贴边 (窄内容如 settings 各自内部 max-w 居中)。

import {
  BookText,
  Gauge,
  List,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  Tags,
} from "lucide-react"
import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
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
import { UpdateIndicator } from "./update-card"
import { useUpdateCheck } from "./use-update-check"

const NAV: Array<{ id: ViewId; key: string; icon: typeof Gauge }> = [
  { id: "dashboard", key: "nav.dashboard", icon: Gauge },
  { id: "logs", key: "nav.logs", icon: List },
  { id: "pricing", key: "nav.pricing", icon: Tags },
  { id: "settings", key: "nav.settings", icon: Settings },
]

const COLLAPSE_KEY = "vaultone:sidebar-collapsed"

// Logo: the radial "One" mark — dark sidebar gets the cream badge, light
// sidebar gets the ink badge, so the mark always stands off its surface.
// Same radial mark as the app/tray icon (vaultone-cream / vaultone-ink).
function Logo({ collapsed }: { collapsed: boolean }) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center gap-2.5">
      <img
        src="/vaultone-cream.svg"
        alt=""
        className="hidden dark:block size-9 shrink-0"
      />
      <img
        src="/vaultone-ink.svg"
        alt=""
        className="block dark:hidden size-9 shrink-0"
      />
      {collapsed ? null : (
        <div className="flex flex-col leading-tight">
          <span className="text-sm font-semibold">VaultOne</span>
          <span className="text-muted-foreground text-[10px]">
            {t("shell.logoSubtitle")}
          </span>
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
  item: { id: ViewId; key: string; icon: typeof Gauge }
  active: boolean
  collapsed: boolean
  onClick: () => void
}) {
  const { t } = useTranslation()
  const Icon = item.icon
  const label = t(item.key)
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
      <Icon className={cn("shrink-0", collapsed ? "size-5" : "size-4")} />
      {collapsed ? null : label}
    </button>
  )
  if (!collapsed) return button
  return (
    <Tooltip>
      <TooltipTrigger render={button} />
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  )
}

// Collapse toggle — lives in the footer control deck next to the theme toggle
// and device status, keeping the top of the sidebar a clean brand anchor
// (logo only). Mirrors ThemeToggle's icon-button + tooltip treatment.
function CollapseButton({
  collapsed,
  onClick,
}: {
  collapsed: boolean
  onClick: () => void
}) {
  const { t } = useTranslation()
  const Icon = collapsed ? PanelLeftOpen : PanelLeftClose
  const label = collapsed ? t("shell.expandMenu") : t("shell.collapseMenu")
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground"
            onClick={onClick}
            aria-label={label}
          />
        }
      >
        <Icon className="size-4" />
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  )
}

export function Shell({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const view = useAppSelector((s) => s.view.view)
  const { data: info } = useAppInfoQuery(undefined, { pollingInterval: 0 })
  const synced = info?.mode === "synced"
  const { openReleases } = useUpdateCheck()

  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage === "undefined") return false
    return localStorage.getItem(COLLAPSE_KEY) === "1"
  })
  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0")
  }, [collapsed])

  const modeLabel = t(synced ? "shell.synced" : "shell.standalone")
  const deviceName = info?.display_name || t("common.unnamed")

  return (
    <div className="bg-background text-foreground flex h-screen w-screen flex-col overflow-hidden">
      <TitleBar />
      <div className="flex min-h-0 flex-1 items-stretch gap-4 overflow-hidden pb-4 pl-4">
        <aside
          className={cn(
            "bg-card border-border flex shrink-0 flex-col rounded-2xl border transition-[width] duration-200",
            collapsed ? "w-16" : "w-52",
          )}
        >
          <div
            className={cn(
              "flex items-center",
              collapsed ? "justify-center px-2 py-4" : "px-4 py-4",
            )}
          >
            <Logo collapsed={collapsed} />
          </div>

          <Separator />

          {collapsed ? null : (
            <div className="text-muted-foreground/60 px-5 pt-3 pb-1 text-[10px] font-medium tracking-wide">
              {t("nav.heading")}
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
              <div className="flex flex-col items-center">
                <span
                  className={cn(
                    "mb-5 size-2 rounded-full",
                    synced ? "bg-primary" : "bg-muted-foreground/40",
                  )}
                  title={`${modeLabel} · ${deviceName}`}
                />
                <div className="flex flex-col items-center gap-2">
                  <ThemeToggle />
                  <CollapseButton
                    collapsed={collapsed}
                    onClick={() => setCollapsed((c) => !c)}
                  />
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-1 px-1 text-xs">
                <div className="text-muted-foreground/60 text-[10px] tracking-wide">
                  {t("shell.thisDevice")}
                </div>
                <div className="truncate">
                  <span className="text-muted-foreground">
                    {t("shell.deviceName")}
                  </span>
                  <span className="font-medium">{deviceName}</span>
                </div>
                <div className="text-muted-foreground truncate">
                  <span>{t("shell.deviceId")}</span>
                  <span className="font-mono text-[11px]">
                    {info?.device_id ?? "—"}
                  </span>
                </div>
                <div className="mt-1 flex items-center justify-between gap-2">
                  <span
                    className={cn(
                      "inline-flex items-center rounded-full px-2.5 py-0.5 text-[11px] font-medium",
                      synced
                        ? "bg-muted text-primary"
                        : "bg-muted text-muted-foreground",
                    )}
                  >
                    {modeLabel}
                  </span>
                  <div className="flex items-center gap-1.5">
                    <Tooltip>
                      <TooltipTrigger
                        render={
                          <button
                            type="button"
                            onClick={() => void openReleases()}
                            aria-label={t("shell.changelog")}
                            className="text-muted-foreground hover:text-foreground inline-flex size-3.5 items-center justify-center transition-colors"
                          />
                        }
                      >
                        <BookText className="size-3.5" />
                      </TooltipTrigger>
                      <TooltipContent side="right">
                        {t("shell.changelogGithub")}
                      </TooltipContent>
                    </Tooltip>
                    {info?.version ? (
                      <span className="text-muted-foreground text-[10px]">
                        v{info.version}
                      </span>
                    ) : null}
                    <UpdateIndicator />
                  </div>
                </div>
                <div className="mt-2 flex items-center justify-between border-border/60 border-t pt-2">
                  <ThemeToggle />
                  <CollapseButton
                    collapsed={collapsed}
                    onClick={() => setCollapsed((c) => !c)}
                  />
                </div>
              </div>
            )}
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex-1 overflow-auto">
            <div className="w-full pr-4">{children}</div>
          </div>
        </main>
      </div>
    </div>
  )
}
