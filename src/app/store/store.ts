// Redux store assembly (ADR-0007). Single consolidated RTK Query API
// (`vaultApi`) holds every Tauri command endpoint — no feature-injection
// side-effect imports.

import { configureStore } from "@reduxjs/toolkit"

import { vaultApi } from "./api"
import filterReducer from "./slices/filterSlice"
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

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
