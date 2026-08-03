// UI state persistence primitives.
//
// Two layers:
//   1. `debouncedLocalStorageWrite(key, value, debounceMs?)` — coalesces a burst
//      of writes (e.g. window `onMoved` firing once per pixel of a drag) into
//      one disk write per idle gap, so dragging a window no longer hammers
//      localStorage. The latest value per key is held in memory and committed
//      when the burst settles (default 300ms, matching the Redux filter
//      persistence in app/store/store.ts). Pending writes are flushed
//      synchronously via `flushPendingWrites()` — wired to `beforeunload` below
//      and to the consumers' unmount effects — so the trailing debounced value
//      is never lost to a close/unmount mid-burst.
//   2. `usePersistedState(key, initial, opts?)` — a `useState` backed by
//      localStorage: lazy-reads on mount, debounced-writes on change, flushes on
//      unmount. For serializable leaf state (booleans, numbers, small objects).
//
// ─── Deliberately NOT migrated here (different primitives, kept as-is) ───
// These two cases are genuinely different shapes; forcing them through
// usePersistedState would lose clarity. They reuse only the debounced writer:
//
//   • `useFreshness` (src/hooks/use-freshness.ts) — per-device freshness state
//     shared LIVE across several hooks/components at once (the cockpit pulse
//     + the control card both read it). Needs `useSyncExternalStore` over a
//     module-level Map so every subscriber sees the same ticking timestamp the
//     instant a collect/sync lands. `usePersistedState` is per-instance local
//     state and cannot share like that. Reuses `debouncedLocalStorageWrite` for
//     the disk mirror only.
//   • `useUpdateCheck` (src/app/shell/use-update-check.ts) — holds a
//     non-serializable Tauri `Update` object (it carries the `downloadAndInstall`
//     side effect) that must outlive any single hook instance and be shared
//     app-wide; that singleton stays a module-level `let`. Only its throttled
//     "last check" timestamp (a plain number) goes through `usePersistedState`.
//
// ─── Also NOT migrated (different mechanism, kept as the reference) ───
//   • Redux filter slice (`FILTER_STORAGE_KEY`) — persists via `store.subscribe`
//     + `setTimeout`, the ORIGINAL debounced pattern whose 300ms this module's
//     default aligns with. RTK state crosses slice boundaries and is read by
//     selectors; it is not a `useState` leaf and does not fit usePersistedState.

import { type Dispatch, type SetStateAction, useEffect, useState } from "react"

const DEFAULT_DEBOUNCE_MS = 300

/** Per-key pending timer for the debounced write. */
const timers = new Map<string, ReturnType<typeof setTimeout>>()
/** Per-key latest value to commit when the timer fires (or on flush). The timer
 *  and the pending entry are always created and cleared together. */
const pending = new Map<string, unknown>()

function commit(key: string): void {
  const handle = timers.get(key)
  if (handle !== undefined) {
    clearTimeout(handle)
    timers.delete(key)
  }
  const value = pending.get(key)
  if (value === undefined) return // nothing pending
  pending.delete(key)
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // localStorage unavailable (private mode / quota) — persistence is
    // best-effort; never crash a UI interaction over it.
  }
}

/** Schedule a debounced write of `value` to `key` (JSON-serialized). Repeated
 *  calls within `debounceMs` reset the timer and replace the pending value, so
 *  only the LAST value in a burst lands on disk. Default 300ms (aligned with
 *  the Redux filter persistence). A `value` of `undefined` is a no-op — nothing
 *  meaningful to persist. */
export function debouncedLocalStorageWrite(
  key: string,
  value: unknown,
  debounceMs: number = DEFAULT_DEBOUNCE_MS,
): void {
  if (value === undefined) return
  pending.set(key, value)
  const existing = timers.get(key)
  if (existing !== undefined) clearTimeout(existing)
  const handle = setTimeout(() => {
    timers.delete(key)
    commit(key)
  }, debounceMs)
  timers.set(key, handle)
}

/** Synchronously commit every pending debounced write now (clears timers).
 *  Idempotent — safe to call repeatedly. Called automatically on
 *  `beforeunload`; `usePersistedState` and the window-geometry hooks also call
 *  it on unmount so the trailing value of a drag/resize is never lost. */
export function flushPendingWrites(): void {
  for (const key of [...timers.keys()]) commit(key)
}

// Auto-flush on page teardown so a close during a debounce window still lands
// the last value. Some Tauri close paths bypass beforeunload, so hooks also
// flush on unmount as a second line of defense.
if (
  typeof window !== "undefined" &&
  typeof window.addEventListener === "function"
) {
  window.addEventListener("beforeunload", flushPendingWrites, { capture: true })
}

/** Read and JSON-parse `key`; return `initial` on miss / parse failure. */
function readPersisted<T>(key: string, initial: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (raw == null) return initial
    return JSON.parse(raw) as T
  } catch {
    return initial
  }
}

/**
 * `useState` backed by localStorage.
 *
 * Lazy-reads `key` on mount (falling back to `initial`), debounced-writes on
 * every change (default 300ms), and flushes any pending write on unmount so the
 * last set is never lost. For serializable leaf state only — booleans,
 * numbers, small plain objects. Legacy on-disk formats parse leniently: a
 * stored "1"/"0" token reads back as the JSON value `1`/`0`, which is truthy/
 * falsy, so a boolean consumer toggled under the previous raw-string format
 * keeps behaving correctly until its first re-write (which then lands as JSON).
 *
 * Not for state that must be shared live across components (use
 * `useSyncExternalStore` for that, see `useFreshness`) or that holds non-
 * serializable values (keep those in a module singleton).
 */
export function usePersistedState<T>(
  key: string,
  initial: T,
  opts?: { debounceMs?: number },
): [T, Dispatch<SetStateAction<T>>] {
  const debounceMs = opts?.debounceMs ?? DEFAULT_DEBOUNCE_MS
  const [state, setState] = useState<T>(() => readPersisted(key, initial))

  // Debounced mirror to disk on every change. The mount run writes the value
  // we just read back (a no-op content-wise); matching the prior useEffect-
  // based pattern and keeping the implementation free of a skip-first ref that
  // StrictMode would defeat anyway.
  useEffect(() => {
    debouncedLocalStorageWrite(key, state, debounceMs)
  }, [key, state, debounceMs])

  // Flush the trailing debounced write on unmount so the last set isn't lost.
  useEffect(() => {
    return () => flushPendingWrites()
  }, [])

  return [state, setState]
}
