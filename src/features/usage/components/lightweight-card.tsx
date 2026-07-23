// Lightweight glance card (ADR-0015): the same main window morphed into a
// small, always-on-top, edge-dockable "today" snapshot. The body is the SAME
// <TokenHero> as the dashboard's right column (1:1, today口径), so the glance
// and the dashboard read as one system — no hand-rolled duplicate.
//
// The window height adapts to the content: a ResizeObserver measures the root
// and reports it via setCardHeight, and the hook sizes the window to fit (width
// is fixed at CARD_WIDTH). The single "expand to full" affordance is a small
// icon in the drag bar (mirrors the title-bar CtrlButton treatment); the
// full-mode title bar is unchanged.
//
// Expand/collapse is eased with a fade driven by useLightweightTuck's phase.
// Refresh is free: providers.tsx invalidates the Usage tags on every
// `usage_changed`, and TokenHero's query matches the dashboard's "today".

import dayjs from "dayjs"
import { Maximize2 } from "lucide-react"
import { useEffect, useMemo, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useLightweightTuck } from "@/app/shell/use-lightweight-tuck"
import { useAppDispatch } from "@/app/store/hooks"
import { toFilter } from "@/app/store/slices/filterSlice"
import { setMode } from "@/app/store/slices/viewSlice"
import { TokenHero } from "@/features/usage/components/token-hero"
import { cn } from "@/lib/utils"

export function LightweightCard() {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { phase, expand, setCardHeight, scheduleTuck, cancelTuck } =
    useLightweightTuck()

  // 今日 · 全部设备 — reuses toFilter (local-day → UTC timestamp bounds) so the
  // 口径 is identical to the dashboard's "today" preset. Recomputed when the
  // local day rolls over (dep on `today`), not every render.
  const today = dayjs().format("YYYY-MM-DD")
  const todayFilter = useMemo(
    () =>
      toFilter({
        from_day: today,
        to_day: today,
        model: "",
        source: "",
        device_scope: "",
      }),
    [today],
  )

  // Measure the card's natural height and tell the hook, so the window shrinks
  // to fit the content (width stays fixed). Tucked is a fixed 48×48, so skip it.
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

  // Tucked "half-icon" (ADR-0015 灵动岛): a tiny mark docked at the right edge;
  // hover/click expands it back to the card. size-10 (40px) in a 48×48 window
  // → ~4px inset, visually filled (no oversized frame around the logo).
  if (phase === "tucked") {
    return (
      <button
        type="button"
        className="bg-background flex h-screen w-screen cursor-default animate-in fade-in zoom-in-95 items-center justify-center border-0 p-0 duration-150 motion-reduce:animate-none"
        onMouseEnter={expand}
        onClick={expand}
        aria-label={t("usage.lightweight.expandToday")}
      >
        <img
          src="/vaultone-cream.svg"
          alt=""
          className="hidden dark:block size-10"
        />
        <img
          src="/vaultone-ink.svg"
          alt=""
          className="block dark:hidden size-10"
        />
      </button>
    )
  }

  const leaving = phase === "leaving"
  return (
    <div
      // key changes between enter/leave so the fade animation replays even when
      // the same card instance would otherwise stay mounted (e.g. a re-enter
      // mid-collapse). No h-screen: the root sizes to its content and the hook
      // matches the window height to it.
      key={leaving ? "leave" : "in"}
      ref={rootRef}
      role="dialog"
      aria-label={t("usage.lightweight.todayGlance")}
      onMouseEnter={cancelTuck}
      onMouseLeave={scheduleTuck}
      className={cn(
        "bg-background text-foreground flex w-screen flex-col overflow-hidden",
        leaving
          ? "animate-out fade-out slide-out-to-right-1 duration-120"
          : "animate-in fade-in slide-in-from-right-1 duration-150",
        "motion-reduce:animate-none",
      )}
    >
      {/* Drag region + a single expand-to-full icon. The button has no
          data-tauri-drag-region so it stays clickable inside the drag bar. */}
      <div
        data-tauri-drag-region
        className="text-muted-foreground flex h-8 shrink-0 items-center justify-between ps-3 pe-1 text-xs select-none"
      >
        <span data-tauri-drag-region>{t("usage.lightweight.header")}</span>
        <button
          type="button"
          aria-label={t("usage.lightweight.expandFull")}
          onClick={() => dispatch(setMode("full"))}
          className="text-muted-foreground hover:bg-muted hover:text-foreground inline-flex size-7 items-center justify-center rounded-md transition-colors"
        >
          <Maximize2 className="size-3.5" />
        </button>
      </div>

      {/* 1:1 with the dashboard's right-column TokenHero (today口径). p-2 wraps
          it with a little outer padding; height is measured, not fixed. */}
      <div className="p-2">
        <TokenHero filter={todayFilter} />
      </div>
    </div>
  )
}
