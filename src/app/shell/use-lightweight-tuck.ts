// 灵动岛 tuck/hover for the lightweight glance card (ADR-0015). Within
// lightweight mode the window has two sub-shapes:
//   - expanded: the full today card (CARD_SIZE)
//   - tucked:   a half-icon docked at the screen's right edge (TUCKED_SIZE)
//
// Dragging the expanded card to the right edge auto-tucks it; hovering the
// half-icon expands it; moving the mouse off the expanded card tucks it again
// (debounced so a stray edge-jitter doesn't collapse it).
//
// Shape changes are applied imperatively in tuck()/expand() (not in a [tucked]
// effect), so nothing happens on mount — useWindowMode owns the entry size, and
// React StrictMode's dev double-invoke of mount effects can't accidentally
// reposition the window. A short "settling" window after mount also ignores the
// onMoved burst from the entry dock, so the card stays expanded on entry (better
// discoverability) instead of immediately collapsing to the half-icon.
//
// v1 defaults for ADR-0015's deferred "待定实现细节": right edge only, 12px dock
// threshold, 56px half-icon, vertical position preserved, 250ms leave debounce.
// Multi-monitor edge choice and "forgot which edge" discoverability still need
// design — tune after interactive testing.

import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
} from "@tauri-apps/api/window"
import { useCallback, useEffect, useRef, useState } from "react"

import {
  CARD_SIZE,
  EDGE_THRESHOLD,
  ENTRY_DOCK_Y,
  rightEdgeLogical,
  TUCKED_SIZE,
} from "./lightweight-geometry"

const appWindow = getCurrentWindow()

export function useLightweightTuck() {
  const [tucked, setTucked] = useState(false)
  // Ignore onMoved we caused ourselves: programmatic setPosition can fire
  // onMoved on Windows, which would re-evaluate the edge and re-tuck on expand.
  const programmatic = useRef(false)
  // Ignore the onMoved burst right after mount (the entry dock) so the card
  // stays expanded on entry instead of immediately tucking.
  const settling = useRef(true)
  // Remember the user's vertical position so tuck/expand keep it.
  const lastY = useRef(ENTRY_DOCK_Y)
  const leaveTimer = useRef<number | null>(null)

  useEffect(() => {
    const t = window.setTimeout(() => {
      settling.current = false
    }, 400)
    return () => window.clearTimeout(t)
  }, [])

  const applyShape = useCallback(async (wantTucked: boolean) => {
    programmatic.current = true
    const size = wantTucked ? TUCKED_SIZE : CARD_SIZE
    const edge = await rightEdgeLogical()
    // Keep the right edge flush with the monitor (grow/shrink leftward) and
    // preserve the user's vertical position.
    if (edge != null) {
      await appWindow.setPosition(
        new LogicalPosition(edge - size.width, lastY.current),
      )
    }
    await appWindow.setSize(size)
    window.setTimeout(() => {
      programmatic.current = false
    }, 150)
  }, [])

  const tuck = useCallback(() => {
    setTucked(true)
    void applyShape(true).catch(() => {})
  }, [applyShape])

  const expand = useCallback(() => {
    setTucked(false)
    void applyShape(false).catch(() => {})
  }, [applyShape])

  // Auto-tuck when the user drags the window to the right edge.
  useEffect(() => {
    const unlisten = appWindow.onMoved(({ payload }) => {
      if (programmatic.current || settling.current) return
      void (async () => {
        const mon = await currentMonitor()
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
    tucked,
    expand,
    /** Schedule a tuck; cancelled by re-entering the card (debounced leave). */
    scheduleTuck: () => {
      leaveTimer.current = window.setTimeout(() => tuck(), 250)
    },
    cancelTuck: () => {
      if (leaveTimer.current != null) {
        window.clearTimeout(leaveTimer.current)
        leaveTimer.current = null
      }
    },
  }
}
