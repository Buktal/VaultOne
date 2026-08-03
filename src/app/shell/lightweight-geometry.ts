// Shared geometry for the lightweight glance window.
//
// The actual docking is done in Rust (dock_window_right): one atomic
// SetWindowPos of the OUTER rect (shadow included), with the monitor picked by
// Windows' "largest intersection area" rule. That fixes the three symptoms at
// once — the shadow no longer overshoots the monitor edge (was ~1/5 on the
// neighbour), there is no intermediate [new pos, old size] state to flip
// MonitorFromWindow, and the window stays wholly on one monitor so WebView2
// renders at that monitor's DPI. This file keeps the docking math and the
// positioning/inset constants; window-shape SIZES live in window-shapes.ts.

import {
  availableMonitors,
  currentMonitor,
  getCurrentWindow,
  type Monitor,
} from "@tauri-apps/api/window"

// Window-shape SIZES (CARD_WIDTH, CARD_HEIGHT_DEFAULT, TUCKED_WIDTH,
// TUCKED_HEIGHT) live in window-shapes.ts; this file keeps only the docking
// math and the positioning/inset constants.
import { commands } from "@/types/generated/bindings"

/** Logical px from the top where the glance card docks on entry. */
export const ENTRY_DOCK_Y = 48
/** How far the OUTER rect is kept inside the monitor edge (passed to the Rust
 * dock so the full outer rect — shadow included — never crosses the A/B edge).
 * Tucked flush-edges (the half-icon kisses the right edge); expanded keeps a
 * small breathing gap so the card doesn't visually merge with the bezel. */
export const INSET_TUCKED = 0
export const INSET_EXPANDED = 2

/** The monitor the window's CENTER is on (best-effort, for the drag-to-edge
 *  test). The authoritative pick for docking lives in Rust (MonitorFromWindow). */
export async function monitorForWindow(): Promise<Monitor | null> {
  try {
    const appWindow = getCurrentWindow()
    const pos = await appWindow.outerPosition()
    const size = await appWindow.outerSize()
    const cx = pos.x + size.width / 2
    const cy = pos.y + size.height / 2
    const monitors = await availableMonitors()
    const hit = monitors.find(
      (m) =>
        cx >= m.position.x &&
        cx < m.position.x + m.size.width &&
        cy >= m.position.y &&
        cy < m.position.y + m.size.height,
    )
    if (hit) return hit
  } catch {
    // fall through to currentMonitor()
  }
  return (await currentMonitor()) ?? null
}

/** Dock the window flush-right via the Rust command (one atomic SetWindowPos of
 *  the OUTER rect, monitor picked by Windows' largest-intersection rule). The
 *  caller picks the inset: tucked flush-edges (0), expanded keeps a gap (2) so
 *  the full outer rect (shadow included) stays inside one monitor.
 *  Returns the clamped logical y to remember, or null on failure. */
export async function dockRight(
  clientLogicalW: number,
  clientLogicalH: number,
  logicalY: number,
  inset: number,
): Promise<number | null> {
  const r = await commands.dockWindowRight(
    clientLogicalW,
    clientLogicalH,
    logicalY,
    inset,
  )
  if ("error" in r) return null
  return r.data
}

/** Center the window on its current monitor at the given CLIENT size, via one
 *  atomic Rust SetWindowPos (size + position together). Avoids the
 *  `[new size, old pos]` straddle of separate setSize + setPosition, which on a
 *  multi-monitor setup of mixed DPI flips MonitorFromWindow and locks WebView2
 *  to the wrong rasterization scale (content renders too small). No-op on
 *  failure. */
export async function centerWindow(
  clientLogicalW: number,
  clientLogicalH: number,
): Promise<void> {
  const r = await commands.centerWindow(clientLogicalW, clientLogicalH)
  if ("error" in r) return
}

/** Restore the window to an arbitrary logical rect via one atomic Rust
 *  SetWindowPos (size + position together). Used to bring the full-mode window
 *  back to the exact position + size the user last left it. Same atomic-write
 *  rationale as dockRight/centerWindow — no `[new size, old pos]` straddle that
 *  would flip MonitorFromWindow and lock WebView2 to the wrong DPI. The outer
 *  rect is clamped to the current monitor in Rust, so an out-of-range stored
 *  position lands on-screen. No-op on failure. */
export async function setWindowRect(
  clientLogicalX: number,
  clientLogicalY: number,
  clientLogicalW: number,
  clientLogicalH: number,
): Promise<void> {
  const r = await commands.setWindowRect(
    clientLogicalX,
    clientLogicalY,
    clientLogicalW,
    clientLogicalH,
  )
  if ("error" in r) return
}
