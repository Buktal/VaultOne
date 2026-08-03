// Lightweight glance card state. The lightweight window has two
// sub-shapes, both docked flush-right via the Rust `dock_window_right` command
// (one atomic SetWindowPos of the OUTER rect — see lightweight-geometry.ts):
//   - expanded: the 5-field today card (CARD_WIDTH × measured content height)
//   - tucked:   the mini-bar docked flush at the right edge (TUCKED_W × H;
//                H grows on hover to reveal a device-picker drawer, see
//                `setTuckDrawer` below)
//
// The phase lives in the store (viewSlice.lightweightPhase) so the full-mode
// title bar can enter lightweight directly as either sub-shape (→中 expanded /
// →小 tucked), and tuck/expand are plain dispatches. This hook reads that
// phase, re-docks whenever it changes (mount included), and reports the
// measured expanded height back.
//
// Transitions are EXPLICIT ONLY — no auto-tuck. The earlier "drag to edge" /
// "mouse off card" auto-tucks were the flicker / DPI / loop bug source
// — the auto-detect → SetWindowPos → onMoved loop.
//
// Dragging either shape moves it; the Y is remembered so the next dock keeps
// it. The dock runs only on phase change / height resize — the card does NOT
// auto-snap back to the edge on drag, so the user can park it.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { useCallback, useEffect, useRef } from "react"

import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { setLightweightPhase } from "@/app/store/slices/viewSlice"
import { flushPendingWrites } from "@/lib/persistence"

import {
  dockRight,
  INSET_EXPANDED,
  INSET_TUCKED,
  monitorForWindow,
} from "./lightweight-geometry"
import { readLwY, saveLwY } from "./window-geometry"
import {
  CARD_HEIGHT_DEFAULT,
  CARD_WIDTH,
  TUCKED_HEIGHT,
  TUCKED_WIDTH,
} from "./window-shapes"

const appWindow = getCurrentWindow()

export type TuckPhase = "tucked" | "expanded"

// Per-phase dock Y lives in the shared window-geometry store (one key for all
// three shapes) alongside the full-mode rect — see window-geometry.ts. That
// shared store is what makes the position survive the hook unmounting when the
// app flips to full mode, and app restarts.

export function useLightweightTuck() {
  const dispatch = useAppDispatch()
  // Phase is store-driven (viewSlice.lightweightPhase): the full-mode title bar
  // sets it on entry, and tuck/expand dispatch it.
  const phase = useAppSelector((s) => s.view.lightweightPhase)
  // Mirror phase in a ref so callbacks read the live value without a stale
  // closure dependency.
  const phaseRef = useRef<TuckPhase>(phase)
  phaseRef.current = phase
  // Ignore onMoved we caused ourselves: programmatic docking fires onMoved.
  const programmatic = useRef(false)
  // Ignore the onMoved burst right after mount (the entry dock).
  const settling = useRef(true)
  // Expanded card height adapts to the content; tucked is fixed.
  const cardHeight = useRef(CARD_HEIGHT_DEFAULT)
  // Tucked hover-drawer extra height (0 when closed): the mini-bar grows
  // downward to reveal a device picker (device_scope in the small window).
  // Only applies in tucked; expanded ignores it.
  const drawerExtra = useRef(0)

  useEffect(() => {
    const t = window.setTimeout(() => {
      settling.current = false
    }, 400)
    return () => window.clearTimeout(t)
  }, [])

  const applyShape = useCallback(async (wantTucked: boolean) => {
    programmatic.current = true
    const logicalW = wantTucked ? TUCKED_WIDTH : CARD_WIDTH
    const logicalH = wantTucked
      ? TUCKED_HEIGHT + drawerExtra.current
      : cardHeight.current
    // Tucked flush-edges (inset 0); expanded keeps a small breathing gap (2).
    const inset = wantTucked ? INSET_TUCKED : INSET_EXPANDED
    // Y is persisted per phase in window-geometry.ts so it survives hook
    // unmount + restart; dockRight returns the clamped Y actually applied.
    const phaseKey: TuckPhase = wantTucked ? "tucked" : "expanded"
    const y = await dockRight(logicalW, logicalH, readLwY(phaseKey), inset)
    if (y != null) saveLwY(phaseKey, y)
    window.setTimeout(() => {
      programmatic.current = false
    }, 150)
  }, [])

  // (Re)dock on mount and whenever the phase flips expanded ⇄ tucked.
  useEffect(() => {
    void applyShape(phase === "tucked").catch(() => {})
  }, [phase, applyShape])

  const tuck = useCallback(
    () => dispatch(setLightweightPhase("tucked")),
    [dispatch],
  )
  const expand = useCallback(
    () => dispatch(setLightweightPhase("expanded")),
    [dispatch],
  )

  /** Called by LightweightCard with the measured content height; resizes the
   *  expanded window to fit. Skipped for sub-2px jitter and when not expanded. */
  const setCardHeight = useCallback(
    (h: number) => {
      if (Math.abs(h - cardHeight.current) < 2) return
      cardHeight.current = h
      if (phaseRef.current === "expanded")
        void applyShape(false).catch(() => {})
    },
    [applyShape],
  )

  /** Grow (or shrink) the tucked mini-bar to reveal the hover device drawer.
   *  `extra` is the drawer height in logical px (0 closes). No-op unless tucked.
   *  Driven by explicit mouse enter/leave — never mouse position — so it does
   *  not retrigger the SetWindowPos ⇄ onMoved loop. */
  const setTuckDrawer = useCallback(
    (extra: number) => {
      const next = Math.max(0, Math.round(extra))
      if (Math.abs(next - drawerExtra.current) < 2) return
      drawerExtra.current = next
      if (phaseRef.current === "tucked") void applyShape(true).catch(() => {})
    },
    [applyShape],
  )

  // Dragging either shape moves it; remember the Y so the next dock keeps it.
  // No auto-tuck, no re-dock on drag — the card stays where it's dropped until
  // the next explicit phase change.
  useEffect(() => {
    const unlisten = appWindow.onMoved(({ payload }) => {
      if (programmatic.current || settling.current) return
      void (async () => {
        const mon = await monitorForWindow()
        const f = mon?.scaleFactor || 1
        // Persist the dragged Y for the current phase so the next explicit
        // tuck/expand (or a restart) re-docks here instead of back at the top.
        saveLwY(phaseRef.current, payload.y / f)
      })()
    })
    return () => {
      void unlisten.then((u) => u())
      // Land the trailing debounced geometry write so unmounting the card (a
      // full-mode switch) right after a drag can't drop the last Y.
      flushPendingWrites()
    }
  }, [])

  return { phase, expand, tuck, setCardHeight, setTuckDrawer }
}
