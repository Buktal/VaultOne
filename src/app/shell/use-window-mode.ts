// Morph the OS main window between full dashboard and lightweight glance card.
// Coarse full ⇄ lightweight transition only — it does NOT size, move, or dock
// the lightweight window. That geometry is owned entirely by the Rust
// dock_window_right command, invoked from useLightweightTuck (on mount,
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
//     resizing, and RESTORE the full-mode geometry the user last left at —
//     maximized if that's how they left it, otherwise the stored x/y/w/h rect,
//     falling back to a centered default on first-ever entry. Position/state
//     persist across lightweight unmount and restart via window-geometry.ts.
//
// This hook is mounted in App (always under the Redux store, never unmounted
// by a lightweight switch), so the onMoved/onResized listeners it attaches in
// full mode keep recording the user's placement for the whole session.
//
// getCurrentWindow() is fetched lazily inside each effect (not at module top),
// so importing this hook does not blow up a non-Tauri test environment. The OS
// window handle thus stays inside the effect seam, never at module load time.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { useEffect } from "react"

import {
  centerWindow,
  monitorForWindow,
  setWindowRect,
} from "@/app/shell/lightweight-geometry"
import {
  type FullGeom,
  readFull,
  saveFull,
  saveFullMaximized,
  saveFullRect,
} from "@/app/shell/window-geometry"
import { useAppSelector } from "@/app/store/hooks"
import { flushPendingWrites } from "@/lib/persistence"
import { clampToMinFull, DEFAULT_SIZE, meetsMinFull } from "./window-shapes"

export function useWindowMode() {
  const mode = useAppSelector((s) => s.view.mode)

  // Morph always-on-top / taskbar / resizability + restore full geometry.
  useEffect(() => {
    void (async () => {
      const appWindow = getCurrentWindow()
      if (mode === "lightweight") {
        await appWindow.setAlwaysOnTop(true)
        // Hide from the taskbar AND Alt+Tab; the tray icon still surfaces it.
        await appWindow.setSkipTaskbar(true)
        // The user must not drag-resize the glance card; its size is driven by
        // dock_window_right. Raw SetWindowPos bypasses this flag, so the card's
        // self-sizing still works.
        await appWindow.setResizable(false)
        return
      }
      await appWindow.setAlwaysOnTop(false)
      await appWindow.setSkipTaskbar(false)
      await appWindow.setResizable(true)
      // One atomic SetWindowPos (via the Rust commands) sets size + position
      // together, so the window never sits at [new size, old pos] straddling
      // two monitors of different DPI — which would flip MonitorFromWindow
      // and lock WebView2 to the wrong rasterization scale (content renders
      // too small on high-DPI multi-monitor setups).
      await restoreFullGeometry(appWindow)
      // Establish/refresh the baseline record from the actual on-screen rect so
      // the partial patches recorded below always have a record to hit.
      await snapshotFull(appWindow)
    })().catch(() => {})
  }, [mode])

  // Record the user's full-mode placement so the next entry restores it. Only
  // attached in full mode — lightweight docks fire onMoved/onResized too and
  // must NOT pollute the full-mode record. Maximized state is tracked without
  // touching the stored rect: the rect is always the windowed ("restored")
  // geometry, so maximizing leaves it alone and an unmaximize returns to it.
  useEffect(() => {
    if (mode !== "full") return
    const appWindow = getCurrentWindow()
    let cancelled = false
    let unlistenMoved: (() => void) | null = null
    let unlistenResized: (() => void) | null = null

    const onMoved = appWindow.onMoved(({ payload }) => {
      void (async () => {
        if (await appWindow.isMaximized()) return // rect is the windowed geom
        const mon = await monitorForWindow()
        const f = mon?.scaleFactor || 1
        saveFullRect({ x: payload.x / f, y: payload.y / f })
      })()
    })
    const onResized = appWindow.onResized(() => {
      void (async () => {
        const isMax = await appWindow.isMaximized()
        saveFullMaximized(isMax)
        if (isMax) return // rect is the windowed geom
        const size = await appWindow.outerSize()
        const mon = await monitorForWindow()
        const f = mon?.scaleFactor || 1
        saveFullRect({ w: size.width / f, h: size.height / f })
      })()
    })

    void Promise.all([onMoved, onResized]).then(([u1, u2]) => {
      if (cancelled) {
        u1()
        u2()
      } else {
        unlistenMoved = u1
        unlistenResized = u2
      }
    })

    return () => {
      cancelled = true
      unlistenMoved?.()
      unlistenResized?.()
      // Land the trailing debounced geometry write so closing/unmounting right
      // after a drag/resize can't drop the last on-screen position.
      flushPendingWrites()
    }
  }, [mode])
}

/** Restore the full-mode window: re-maximize if that's how the user left it
 *  (after first seating the restored rect so an unmaximize returns to it),
 *  otherwise land at the stored rect, or center the default size on the very
 *  first entry when nothing is stored yet. */
async function restoreFullGeometry(
  appWindow: ReturnType<typeof getCurrentWindow>,
): Promise<void> {
  const geom: FullGeom | null = readFull()
  if (!geom) {
    await centerWindow(DEFAULT_SIZE.w, DEFAULT_SIZE.h)
    return
  }
  // Clamp the stored rect to the full-mode minimum so a stale / corrupted
  // small record never restores the dashboard undersized.
  const { w, h } = clampToMinFull(geom.w, geom.h)
  if (geom.maximized) {
    await setWindowRect(geom.x, geom.y, w, h)
    await appWindow.maximize()
  } else {
    await setWindowRect(geom.x, geom.y, w, h)
  }
}

/** Write a complete full-mode record from the window's live outer rect + maximize
 *  state. Used on entry to establish the baseline. The rect is only captured
 *  when windowed — a maximized window's outer rect overflows its monitor and is
 *  not the user's "restored" geometry, so we leave the stored rect alone and
 *  only flip the flag. */
async function snapshotFull(
  appWindow: ReturnType<typeof getCurrentWindow>,
): Promise<void> {
  const isMax = await appWindow.isMaximized()
  if (isMax) {
    saveFullMaximized(true)
    return
  }
  const pos = await appWindow.outerPosition()
  const size = await appWindow.outerSize()
  const mon = await monitorForWindow()
  const f = mon?.scaleFactor || 1
  const w = size.width / f
  const h = size.height / f
  // Refuse to record a degenerate (sub-minimum) size as the full-mode rect —
  // it would re-restore undersized. Leave the prior record untouched instead.
  if (!meetsMinFull(w, h)) return
  saveFull({ maximized: false, x: pos.x / f, y: pos.y / f, w, h })
}
