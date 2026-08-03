// Update Check orchestration. Exposes the side-effect surface:
//   - checkNow:    probe GitHub Releases for a newer version (startup silent
//                  probe is 24h-throttled via a localStorage stamp; Settings
//                  calls this manually). A check() failure is silent (back to
//                  idle; the indicator never shows).
//   - applyUpdate: downloadAndInstall the pending Update. Progress → slice;
//                  success → ready; failure → Manual Fallback (failed).
//   - restartNow:  restart after a ready install (process:allow-restart).
//   - openReleases: open GitHub Releases (footer 📖 button + Manual Fallback).
//
// The pending Update object (returned by check, holds downloadAndInstall) is
// module-level: at most one is in flight at a time and it is shared across hook
// instances — App mounts the startup probe, UpdateCard calls applyUpdate.

import { openUrl } from "@tauri-apps/plugin-opener"
import { relaunch } from "@tauri-apps/plugin-process"
import { check, type Update } from "@tauri-apps/plugin-updater"
import { useCallback, useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"

import { useAppDispatch } from "@/app/store/hooks"
import {
  setAvailable,
  setChecking,
  setDownloading,
  setFailed,
  setIdle,
  setReady,
  setUpToDate,
} from "@/app/store/slices/updateSlice"
import { describeError } from "@/lib/error"
import { usePersistedState } from "@/lib/persistence"

const LAST_CHECK_KEY = "vaultone:update-last-check"
const THROTTLE_MS = 24 * 60 * 60 * 1000

const RELEASES_URL = "https://github.com/Buktal/VaultOne/releases/latest"

/** Singleton: the Update found by the last check (holds downloadAndInstall).
 *  This is the documented exception to usePersistedState — a Tauri `Update`
 *  carries a non-serializable side-effect (`downloadAndInstall`) and must be
 *  shared app-wide across the App / footer / Settings hook instances; it stays
 *  a module-level `let`, not persisted state. Only the throttle timestamp
 *  below is serializable leaf state. */
let pendingUpdate: Update | null = null
/** Singleton: the startup probe runs exactly once app-wide, even though
 *  useUpdateCheck is mounted in App + footer + Settings. */
let startupProbed = false

export function useUpdateCheck() {
  const dispatch = useAppDispatch()
  const { t } = useTranslation()
  // Guard against a probe already in flight (startup fire + manual click).
  const inFlight = useRef(false)
  // 24h-throttle stamp for the silent startup probe. Plain number →
  // usePersistedState. The legacy raw-numeric-string format JSON-parses to the
  // same number, so an upgrade carries the old stamp over.
  const [lastCheck, setLastCheck] = usePersistedState<number>(LAST_CHECK_KEY, 0)

  const checkNow = useCallback(async () => {
    if (inFlight.current) return
    inFlight.current = true
    dispatch(setChecking())
    try {
      const update = await check()
      if (update?.available) {
        pendingUpdate = update
        dispatch(
          setAvailable({
            version: update.version,
            currentVersion: update.currentVersion,
            notes: update.body ?? null,
          }),
        )
      } else {
        pendingUpdate = null
        dispatch(setUpToDate())
      }
    } catch {
      // Silent failure: no network, 404 latest.json, endpoint down.
      pendingUpdate = null
      dispatch(setIdle())
    } finally {
      inFlight.current = false
    }
  }, [dispatch])

  const applyUpdate = useCallback(async () => {
    const update = pendingUpdate
    if (!update) return
    let downloaded = 0
    let total = 0
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started" && "contentLength" in event.data) {
          total = event.data.contentLength ?? 0
          dispatch(setDownloading({ downloadedBytes: 0, totalBytes: total }))
        } else if (event.event === "Progress" && "chunkLength" in event.data) {
          downloaded += event.data.chunkLength
          dispatch(
            setDownloading({ downloadedBytes: downloaded, totalBytes: total }),
          )
        }
      })
      dispatch(setReady())
      await update.close()
    } catch (e) {
      // Manual Fallback: surface the "go to GitHub" card.
      dispatch(setFailed({ error: describeError(e, t) || String(e) }))
    }
  }, [dispatch, t])

  const restartNow = useCallback(async () => {
    await relaunch()
  }, [])

  const openReleases = useCallback(async () => {
    await openUrl(RELEASES_URL)
  }, [])

  // Startup silent probe, 24h-throttled. Guarded app-wide so the
  // many useUpdateCheck mounts (App + footer + Settings) fire it exactly once.
  // The stamp write is debounced via usePersistedState and flushed on unmount,
  // so it still lands before a close even if checkNow() never returns.
  useEffect(() => {
    if (startupProbed) return
    startupProbed = true
    if (Date.now() - lastCheck >= THROTTLE_MS) {
      setLastCheck(Date.now())
      void checkNow()
    }
  }, [checkNow, lastCheck, setLastCheck])

  return { checkNow, applyUpdate, restartNow, openReleases }
}
