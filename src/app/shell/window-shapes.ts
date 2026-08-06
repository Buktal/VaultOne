// Window-shape size constants and the full-mode minimum clamp, in one place.
//
// The OS main window has three shapes, each with a canonical logical-px size:
//
//   - full:        the whole dashboard. `DEFAULT_SIZE` is the first-entry size
//                  (kept in sync with tauri.conf.json); `MIN_FULL` is the floor
//                  it can never shrink below (drag constraint + restore clamp +
//                  record guard, via the helpers below). Distinct from the two
//                  lightweight widths so the large window can't end up "as
//                  small as the small window".
//   - expanded:    the right-docked 5-field glance card. Width = `CARD_WIDTH`,
//                  height adapts to content (initial guess = CARD_HEIGHT_DEFAULT).
//   - tucked:      the right-docked mini-bar. `TUCKED_WIDTH` × `TUCKED_HEIGHT`.
//
// Positioning / docking constants (ENTRY_DOCK_Y, INSET_*) and the docking math
// stay in lightweight-geometry.ts — this file owns only the size definitions.

/** Default full-mode window size (logical px), kept in sync with tauri.conf.json.
 *  Used only on the very first entry, before the user has shaped full mode. */
export const DEFAULT_SIZE = { w: 920, h: 680 }

/** Minimum full-mode client size (logical px). Enforced as an OS min-size in
 *  full mode, and via `clampToMinFull` / `meetsMinFull` on restore + record. */
export const MIN_FULL = { w: 840, h: 600 }

/** Expanded glance-card window width. The reused TokenHero card sits inside a
 *  p-3 inset so its rounded corners clear the square window edge, so the card
 *  renders a touch narrower than this. Height adapts to content. */
export const CARD_WIDTH = 320
/** Initial expanded-height guess before content is measured; replaced on mount. */
export const CARD_HEIGHT_DEFAULT = 360

/** Tucked mini-bar window — [grip][number][→大]. Width fits the longest compact
 *  token plus grip + →大 + spacing; height is a thin strip. */
export const TUCKED_WIDTH = 120
export const TUCKED_HEIGHT = 40

/** Clamp a logical w/h up to the full-mode minimum, so a stale or corrupted
 *  small record never restores the dashboard undersized. */
export function clampToMinFull(w: number, h: number): { w: number; h: number } {
  return { w: Math.max(w, MIN_FULL.w), h: Math.max(h, MIN_FULL.h) }
}

/** Whether a logical w/h meets the full-mode minimum. Used to refuse recording
 *  a degenerate (sub-minimum) size as the full-mode rect — it would re-restore
 *  undersized. */
export function meetsMinFull(w: number, h: number): boolean {
  return w >= MIN_FULL.w && h >= MIN_FULL.h
}
