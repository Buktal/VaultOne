// Redux store assembly (ADR-0007). Imports feature api modules for side-effect
// injection into the single base api before the store is created.

import { configureStore } from "@reduxjs/toolkit"

import { api } from "./api"
import viewReducer from "./slices/viewSlice"

// Side-effect: register usage / pricing / settings endpoints on the base api.
import "@/features/usage/api"
import "@/features/pricing/api"
import "@/features/settings/api"

export const store = configureStore({
  reducer: {
    view: viewReducer,
    [api.reducerPath]: api.reducer,
  },
  middleware: (getDefault) => getDefault().concat(api.middleware),
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
