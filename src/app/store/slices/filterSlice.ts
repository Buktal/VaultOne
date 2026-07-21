// Shared usage-filter state (ADR-0011). Lifted out of DashboardView so the
// toolbar's query conditions are shared across dashboard ⇆ logs. Empty
// string = "no constraint"; toFilter() converts to the nullable UsageFilter
// the API expects.

import { createSlice, type PayloadAction } from "@reduxjs/toolkit"

import type { UsageFilter } from "@/types/generated/bindings"

export interface FilterState {
  from_day: string
  to_day: string
  model: string
  source: string
  device_scope: string
}

export const EMPTY_FILTER: FilterState = {
  from_day: "",
  to_day: "",
  model: "",
  source: "",
  device_scope: "",
}

/** Convert internal FilterState (empty = no constraint) → API UsageFilter (null). */
export function toFilter(s: FilterState): UsageFilter {
  return {
    from_day: s.from_day || null,
    to_day: s.to_day || null,
    model: s.model || null,
    source: s.source || null,
    device_scope: s.device_scope || null,
  }
}

interface FilterSliceState {
  filter: FilterState
}

const initialState: FilterSliceState = { filter: EMPTY_FILTER }

const filterSlice = createSlice({
  name: "filter",
  initialState,
  reducers: {
    setFilter(state, action: PayloadAction<FilterState>) {
      state.filter = action.payload
    },
    patchFilter(state, action: PayloadAction<Partial<FilterState>>) {
      Object.assign(state.filter, action.payload)
    },
    clearFilterKey(state, action: PayloadAction<keyof FilterState>) {
      state.filter[action.payload] = ""
    },
    resetFilter(state) {
      state.filter = EMPTY_FILTER
    },
  },
})

export const { setFilter, patchFilter, clearFilterKey, resetFilter } =
  filterSlice.actions
export default filterSlice.reducer
