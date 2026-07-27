// Lightweight glance card (ADR-0018): the same main window morphs into a small,
// always-on-top, right-edge-docked "today" snapshot. Two sub-shapes (both
// docked flush-right via the Rust `dock_window_right` command — one atomic
// SetWindowPos of the OUTER rect; see lightweight-geometry.ts):
//   - tucked: a mini-bar that ALWAYS shows today's token total — the "glance"
//     value. Layout [number][→大]. The whole bar drags via startDragging() on
//     the root (a JS call, NOT data-tauri-drag-region, so it doesn't swallow the
//     number's click): a press that moves >4px starts a window drag, a press
//     that doesn't is a click → expand. →大 stops propagation so it stays a
//     pure click.
//   - expanded: a 1:1 reuse of the dashboard's right-column anchor (TokenHero,
//     ADR-0011) fed today's filter — the "中窗口" mirrors the 右中 card exactly,
//     only adding a drag/title bar with expand + shrink controls.
//
// Three "windows", each reachable from the others: full ⇄ expanded ⇄ tucked,
// plus tucked → full directly via its [→大] button. Phase is store-driven
// (viewSlice.lightweightPhase); this card just renders it.
//
// Icon language (per target shape, consistent across windows): →tucked =
// AlignHorizontalJustifyEnd (a strip pinned to the right edge); →full = Airplay
// (cast to the big screen). →中 keeps PictureInPicture2 in the title bar.
//
// Button ORDER everywhere is target-size descending (大→中→小): each window
// lists its switch targets biggest-first. So the expanded title bar is
// [全→大][缩→小], not the reverse.
//
// Data: tucked reads total_tokens from a useStatsQuery(todayFilter). Expanded
// reuses <TokenHero filter={todayFilter}/> — which runs its own stats + trend
// queries — so the snapshot is identical to the dashboard from one source.
// Refresh is free: providers.tsx invalidates the Usage tags on every
// `usage_changed`, and the filter matches the dashboard's "today" preset.

import { getCurrentWindow } from "@tauri-apps/api/window"
import dayjs from "dayjs"
import { Airplay, AlignHorizontalJustifyEnd, ChevronDown } from "lucide-react"
import { type MouseEvent, useEffect, useMemo, useRef, useState } from "react"
import { useTranslation } from "react-i18next"
import { useLightweightTuck } from "@/app/shell/use-lightweight-tuck"
import {
  useDevicesQuery,
  usePreferencesQuery,
  useStatsQuery,
  ZERO_STATS,
} from "@/app/store/api"
import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { patchFilter, toFilter } from "@/app/store/slices/filterSlice"
import { setMode } from "@/app/store/slices/viewSlice"
import { formatTokens } from "@/lib/format"
import { cn } from "@/lib/utils"

import { DeviceScopeControl } from "./device-scope-control"
import { TokenHero } from "./token-hero"

const appWindow = getCurrentWindow()
/** Square of the move distance (CSS px) that distinguishes a drag from a click
 *  on the tucked bar. 4px — small enough to feel instant, large enough that a
 *  click's natural jitter doesn't start a drag. */
const DRAG_THRESHOLD_SQ = 16

export function LightweightCard() {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { phase, expand, tuck, setCardHeight, setTuckDrawer } =
    useLightweightTuck()
  // Tucked hover-drawer (two-step): hover the mini-bar → a Select-like trigger
  // appears directly below it (full-width, top border as the divider — not a
  // floating card inset); clicking the trigger opens the device list.
  // device_scope in the small window. Disabled when hover-expand is chosen
  // (hover then means "go to the mid window") or with ≤1 device.
  const [drawerHover, setDrawerHover] = useState(false)
  const [listOpen, setListOpen] = useState(false)
  // Hover-leave 关闭延迟: tucked 窗口 resize (抽屉展开/收起) 时鼠标可能瞬间越过
  // 窗口边界 → mouseleave → 立即收起 → 鼠标又回窗口 → mouseenter → 再展开, 形成抖动
  // 循环。延迟 180ms 关闭, 期间 mouseenter 取消定时器, 打破循环。
  const leaveTimer = useRef<number | undefined>(undefined)

  // Whole-bar drag for the tucked mini-bar. startDragging() is a JS call, not
  // data-tauri-drag-region, so it does NOT swallow the number's click — a press
  // that moves > DRAG_THRESHOLD starts a window drag, a press that doesn't is a
  // plain click → expand. →大 stops propagation to stay a pure click. This is
  // why the whole bar (number + gutters) is draggable, not just a tiny grip.
  const dragArmed = useRef(false)
  const dragStart = useRef({ x: 0, y: 0 })
  const dragged = useRef(false)
  const armDrag = (e: MouseEvent) => {
    if (e.button !== 0) return
    dragArmed.current = true
    dragged.current = false
    dragStart.current = { x: e.screenX, y: e.screenY }
  }
  const maybeDrag = (e: MouseEvent) => {
    if (!dragArmed.current || dragged.current) return
    const dx = e.screenX - dragStart.current.x
    const dy = e.screenY - dragStart.current.y
    if (dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
      dragged.current = true
      dragArmed.current = false
      void appWindow.startDragging()
    }
  }
  const disarm = () => {
    dragArmed.current = false
  }
  // Hover-to-expand is opt-in (ADR-0018): the default is click. When hover is
  // chosen, the tucked number area also expands on mouse-enter.
  const { data: prefs } = usePreferencesQuery()
  const hoverExpand = prefs?.lightweight_expand === "hover"

  // 今日 · device_scope 跟随全局 (统一控制: 大窗口选了某设备，中/小窗今日快照
  // 也是该设备)。仍固定"今日"范围——只并入设备维度，不并 model/日期 (中/小窗恒
  // 为今日快照)。reuses toFilter (local-day → UTC timestamp bounds) 与看板"今天"
  // preset 同口径; local day 翻页或 device_scope 变更时重算。
  const today = dayjs().format("YYYY-MM-DD")
  const deviceScope = useAppSelector((s) => s.filter.filter.device_scope)
  // 设备列表 — 仅用于 expanded 卡内设备分段的显隐 (单设备不渲染)。缓存与
  // dashboard / DeviceScopeControl 共享，无额外请求。
  const { data: devices = [] } = useDevicesQuery()
  // Hover-drawer gating + heights. Two steps: hover slides out a trigger
  // (TRIGGER_H); clicking the trigger opens the list (listH = items × row).
  const drawerEnabled = devices.length > 1 && !hoverExpand
  const TRIGGER_H = 28
  const listH = drawerEnabled ? (devices.length + 1) * 26 + 10 : 0
  const closeDrawer = () => {
    if (leaveTimer.current) {
      window.clearTimeout(leaveTimer.current)
      leaveTimer.current = undefined
    }
    setDrawerHover(false)
    setListOpen(false)
    setTuckDrawer(0)
  }
  const openDrawer = () => {
    if (leaveTimer.current) {
      window.clearTimeout(leaveTimer.current)
      leaveTimer.current = undefined
    }
    if (!drawerEnabled) return
    setDrawerHover(true)
    setTuckDrawer(TRIGGER_H)
  }
  // 延迟关闭 (见 leaveTimer 注释): 取代 onMouseLeave 内的立即 closeDrawer, 避开
  // resize 越界导致的 leave → close → enter → open 抖动循环。
  const scheduleClose = () => {
    disarm()
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current)
    leaveTimer.current = window.setTimeout(closeDrawer, 180)
  }
  const toggleList = () => {
    const next = !listOpen
    setListOpen(next)
    setTuckDrawer(next ? TRIGGER_H + listH : TRIGGER_H)
  }
  // Reset the drawer when leaving tucked (e.g. →大 to full): otherwise it
  // would reopen the next time the mini-bar shows.
  useEffect(() => {
    if (phase !== "tucked") {
      if (leaveTimer.current) {
        window.clearTimeout(leaveTimer.current)
        leaveTimer.current = undefined
      }
      setDrawerHover(false)
      setListOpen(false)
      setTuckDrawer(0)
    }
  }, [phase, setTuckDrawer])
  const todayFilter = useMemo(
    () =>
      toFilter({
        from_day: today,
        to_day: today,
        model: "",
        source: "",
        device_scope: deviceScope,
      }),
    [today, deviceScope],
  )

  // tucked reads total_tokens here; expanded reuses <TokenHero> which runs its
  // own queries. ZERO_STATS keeps the first paint sane before data lands.
  const { data: stats } = useStatsQuery(todayFilter)
  const s = stats ?? ZERO_STATS

  // Measure the expanded card's natural height and tell the hook, so the
  // window shrinks to fit the content. Tucked is a fixed mini-bar, so skip it.
  const rootRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (phase === "tucked") return
    const el = rootRef.current
    if (!el) return
    const measure = () => {
      const h = Math.ceil(el.getBoundingClientRect().height)
      if (h > 0) setCardHeight(h)
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [phase, setCardHeight])

  // Tucked mini-bar: [number] [→大]. The whole bar drags via startDragging() on
  // the root (see armDrag/maybeDrag above) — not data-tauri-drag-region, so the
  // number stays clickable. number is flex-1 (the big drag/click target); →大
  // stops propagation so a press on it never starts a drag.
  if (phase === "tucked") {
    // 本机写「本机」(与 DeviceScopeControl 一致), 对端显示 display_name。
    const items = [
      { id: "", label: t("usage.control.all") },
      ...devices.map((d) => ({
        id: d.device_id,
        label: d.is_self
          ? t("devices.thisDevice")
          : d.display_name || t("common.unnamed"),
      })),
    ]
    return (
      // biome-ignore lint/a11y/noStaticElementInteractions: Tauri window drag handle + hover drawer — mouse-only startDragging with no keyboard equivalent; keyboard users reach the same actions via the inner buttons.
      <div
        onMouseDown={armDrag}
        onMouseMove={maybeDrag}
        onMouseUp={disarm}
        onMouseEnter={openDrawer}
        onMouseLeave={scheduleClose}
        className="bg-background flex h-screen w-screen flex-col animate-in fade-in slide-in-from-right-2 cursor-grab overflow-hidden duration-150 motion-reduce:animate-none"
      >
        {/* h-10 = TUCKED_HEIGHT (40px): 固定占满 tucked 高度 + items-center 让
            数字/→大 垂直居中 (不再置顶)。显式 bg-background 满铺数字条, 不靠
            外层透出。 */}
        <div className="bg-background relative z-10 flex h-10 shrink-0 items-center gap-1 px-1">
          <button
            type="button"
            onMouseEnter={hoverExpand ? expand : undefined}
            onClick={() => {
              if (!dragged.current) expand()
            }}
            aria-label={t("usage.lightweight.expandToday")}
            className="flex flex-1 cursor-pointer items-center justify-center border-0 bg-transparent p-0"
          >
            <span className="font-semibold tabular-nums text-base leading-none">
              {formatTokens(s.total_tokens)}
            </span>
          </button>
          <button
            type="button"
            aria-label={t("usage.lightweight.expandFull")}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => dispatch(setMode("full"))}
            className="text-muted-foreground hover:bg-muted hover:text-foreground inline-flex w-6 shrink-0 items-center justify-center rounded-md my-0.5"
          >
            <Airplay className="size-3.5" />
          </button>
        </div>
        {drawerHover && drawerEnabled ? (
          <button
            type="button"
            aria-label={t("usage.deviceScope.label")}
            aria-expanded={listOpen}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={toggleList}
            className="border-border bg-background text-foreground hover:bg-muted/60 flex w-full shrink-0 items-center justify-between border-t px-2 py-1 text-[11px] transition-colors"
          >
            <span className="min-w-0 flex-1 truncate">
              {deviceScope
                ? items.find((it) => it.id === deviceScope)?.label ||
                  t("common.unnamed")
                : t("usage.control.all")}
            </span>
            <ChevronDown
              className={cn(
                "text-muted-foreground size-3 shrink-0 transition-transform",
                listOpen && "rotate-180",
              )}
            />
          </button>
        ) : null}
        {drawerHover && listOpen && drawerEnabled ? (
          <fieldset
            aria-label={t("devices.currentDevice")}
            className="bg-background m-0 flex w-full min-w-0 shrink-0 flex-col gap-0.5 p-0"
          >
            {items.map((it) => {
              const selected = deviceScope === it.id
              return (
                <button
                  key={it.id || "all"}
                  type="button"
                  aria-pressed={selected}
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={() => {
                    dispatch(patchFilter({ device_scope: it.id }))
                    closeDrawer()
                  }}
                  className={cn(
                    "focus-visible:ring-ring/40 flex w-full items-center rounded-none px-2 py-1 text-[11px] outline-none transition-colors focus-visible:ring-2",
                    selected
                      ? "bg-accent-tint text-accent-brand-strong"
                      : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{it.label}</span>
                </button>
              )
            })}
          </fieldset>
        ) : null}
      </div>
    )
  }

  return (
    <div
      ref={rootRef}
      role="dialog"
      aria-label={t("usage.lightweight.todayGlance")}
      className="bg-background text-foreground lw-reveal-in flex w-screen flex-col overflow-hidden"
    >
      {/* Drag region + two actions, ordered 大→小 (biggest target first): expand
          to full, then shrink to tucked. The buttons have no
          data-tauri-drag-region so they stay clickable inside the drag bar.
          Airplay = cast to the full dashboard; AlignHorizontalJustifyEnd = the
          right-pinned mini-bar that shrink lands on. */}
      <div
        data-tauri-drag-region
        className="text-muted-foreground flex h-8 shrink-0 items-center justify-between ps-3 pe-1 text-xs select-none"
      >
        <span data-tauri-drag-region>{t("usage.lightweight.header")}</span>
        <div className="flex items-center">
          <button
            type="button"
            aria-label={t("usage.lightweight.expandFull")}
            onClick={() => dispatch(setMode("full"))}
            className="text-muted-foreground hover:bg-muted hover:text-foreground inline-flex size-7 items-center justify-center rounded-md transition-colors"
          >
            <Airplay className="size-3.5" />
          </button>
          <button
            type="button"
            aria-label={t("usage.lightweight.tuck")}
            onClick={tuck}
            className="text-muted-foreground hover:bg-muted hover:text-foreground inline-flex size-7 items-center justify-center rounded-md transition-colors"
          >
            <AlignHorizontalJustifyEnd className="size-3.5" />
          </button>
        </div>
      </div>

      {/* The dashboard's 右中 card, unchanged. p-3 insets it off the window's
          square edge so the card's rounded corners don't sit flush against a
          square window border — the full dashboard gives the same card the
          same breathing room via the main-area padding/gap. */}
      {/* 设备视角切换 — drag-bar 下方右对齐。selector 顶部留 mt-4(16) 呼吸, 与
          大窗口 dashboard selector 行同间距 → 两窗口 selector 离窗口顶都是 32+16
          =48, 完全对齐, 且与下方使用趋势的 gap-4 节奏一致 (不再紧贴顶部栏显偏
          上)。px-3 右缩进与 TokenHero 卡右边平齐。单设备不渲染。 */}
      {devices.length > 1 ? (
        <div className="flex justify-end px-3 mt-4">
          <DeviceScopeControl compact />
        </div>
      ) : null}
      {/* TokenHero 卡保留 p-3 呼吸 (圆角不贴窗口边); 其 pt-3 同时给出与上方
          selector 行的间距。 */}
      <div className="p-3">
        <TokenHero filter={todayFilter} />
      </div>
    </div>
  )
}
