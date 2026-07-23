// Update state machine (ADR-0017). The footer ⓘ indicator reads `status` to
// decide visibility; UpdateCard renders one body per status. A check() failure
// is silent (back to idle — the indicator never shows); only a download/install
// failure surfaces the Manual Fallback card.

import { createSlice } from "@reduxjs/toolkit"

export type UpdateStatus =
  | "idle" // not probed / silent check failure
  | "checking" // probe in flight
  | "up-to-date" // probe done, no new version
  | "available" // new version found, awaiting user action
  | "downloading" // downloadAndInstall in progress
  | "ready" // installed, awaiting relaunch
  | "failed" // download/install failed → Manual Fallback

export interface UpdateState {
  status: UpdateStatus
  /** Latest release version, e.g. "1.1.0". */
  version: string | null
  /** Running app version, e.g. "1.0.0". */
  currentVersion: string | null
  /** Raw release notes (markdown) from latest.json `notes`. */
  notes: string | null
  /** Failure reason for the Manual Fallback card. */
  error: string | null
  /** Download progress, bytes. */
  downloadedBytes: number
  totalBytes: number
}

const initialState: UpdateState = {
  status: "idle",
  version: null,
  currentVersion: null,
  notes: null,
  error: null,
  downloadedBytes: 0,
  totalBytes: 0,
}

const updateSlice = createSlice({
  name: "update",
  initialState,
  reducers: {
    setChecking(state) {
      state.status = "checking"
    },
    setIdle(state) {
      state.status = "idle"
    },
    setUpToDate(state) {
      state.status = "up-to-date"
    },
    setAvailable(
      state,
      action: {
        payload: {
          version: string
          currentVersion: string
          notes: string | null
        }
      },
    ) {
      state.status = "available"
      state.version = action.payload.version
      state.currentVersion = action.payload.currentVersion
      state.notes = action.payload.notes
    },
    setDownloading(
      state,
      action: { payload: { downloadedBytes: number; totalBytes: number } },
    ) {
      state.status = "downloading"
      state.downloadedBytes = action.payload.downloadedBytes
      state.totalBytes = action.payload.totalBytes
    },
    setReady(state) {
      state.status = "ready"
    },
    setFailed(state, action: { payload: { error: string } }) {
      state.status = "failed"
      state.error = action.payload.error
    },
  },
})

export const {
  setChecking,
  setIdle,
  setUpToDate,
  setAvailable,
  setDownloading,
  setReady,
  setFailed,
} = updateSlice.actions

export default updateSlice.reducer
