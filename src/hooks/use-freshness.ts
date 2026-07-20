// Data freshness hook (ADR-0007 hooks/ first落地). Tracks the last time the
// Local Store was written (collect / sync) so the cockpit can show "采集于 3 分钟前".
//
// Source of truth for "data was written" is the Tauri `usage_changed` event
// (providers.tsx already invalidates Usage cache on it). Here we *also* persist
// a timestamp to localStorage keyed by device_id, so the hint survives reloads
// and degrades gracefully when no log rows exist yet.
//
// State is held in a module-level Map + useSyncExternalStore. getSnapshot reads
// only from the in-memory map (never re-parses localStorage) so the reference
// stays stable across renders when nothing changed.

import { listen } from "@tauri-apps/api/event"
import { useEffect, useSyncExternalStore } from "react"

import { useGetAppInfoQuery } from "@/features/settings/api"

export interface FreshnessState {
  /** epoch ms of last successful collect (or null if never). */
  lastCollectAt: number | null
  /** epoch ms of last successful sync (or null if never / Standalone). */
  lastSyncAt: number | null
}

const NULL_STATE: FreshnessState = { lastCollectAt: null, lastSyncAt: null }

const stores = new Map<string, FreshnessState>()
const listeners = new Set<() => void>()

function storageKey(deviceId: string) {
  return `vaultone:freshness:${deviceId}`
}

function readStorage(deviceId: string): FreshnessState {
  try {
    const raw = localStorage.getItem(storageKey(deviceId))
    if (!raw) return { ...NULL_STATE }
    const parsed = JSON.parse(raw) as Partial<FreshnessState>
    return {
      lastCollectAt: parsed.lastCollectAt ?? null,
      lastSyncAt: parsed.lastSyncAt ?? null,
    }
  } catch {
    return { ...NULL_STATE }
  }
}

function ensure(deviceId: string): FreshnessState {
  let s = stores.get(deviceId)
  if (!s) {
    s = readStorage(deviceId)
    stores.set(deviceId, s)
  }
  return s
}

function write(deviceId: string, next: FreshnessState) {
  stores.set(deviceId, next)
  try {
    localStorage.setItem(storageKey(deviceId), JSON.stringify(next))
  } catch {
    // localStorage unavailable (quota / private mode) — in-memory only.
  }
  for (const l of listeners) l()
}

function mark(deviceId: string, field: "lastCollectAt" | "lastSyncAt") {
  const cur = ensure(deviceId)
  write(deviceId, { ...cur, [field]: Date.now() })
}

function subscribe(cb: () => void) {
  listeners.add(cb)
  return () => {
    listeners.delete(cb)
  }
}

/**
 * Subscribe to the device's data-freshness state. Mounts a `usage_changed`
 * listener that bumps `lastCollectAt` automatically; callers should also call
 * `markCollected()`/`markSynced()` on their own mutation success for an
 * immediate hint even when the ingest produced no new rows.
 */
export function useFreshness() {
  const { data: info } = useGetAppInfoQuery(undefined, { pollingInterval: 0 })
  const deviceId = info?.device_id ?? null

  useEffect(() => {
    if (!deviceId) return
    ensure(deviceId)
    let unlisten: (() => void) | null = null
    listen("usage_changed", () => mark(deviceId, "lastCollectAt")).then((u) => {
      unlisten = u
    })
    return () => {
      unlisten?.()
    }
  }, [deviceId])

  const state = useSyncExternalStore(
    subscribe,
    () => (deviceId ? ensure(deviceId) : NULL_STATE),
    () => (deviceId ? ensure(deviceId) : NULL_STATE),
  )

  return {
    state,
    markCollected: () => {
      if (deviceId) mark(deviceId, "lastCollectAt")
    },
    markSynced: () => {
      if (deviceId) mark(deviceId, "lastSyncAt")
    },
  }
}
