// 灵动岛 tuck/hover for the lightweight glance card (ADR-0015). Within
// lightweight mode the window has two sub-shapes:
//   - expanded: the today card (CARD_WIDTH × measured content height)
//   - tucked:   a half-icon docked at the screen's right edge (48×48)
//
// Dragging the expanded card to the right edge auto-tucks it; hovering the
// half-icon expands it; moving the mouse off the expanded card tucks it again
// (debounced so a stray edge-jitter doesn't collapse it).
//
// All shape changes go through the Rust `dock_window_right` command (one atomic
// SetWindowPos of the OUTER rect) — see lightweight-geometry.ts for why. The
// expand/collapse fade (tw-animate-css) hides the size jump behind invisible
// content; a re-enter mid-collapse cancels it and replays the enter animation.
//
// The first dock happens in a mount effect here (useWindowMode no longer sizes
// or docks the window). It is idempotent, so React StrictMode's dev
// double-invoke is harmless, and SetWindowPos implicitly restores a maximized
// window. A short "settling" window after mount ignores the onMoved burst from
// the entry dock so the card stays expanded on entry.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { useCallback, useEffect, useRef, useState } from "react"

import {
  CARD_HEIGHT_DEFAULT,
  CARD_WIDTH,
  dockRight,
  EDGE_THRESHOLD,
  ENTRY_DOCK_Y,
  INSET_EXPANDED,
  INSET_TUCKED,
  monitorForWindow,
  rightEdgeLogical,
  TUCKED_HEIGHT,
  TUCKED_WIDTH,
} from "./lightweight-geometry"

const appWindow = getCurrentWindow()

/** Hover-leave debounce before the card starts collapsing. */
const LEAVE_DEBOUNCE_MS = 250
/** Duration of the collapse reveal-out; the shrink to the half-icon runs after. */
const LEAVE_FADE_MS = 200

export type TuckPhase = "tucked" | "expanded" | "leaving"

export function useLightweightTuck() {
  // Entry is the expanded card; it stays expanded on entry for discoverability.
  const [phase, setPhase] = useState<TuckPhase>("expanded")
  // Mirror phase in a ref so setCardHeight reads the live value without being a
  // stale-closure dependency.
  const phaseRef = useRef<TuckPhase>("expanded")
  phaseRef.current = phase
  // Ignore onMoved we caused ourselves: programmatic docking can fire onMoved
  // on Windows, which would re-evaluate the edge and re-tuck on expand.
  const programmatic = useRef(false)
  // Ignore the onMoved burst right after mount (the entry dock).
  const settling = useRef(true)
  // Remember the user's vertical position so tuck/expand keep it.
  const lastY = useRef(ENTRY_DOCK_Y)
  // Expanded card height adapts to the content (measured by LightweightCard);
  // tucked is fixed. Width is fixed at CARD_WIDTH.
  const cardHeight = useRef(CARD_HEIGHT_DEFAULT)
  const leaveDebounce = useRef<number | null>(null)
  const leaveEnd = useRef<number | null>(null)

  useEffect(() => {
    const t = window.setTimeout(() => {
      settling.current = false
    }, 400)
    return () => window.clearTimeout(t)
  }, [])

  useEffect(() => {
    return () => {
      if (leaveDebounce.current != null)
        window.clearTimeout(leaveDebounce.current)
      if (leaveEnd.current != null) window.clearTimeout(leaveEnd.current)
    }
  }, [])

  const applyShape = useCallback(async (wantTucked: boolean) => {
    programmatic.current = true
    const logicalW = wantTucked ? TUCKED_WIDTH : CARD_WIDTH
    const logicalH = wantTucked ? TUCKED_HEIGHT : cardHeight.current
    // Tucked flush-edges (inset 0); expanded keeps a small breathing gap (2).
    const inset = wantTucked ? INSET_TUCKED : INSET_EXPANDED
    // Atomic Rust dock: sets the OUTER rect (shadow included) on the monitor
    // Windows considers the window to be on, in one SetWindowPos.
    const y = await dockRight(logicalW, logicalH, lastY.current, inset)
    if (y != null) lastY.current = y
    window.setTimeout(() => {
      programmatic.current = false
    }, 150)
  }, [])

  // First dock on mount (useWindowMode no longer sizes/docks).
  useEffect(() => {
    void applyShape(false).catch(() => {})
  }, [applyShape])

  const tuck = useCallback(() => {
    // Reveal the card out (clip wipes back to the top-right) first; only after
    // it does the window shrink to the half-icon, so the size jump is hidden
    // behind the already-invisible content.
    setPhase("leaving")
    if (leaveEnd.current != null) window.clearTimeout(leaveEnd.current)
    leaveEnd.current = window.setTimeout(() => {
      leaveEnd.current = null
      setPhase("tucked")
      void applyShape(true).catch(() => {})
    }, LEAVE_FADE_MS)
  }, [applyShape])

  const expand = useCallback(() => {
    if (leaveDebounce.current != null) {
      window.clearTimeout(leaveDebounce.current)
      leaveDebounce.current = null
    }
    if (leaveEnd.current != null) {
      window.clearTimeout(leaveEnd.current)
      leaveEnd.current = null
    }
    setPhase("expanded")
    void applyShape(false).catch(() => {})
  }, [applyShape])

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

  // Auto-tuck when the user drags the window to the right edge.
  useEffect(() => {
    const unlisten = appWindow.onMoved(({ payload }) => {
      if (programmatic.current || settling.current) return
      void (async () => {
        const mon = await monitorForWindow()
        const f = mon?.scaleFactor || 1
        lastY.current = payload.y / f
        const edge = await rightEdgeLogical()
        if (edge == null) return
        const phys = await appWindow.outerSize()
        const xRight = payload.x / f + phys.width / f
        if (xRight >= edge - EDGE_THRESHOLD) tuck()
      })()
    })
    return () => {
      void unlisten.then((u) => u())
    }
  }, [tuck])

  return {
    phase,
    expand,
    setCardHeight,
    /** Schedule a tuck; cancelled by re-entering the card (debounced leave). */
    scheduleTuck: () => {
      if (leaveDebounce.current != null) {
        window.clearTimeout(leaveDebounce.current)
      }
      leaveDebounce.current = window.setTimeout(() => tuck(), LEAVE_DEBOUNCE_MS)
    },
    /** Re-entering the card cancels a pending leave and, if the collapse fade
     * was already running, returns to expanded (replaying the enter animation). */
    cancelTuck: () => {
      if (leaveDebounce.current != null) {
        window.clearTimeout(leaveDebounce.current)
        leaveDebounce.current = null
      }
      if (leaveEnd.current != null) {
        window.clearTimeout(leaveEnd.current)
        leaveEnd.current = null
      }
      setPhase((p) => (p === "leaving" ? "expanded" : p))
    },
  }
}
