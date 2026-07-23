// Shared geometry + edge math for the lightweight glance window (ADR-0015).
//
// The actual docking is done in Rust (dock_window_right): one atomic
// SetWindowPos of the OUTER rect (shadow included), with the monitor picked by
// Windows' "largest intersection area" rule. That fixes the three symptoms at
// once — the shadow no longer overshoots the monitor edge (was ~1/5 on the
// neighbour), there is no intermediate [new pos, old size] state to flip
// MonitorFromWindow, and the window stays wholly on one monitor so WebView2
// renders at that monitor's DPI. This file keeps the JS-side constants and the
// drag-to-edge detection only.

import {
  availableMonitors,
  currentMonitor,
  getCurrentWindow,
  type Monitor,
} from "@tauri-apps/api/window"

import { commands } from "@/types/generated/bindings"

/** Expanded glance card width (fixed). Height adapts to the content. */
export const CARD_WIDTH = 288
/** Initial height guess before the content is measured; replaced on mount. */
export const CARD_HEIGHT_DEFAULT = 360
/** Tucked "half-icon" — 48×48 with a size-10 (40px) logo → ~4px inset. */
export const TUCKED_WIDTH = 48
export const TUCKED_HEIGHT = 48
/** A move whose right edge is within this many logical px of the monitor's
 * right edge counts as "docked" → tuck. (Right edge only for v1.) */
export const EDGE_THRESHOLD = 12
/** Logical px from the top where the glance card docks on entry. */
export const ENTRY_DOCK_Y = 48
/** How far the OUTER rect is kept inside the monitor edge (passed to the Rust
 * dock so the full outer rect — shadow included — never touches the A/B edge). */
export const INSET = 2

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

/** Right edge of the window's current monitor, in logical px (null if unknown).
 * Used by the drag-to-edge auto-tuck test. */
export async function rightEdgeLogical(): Promise<number | null> {
  const mon = await monitorForWindow()
  if (!mon) return null
  return (mon.position.x + mon.size.width) / (mon.scaleFactor || 1)
}

/** Dock the window flush-right via the Rust command (one atomic SetWindowPos of
 *  the OUTER rect, monitor picked by Windows' largest-intersection rule). Passes
 *  INSET so the full outer rect (shadow included) stays inside one monitor.
 *  Returns the clamped logical y to remember, or null on failure. */
export async function dockRight(
  clientLogicalW: number,
  clientLogicalH: number,
  logicalY: number,
): Promise<number | null> {
  const r = await commands.dockWindowRight(
    clientLogicalW,
    clientLogicalH,
    logicalY,
    INSET,
  )
  if ("error" in r) return null
  return r.data
}
