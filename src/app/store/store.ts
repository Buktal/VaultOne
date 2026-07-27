// Redux store assembly (ADR-0007). Single consolidated RTK Query API
// (`vaultApi`) holds every Tauri command endpoint — no feature-injection
// side-effect imports.

import { configureStore } from "@reduxjs/toolkit"

import { vaultApi } from "./api"
import filterReducer, { FILTER_STORAGE_KEY } from "./slices/filterSlice"
import updateReducer from "./slices/updateSlice"
import viewReducer from "./slices/viewSlice"

export const store = configureStore({
  reducer: {
    view: viewReducer,
    filter: filterReducer,
    update: updateReducer,
    [vaultApi.reducerPath]: vaultApi.reducer,
  },
  middleware: (getDefault) => getDefault().concat(vaultApi.middleware),
})

// Persist the usage filter (time range / model / device scope) so a restart
// keeps the user's last selection. Debounced — a date-range keystroke dispatches
// per keystroke, but we only write once per idle gap.
let persistTimer: ReturnType<typeof setTimeout> | undefined
store.subscribe(() => {
  if (typeof localStorage === "undefined") return
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(() => {
    try {
      const f = store.getState().filter.filter
      localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(f))
    } catch {
      // Quota / private mode — persistence is best-effort.
    }
  }, 300)
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
