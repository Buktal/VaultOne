// Lightweight glance card (ADR-0018): the same main window morphs into a small,
// always-on-top, right-edge-docked "today" snapshot. Two sub-shapes (both
// docked flush-right via the Rust `dock_window_right` command — one atomic
// SetWindowPos of the OUTER rect; see lightweight-geometry.ts):
//   - tucked: a mini-bar that ALWAYS shows today's token total — the "glance"
//     value. Layout [grip(drag)][number(click→中)][→大]. Tauri's drag
//     region swallows clicks (tauri#9751/#9901), so the drag grip and the
//     clickable number/expand are SIBLINGS, never nested.
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

import dayjs from "dayjs"
import { Airplay, AlignHorizontalJustifyEnd, GripVertical } from "lucide-react"
import { useEffect, useMemo, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useLightweightTuck } from "@/app/shell/use-lightweight-tuck"
import { usePreferencesQuery, useStatsQuery, ZERO_STATS } from "@/app/store/api"
import { useAppDispatch } from "@/app/store/hooks"
import { toFilter } from "@/app/store/slices/filterSlice"
import { setMode } from "@/app/store/slices/viewSlice"
import { formatTokens } from "@/lib/format"

import { TokenHero } from "./token-hero"

export function LightweightCard() {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { phase, expand, tuck, setCardHeight } = useLightweightTuck()
  // Hover-to-expand is opt-in (ADR-0018): the default is click. When hover is
  // chosen, the tucked number area also expands on mouse-enter.
  const { data: prefs } = usePreferencesQuery()
  const hoverExpand = prefs?.lightweight_expand === "hover"

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

  // Tucked mini-bar: [grip(drag)] [number(click→中)] [→大]. The three cells
  // have breathing room (px-1 + gap-1), and the grip / →大 hover tiles are
  // rounded with a vertical inset (my-0.5) so they don't kiss the window edge
  // on hover. The grip is the ONLY data-tauri-drag-region — drag and click
  // stay siblings (Tauri's drag region swallows clicks on itself and its
  // children, tauri#9751/#9901).
  if (phase === "tucked") {
    return (
      <div className="bg-background flex h-screen w-screen animate-in fade-in slide-in-from-right-2 items-stretch gap-1 overflow-hidden px-1 duration-150 motion-reduce:animate-none">
        <div
          data-tauri-drag-region
          aria-hidden
          className="hover:bg-muted flex w-4 shrink-0 cursor-grab items-center justify-center rounded-md my-0.5"
        >
          <GripVertical className="text-muted-foreground size-3" />
        </div>
        <button
          type="button"
          onMouseEnter={hoverExpand ? expand : undefined}
          onClick={expand}
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
          onClick={() => dispatch(setMode("full"))}
          className="text-muted-foreground hover:bg-muted hover:text-foreground inline-flex w-6 shrink-0 items-center justify-center rounded-md my-0.5"
        >
          <Airplay className="size-3.5" />
        </button>
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
      <div className="p-3">
        <TokenHero filter={todayFilter} />
      </div>
    </div>
  )
}
