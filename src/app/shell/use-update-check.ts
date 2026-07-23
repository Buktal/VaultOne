// Update Check orchestration (ADR-0017). Exposes the side-effect surface:
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

const LAST_CHECK_KEY = "vaultone:update-last-check"
const THROTTLE_MS = 24 * 60 * 60 * 1000

const RELEASES_URL = "https://github.com/Buktal/VaultOne/releases/latest"

/** Singleton: the Update found by the last check (holds downloadAndInstall). */
let pendingUpdate: Update | null = null
/** Singleton: the startup probe runs exactly once app-wide, even though
 *  useUpdateCheck is mounted in App + footer + Settings. */
let startupProbed = false

export function useUpdateCheck() {
  const dispatch = useAppDispatch()
  // Guard against a probe already in flight (startup fire + manual click).
  const inFlight = useRef(false)

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
      // Silent failure (ADR-0017): no network, 404 latest.json, endpoint down.
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
      // Manual Fallback (ADR-0017): surface the "go to GitHub" card.
      dispatch(setFailed({ error: describeError(e) }))
    }
  }, [dispatch])

  const restartNow = useCallback(async () => {
    await relaunch()
  }, [])

  const openReleases = useCallback(async () => {
    await openUrl(RELEASES_URL)
  }, [])

  // Startup silent probe, 24h-throttled (ADR-0017). Guarded app-wide so the
  // many useUpdateCheck mounts (App + footer + Settings) fire it exactly once.
  useEffect(() => {
    if (startupProbed) return
    startupProbed = true
    const last = Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0)
    if (Date.now() - last >= THROTTLE_MS) {
      localStorage.setItem(LAST_CHECK_KEY, String(Date.now()))
      void checkNow()
    }
  }, [checkNow])

  return { checkNow, applyUpdate, restartNow, openReleases }
}

function describeError(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
