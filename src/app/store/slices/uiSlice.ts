// UI preferences (ADR-0011). refreshInterval drives the shared polling
// cadence across dashboard charts/logs — replaces the 5 hardcoded 30_000
// sites. 0 = off (no polling). Persisted to localStorage.

import { createSlice, type PayloadAction } from "@reduxjs/toolkit"

const REFRESH_KEY = "vaultone:refresh-interval"

export type RefreshInterval = 0 | 5 | 10 | 30 | 60

export const REFRESH_OPTIONS: ReadonlyArray<{
  value: RefreshInterval
  label: string
}> = [
  { value: 0, label: "关闭" },
  { value: 5, label: "5 秒" },
  { value: 10, label: "10 秒" },
  { value: 30, label: "30 秒" },
  { value: 60, label: "60 秒" },
]

const VALID: ReadonlyArray<number> = [0, 5, 10, 30, 60]

function readInitial(): RefreshInterval {
  if (typeof localStorage === "undefined") return 30
  const raw = Number(localStorage.getItem(REFRESH_KEY))
  return VALID.includes(raw) ? (raw as RefreshInterval) : 30
}

const uiSlice = createSlice({
  name: "ui",
  initialState: { refreshInterval: readInitial() as RefreshInterval },
  reducers: {
    setRefreshInterval(state, action: PayloadAction<RefreshInterval>) {
      state.refreshInterval = action.payload
      if (typeof localStorage !== "undefined") {
        localStorage.setItem(REFRESH_KEY, String(action.payload))
      }
    },
  },
})

export const { setRefreshInterval } = uiSlice.actions
export default uiSlice.reducer
