// Persisted window geometry for all three window shapes, in ONE JSON blob so
// each shape's position/state survives both the lightweight unmount (App.tsx
// drops <LightweightCard/> when mode flips to full) and app restarts. One key
// = one source of truth; was previously scattered across ad-hoc keys.
//
// Shapes:
//   - full:     the full dashboard. Stores the OS "restored" rect (x/y/w/h in
//               logical px) plus whether it was maximized. On re-entry we
//               re-maximize or land at the stored rect. null until the user
//               has shaped full-mode once (first entry falls back to center).
//   - expanded: the right-docked glance card. Only Y varies (X is flush-right).
//   - tucked:   the right-docked mini-bar. Only Y varies.
//
// All values are LOGICAL px. Full-mode x/y are relative to the virtual-screen
// origin (outerPosition / scaleFactor); w/h are outer size / scaleFactor. The
// Rust set_window_rect command converts back to physical on restore.

import { ENTRY_DOCK_Y } from "./lightweight-geometry"

export type FullGeom = {
  maximized: boolean
  x: number
  y: number
  w: number
  h: number
}

export type WindowGeometry = {
  full: FullGeom | null
  expanded: { y: number }
  tucked: { y: number }
}

const KEY = "vaultone:window-geometry"

function defaults(): WindowGeometry {
  return {
    full: null,
    expanded: { y: ENTRY_DOCK_Y },
    tucked: { y: ENTRY_DOCK_Y },
  }
}

function load(): WindowGeometry {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return defaults()
    const p = JSON.parse(raw) as Partial<WindowGeometry>
    return {
      full: p.full ?? null,
      expanded: { y: p.expanded?.y ?? ENTRY_DOCK_Y },
      tucked: { y: p.tucked?.y ?? ENTRY_DOCK_Y },
    }
  } catch {
    return defaults()
  }
}

function save(g: WindowGeometry): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(g))
  } catch {
    // localStorage unavailable (private mode / quota) — geometry just won't
    // persist this session; never crash window placement over it.
  }
}

export function readFull(): FullGeom | null {
  return load().full
}

/** Overwrite the full-mode record wholesale (used by the entry snapshot). */
export function saveFull(full: FullGeom): void {
  const g = load()
  g.full = full
  save(g)
}

/** Patch the restored-window rect (x/y/w/h) leaving `maximized` as-is. No-op
 *  if no full record exists yet — the entry snapshot establishes it first. */
export function saveFullRect(
  rect: Partial<Pick<FullGeom, "x" | "y" | "w" | "h">>,
): void {
  const g = load()
  if (!g.full) return
  g.full = { ...g.full, ...rect }
  save(g)
}

/** Flip the maximized flag without touching the restored rect (the rect is
 *  the windowed geometry; maximizing never overwrites it). No-op if there's
 *  no full record yet, or nothing changed. */
export function saveFullMaximized(maximized: boolean): void {
  const g = load()
  if (!g.full || g.full.maximized === maximized) return
  g.full.maximized = maximized
  save(g)
}

export function readLwY(phase: "expanded" | "tucked"): number {
  return load()[phase].y
}

export function saveLwY(phase: "expanded" | "tucked", y: number): void {
  const g = load()
  g[phase] = { y }
  save(g)
}
