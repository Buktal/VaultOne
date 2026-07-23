// Morph the OS main window between full dashboard and lightweight glance card
// (ADR-0015). Coarse full ⇄ lightweight transition only — it does NOT size,
// move, or dock the lightweight window. That geometry is owned entirely by the
// Rust `dock_window_right` command, invoked from useLightweightTuck (on mount,
// tuck, expand, and height-resize) — one atomic SetWindowPos of the OUTER rect
// that keeps the window wholly on one monitor (shadow included), so it can't
// straddle two monitors of different DPI or lock WebView2 to the wrong scale.
//
//   - full → lightweight: raise always-on-top, drop off the taskbar/Alt+Tab
//     (the glance is an edge-docked tool window, not an app you switch to), and
//     lock user resizing (the card self-sizes via dock_window_right). The dock
//     itself runs from useLightweightTuck's mount effect, which fires before
//     this parent effect, and its raw SetWindowPos is unaffected by setResizable.
//   - lightweight → full: clear always-on-top, return to the taskbar, re-enable
//     resizing, and land the window at the default 800×600 centered on this
//     monitor. We deliberately do NOT remember/restore the pre-lightweight
//     geometry: by the time we'd read it, the window had already been docked
//     down to the 288-wide card, so "restore" reliably produced a too-small full
//     window. Default size + center is predictable instead.
//
// This is a dep-change effect in App (not a mount effect), so it is one async
// batch per transition.

import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window"
import { useEffect } from "react"

import { centerWindow } from "@/app/shell/lightweight-geometry"
import { useAppSelector } from "@/app/store/hooks"

/** The default full-mode window size, kept in sync with tauri.conf.json. */
const DEFAULT_SIZE = new LogicalSize(800, 600)

export function useWindowMode() {
  const mode = useAppSelector((s) => s.view.mode)

  useEffect(() => {
    const appWindow = getCurrentWindow()
    void (async () => {
      if (mode === "lightweight") {
        await appWindow.setAlwaysOnTop(true)
        // Hide from the taskbar AND Alt+Tab; the tray icon still surfaces it.
        await appWindow.setSkipTaskbar(true)
        // The user must not drag-resize the glance card; its size is driven by
        // dock_window_right. Raw SetWindowPos bypasses this flag, so the card's
        // self-sizing still works.
        await appWindow.setResizable(false)
      } else {
        await appWindow.setAlwaysOnTop(false)
        await appWindow.setSkipTaskbar(false)
        await appWindow.setResizable(true)
        // One atomic SetWindowPos (via the Rust command) sets size + position
        // together, so the window never sits at [new size, old pos] straddling
        // two monitors of different DPI — which would flip MonitorFromWindow
        // and lock WebView2 to the wrong rasterization scale (content renders
        // too small on high-DPI multi-monitor setups). Centers on the window's
        // current monitor (the one the lightweight card was docked on).
        await centerWindow(DEFAULT_SIZE.width, DEFAULT_SIZE.height)
      }
    })().catch(() => {})
  }, [mode])
}
