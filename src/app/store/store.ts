// Redux store assembly. Single consolidated RTK Query API
// (`vaultApi`) holds every Tauri command endpoint — no feature-injection
// side-effect imports.

import { configureStore } from "@reduxjs/toolkit"

import { debouncedLocalStorageWrite } from "@/lib/persistence"
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
// keeps the user's last selection. RTK state crosses slice boundaries and is
// read by selectors, so it does not fit `usePersistedState` (a `useState` leaf)
// — but the write side reuses the shared debounced-local-storage primitive so
// there is still a single persistence mechanism (see `lib/persistence.ts`).
store.subscribe(() => {
  debouncedLocalStorageWrite(FILTER_STORAGE_KEY, store.getState().filter.filter)
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
