// Shared geometry + edge math for the lightweight glance window (ADR-0015), so
// the coarse full⇄lightweight morph (useWindowMode) and the in-lightweight
// tuck/hover (useLightweightTuck) never drift on sizes or edge detection.

import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window"

/** Expanded glance card — the full "today" snapshot. */
export const CARD_SIZE = new LogicalSize(288, 388)
/** Tucked "half-icon" — just the radial mark, docked at the right edge. */
export const TUCKED_SIZE = new LogicalSize(56, 56)
/** A move whose right edge is within this many logical px of the monitor's
 * right edge counts as "docked" → tuck. (Right edge only for v1; multi-monitor
 * edge choice is a deferred ADR-0015 detail.) */
export const EDGE_THRESHOLD = 12
/** Logical px from the top where the glance card docks on entry. */
export const ENTRY_DOCK_Y = 48

const appWindow = getCurrentWindow()

/** Right edge of the window's current monitor, in logical px (null if unknown). */
export async function rightEdgeLogical(): Promise<number | null> {
  const mon = await appWindow.currentMonitor()
  if (!mon) return null
  const f = mon.scaleFactor || 1
  return (mon.position.x + mon.size.width) / f
}
