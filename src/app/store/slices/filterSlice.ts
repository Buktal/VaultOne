// Shared usage-filter state (ADR-0011). Lifted out of DashboardView so the
// toolbar's query conditions are shared across dashboard ⇆ logs. Empty
// string = "no constraint"; toFilter() converts to the nullable UsageFilter
// the API expects.
//
// The filter persists to localStorage so the chosen time range / model / device
// scope survive a restart (same pattern as the sidebar/control collapse flags).
// It is pure frontend query state — never sent to the Rust layer.

import { createSlice, type PayloadAction } from "@reduxjs/toolkit"
import dayjs from "dayjs"

import type { UsageFilter } from "@/types/generated/bindings"

export const FILTER_STORAGE_KEY = "vaultone:usage-filter"

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

/** Read the persisted filter at startup; any shape mismatch ⇒ empty filter. */
function loadPersistedFilter(): FilterState {
  if (typeof localStorage === "undefined") return EMPTY_FILTER
  const raw = localStorage.getItem(FILTER_STORAGE_KEY)
  if (!raw) return EMPTY_FILTER
  try {
    const p = JSON.parse(raw) as Partial<FilterState>
    const str = (v: unknown) => (typeof v === "string" ? v : "")
    return {
      from_day: str(p.from_day),
      to_day: str(p.to_day),
      model: str(p.model),
      source: str(p.source),
      device_scope: str(p.device_scope),
    }
  } catch {
    return EMPTY_FILTER
  }
}

/** Convert internal FilterState (empty = no constraint) → API UsageFilter (null). */
export function toFilter(s: FilterState): UsageFilter {
  return {
    // Local-day range → inclusive UTC timestamp bounds. The backend filters on
    // `timestamp` (UTC), not the UTC `day` bucket: a local "today" in UTC+8
    // straddles two UTC days, so we must widen to timestamps or the early-
    // morning rows (whose UTC day is still yesterday) vanish from "today".
    from_ts: s.from_day ? dayjs(s.from_day).startOf("day").toISOString() : null,
    to_ts: s.to_day ? dayjs(s.to_day).endOf("day").toISOString() : null,
    model: s.model || null,
    source: s.source || null,
    device_scope: s.device_scope || null,
  }
}

interface FilterSliceState {
  filter: FilterState
}

const initialState: FilterSliceState = { filter: loadPersistedFilter() }

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
