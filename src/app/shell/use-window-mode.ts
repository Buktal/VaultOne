// Morph the OS main window between full dashboard and lightweight glance card
// (ADR-0015). Owns the coarse full ⇄ lightweight transition only:
//   - full → lightweight: save the prior geometry, un-maximize, set
//     always-on-top, shrink to the card and dock it at the screen's right edge.
//   - lightweight → full: clear always-on-top and restore the saved geometry.
// The in-lightweight tuck/hover (card ⇄ half-icon) is owned by useLightweightTuck.
//
// This is a dep-change effect in App (not a mount effect), so it is a single
// invoke per transition. The tuck hook lives in LightweightCard and does no
// window ops on mount, so the full-geometry read below happens before anything
// shrinks (no child/parent effect race).

import { useEffect, useRef } from "react"
import {
  getCurrentWindow,
  LogicalPosition,
  type PhysicalPosition,
  type PhysicalSize,
} from "@tauri-apps/api/window"

import { useAppSelector } from "@/app/store/hooks"
import { CARD_SIZE, ENTRY_DOCK_Y, rightEdgeLogical } from "./lightweight-geometry"

type SavedGeometry =
  | { maximized: true }
  | { maximized: false; size: PhysicalSize; position: PhysicalPosition }

export function useWindowMode() {
  const mode = useAppSelector((s) => s.view.mode)
  const saved = useRef<SavedGeometry | null>(null)

  useEffect(() => {
    const appWindow = getCurrentWindow()
    void (async () => {
      if (mode === "lightweight") {
        // Save the full-mode geometry before shrinking. A maximized window is
        // restored via maximize() (a size copy would lose the "maximized"
        // window state).
        const wasMaximized = await appWindow.isMaximized()
        saved.current = wasMaximized
          ? { maximized: true }
          : {
              maximized: false,
              size: await appWindow.outerSize(),
              position: await appWindow.outerPosition(),
            }
        if (wasMaximized) await appWindow.unmaximize()
        await appWindow.setAlwaysOnTop(true)
        await appWindow.setSize(CARD_SIZE)
        // Dock to the right edge (v1: right only; multi-monitor choice deferred).
        const edge = await rightEdgeLogical()
        if (edge != null) {
          await appWindow.setPosition(
            new LogicalPosition(edge - CARD_SIZE.width, ENTRY_DOCK_Y),
          )
        }
      } else {
        await appWindow.setAlwaysOnTop(false)
        const geo = saved.current
        if (geo?.maximized) {
          await appWindow.maximize()
        } else if (geo) {
          // Size first, then position, to land exactly where it was.
          await appWindow.setSize(geo.size)
          await appWindow.setPosition(geo.position)
        }
        saved.current = null
      }
    })().catch(() => {})
  }, [mode])
}
