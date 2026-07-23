// Lightweight glance card state (ADR-0018). The lightweight window has two
// sub-shapes, both docked flush-right via the Rust `dock_window_right` command
// (one atomic SetWindowPos of the OUTER rect — see lightweight-geometry.ts):
//   - expanded: the 5-field today card (CARD_WIDTH × measured content height)
//   - tucked:   the mini-bar docked flush at the right edge (TUCKED_W × H)
//
// The phase lives in the store (viewSlice.lightweightPhase) so the full-mode
// title bar can enter lightweight directly as either sub-shape (→中 expanded /
// →小 tucked), and tuck/expand are plain dispatches. This hook reads that
// phase, re-docks whenever it changes (mount included), and reports the
// measured expanded height back.
//
// Transitions are EXPLICIT ONLY — no auto-tuck. The earlier "drag to edge" /
// "mouse off card" auto-tucks were the flicker / DPI / loop bug source
// (ADR-0018): the auto-detect → SetWindowPos → onMoved loop.
//
// Dragging either shape moves it; the Y is remembered so the next dock keeps
// it. The dock runs only on phase change / height resize — the card does NOT
// auto-snap back to the edge on drag, so the user can park it.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { useCallback, useEffect, useRef } from "react"

import { useAppDispatch, useAppSelector } from "@/app/store/hooks"
import { setLightweightPhase } from "@/app/store/slices/viewSlice"

import {
  CARD_HEIGHT_DEFAULT,
  CARD_WIDTH,
  dockRight,
  ENTRY_DOCK_Y,
  INSET_EXPANDED,
  INSET_TUCKED,
  monitorForWindow,
  TUCKED_HEIGHT,
  TUCKED_WIDTH,
} from "./lightweight-geometry"

const appWindow = getCurrentWindow()

export type TuckPhase = "tucked" | "expanded"

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
  // Remember the user's vertical position so tuck/expand keep it.
  const lastY = useRef(ENTRY_DOCK_Y)
  // Expanded card height adapts to the content; tucked is fixed.
  const cardHeight = useRef(CARD_HEIGHT_DEFAULT)

  useEffect(() => {
    const t = window.setTimeout(() => {
      settling.current = false
    }, 400)
    return () => window.clearTimeout(t)
  }, [])

  const applyShape = useCallback(async (wantTucked: boolean) => {
    programmatic.current = true
    const logicalW = wantTucked ? TUCKED_WIDTH : CARD_WIDTH
    const logicalH = wantTucked ? TUCKED_HEIGHT : cardHeight.current
    // Tucked flush-edges (inset 0); expanded keeps a small breathing gap (2).
    const inset = wantTucked ? INSET_TUCKED : INSET_EXPANDED
    const y = await dockRight(logicalW, logicalH, lastY.current, inset)
    if (y != null) lastY.current = y
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

  // Dragging either shape moves it; remember the Y so the next dock keeps it.
  // No auto-tuck, no re-dock on drag — the card stays where it's dropped until
  // the next explicit phase change.
  useEffect(() => {
    const unlisten = appWindow.onMoved(({ payload }) => {
      if (programmatic.current || settling.current) return
      void (async () => {
        const mon = await monitorForWindow()
        const f = mon?.scaleFactor || 1
        lastY.current = payload.y / f
      })()
    })
    return () => {
      void unlisten.then((u) => u())
    }
  }, [])

  return { phase, expand, tuck, setCardHeight }
}
