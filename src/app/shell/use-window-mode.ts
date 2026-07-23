// Morph the OS main window between full dashboard and lightweight glance card
// (ADR-0015). Coarse full ⇄ lightweight transition only:
//   - full → lightweight: remember the prior geometry (in LOGICAL units) and
//     raise always-on-top.
//   - lightweight → full: clear always-on-top and restore the saved geometry.
//
// This effect NO LONGER sizes, moves, or docks the window. The lightweight
// window's geometry is owned entirely by the Rust `dock_window_right` command,
// invoked from useLightweightTuck (on mount, tuck, expand, and height-resize) —
// one atomic SetWindowPos of the OUTER rect that keeps the window wholly on one
// monitor (shadow included), so it can't straddle two monitors of different DPI
// or lock WebView2 to the wrong rasterization scale.
//
// Geometry is saved/restored in LOGICAL units (physical ÷ the monitor's
// scaleFactor at save time) so the restore shares one coordinate system.
//
// This is a dep-change effect in App (not a mount effect), so it is a single
// invoke per transition.

import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window"
import { useEffect, useRef } from "react"

import { useAppSelector } from "@/app/store/hooks"

type SavedGeometry =
  | { maximized: true }
  | { maximized: false; size: LogicalSize; position: LogicalPosition }

export function useWindowMode() {
  const mode = useAppSelector((s) => s.view.mode)
  const saved = useRef<SavedGeometry | null>(null)

  useEffect(() => {
    const appWindow = getCurrentWindow()
    void (async () => {
      if (mode === "lightweight") {
        // Remember the full-mode geometry for restore. Docking/sizing is now
        // done atomically by dock_window_right (from useLightweightTuck).
        const wasMaximized = await appWindow.isMaximized()
        if (wasMaximized) {
          saved.current = { maximized: true }
        } else {
          const mon = await currentMonitor()
          const f = mon?.scaleFactor || 1
          const outer = await appWindow.outerSize()
          const pos = await appWindow.outerPosition()
          saved.current = {
            maximized: false,
            size: new LogicalSize(outer.width / f, outer.height / f),
            position: new LogicalPosition(pos.x / f, pos.y / f),
          }
        }
        await appWindow.setAlwaysOnTop(true)
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
