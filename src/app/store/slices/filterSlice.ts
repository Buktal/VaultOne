// Shared usage-filter state. Lifted out of DashboardView so the
// toolbar's query conditions are shared across dashboard ⇆ logs. Empty
// string = "no constraint"; toFilter() converts to the nullable UsageFilter
// the API expects.
//
// The filter persists to localStorage so the chosen time range / model / device
// scope survive a restart (same pattern as the sidebar/control collapse flags).
// It is pure frontend query state — never sent to the Rust layer.

import { createSlice, type PayloadAction } from "@reduxjs/toolkit"
import dayjs from "dayjs"
import { useMemo } from "react"

import { useAppSelector } from "@/app/store/hooks"
import type { UsageFilter } from "@/types/generated/bindings"

export const FILTER_STORAGE_KEY = "vaultone:usage-filter"

/** Persisted time-range preset. The dynamic ones (today / 7d / 30d / all) are
 *  the source of truth — their day bounds are recomputed on every load, so
 *  "today" stays today across a restart. Storing concrete dates instead would
 *  drift to "yesterday" after midnight. "custom" keeps the user-picked
 *  from_day / to_day verbatim. */
export type Preset = "today" | "7d" | "30d" | "all" | "custom"

export interface FilterState {
  range_preset: Preset
  from_day: string
  to_day: string
  model: string
  source: string
  device_scope: string
}

export const EMPTY_FILTER: FilterState = {
  range_preset: "all",
  from_day: "",
  to_day: "",
  model: "",
  source: "",
  device_scope: "",
}

/** Today's local date offset by `offset` days, as "YYYY-MM-DD". */
export function dayStr(offset = 0): string {
  return dayjs().add(offset, "day").format("YYYY-MM-DD")
}

/** Concrete day bounds for a dynamic preset. "custom" / "all" return empty —
 *  "custom" uses the user-picked from_day / to_day, "all" means no bounds. */
export function presetDays(
  p: Preset,
): Pick<FilterState, "from_day" | "to_day"> {
  switch (p) {
    case "today":
      return { from_day: dayStr(), to_day: dayStr() }
    case "7d":
      return { from_day: dayStr(-6), to_day: dayStr() }
    case "30d":
      return { from_day: dayStr(-29), to_day: dayStr() }
    default:
      return { from_day: "", to_day: "" }
  }
}

/** The EFFECTIVE day bounds for a filter: a dynamic preset (today / 7d / 30d)
 *  is recomputed on the spot, so a preset picked yesterday still means "today"
 *  when the app is left running across midnight (the stored from_day/to_day
 *  are frozen at selection time and must not be trusted). "all" / "custom"
 *  return the stored values verbatim — "all" stores empty bounds, "custom"
 *  keeps the user-picked days. Single place that answers "what days does this
 *  filter mean", shared by the query path and the date-input display. */
export function effectiveDays(
  f: Pick<FilterState, "range_preset" | "from_day" | "to_day">,
): Pick<FilterState, "from_day" | "to_day"> {
  if (
    f.range_preset === "today" ||
    f.range_preset === "7d" ||
    f.range_preset === "30d"
  ) {
    return presetDays(f.range_preset)
  }
  return { from_day: f.from_day, to_day: f.to_day }
}

function isPreset(v: unknown): v is Preset {
  return (
    v === "today" || v === "7d" || v === "30d" || v === "all" || v === "custom"
  )
}

/** Read the persisted filter at startup; any shape mismatch ⇒ empty filter.
 *  Dynamic presets are re-expanded here so a restart never serves stale dates
 *  (the bug: "today" selected, then overnight it read back as "yesterday").
 *  Exported so the re-expand + legacy-back-compat behavior is unit-tested. */
export function loadPersistedFilter(): FilterState {
  const base = (over: Partial<FilterState>): FilterState => ({
    ...EMPTY_FILTER,
    ...over,
  })
  if (typeof localStorage === "undefined") return base({})
  const raw = localStorage.getItem(FILTER_STORAGE_KEY)
  if (!raw) return base({})
  try {
    const p = JSON.parse(raw) as Partial<FilterState>
    const str = (v: unknown) => (typeof v === "string" ? v : "")
    const common = {
      model: str(p.model),
      source: str(p.source),
      device_scope: str(p.device_scope),
    }
    // A missing preset is legacy data that stored only concrete dates — treat
    // it as "custom" so those literal bounds survive unchanged.
    const preset: Preset = isPreset(p.range_preset) ? p.range_preset : "custom"
    if (preset === "custom") {
      return base({
        range_preset: "custom",
        from_day: str(p.from_day),
        to_day: str(p.to_day),
        ...common,
      })
    }
    const days = presetDays(preset)
    return base({ range_preset: preset, ...days, ...common })
  } catch {
    return base({})
  }
}

/** Convert internal FilterState (empty = no constraint) → API UsageFilter (null). */
export function toFilter(s: FilterState): UsageFilter {
  const { from_day, to_day } = effectiveDays(s)
  return {
    // Local-day range → inclusive UTC timestamp bounds. The backend filters on
    // `timestamp` (UTC), not the UTC `day` bucket: a local "today" in UTC+8
    // straddles two UTC days, so we must widen to timestamps or the early-
    // morning rows (whose UTC day is still yesterday) vanish from "today".
    // effectiveDays keeps a dynamic preset rolling to the current day instead
    // of trusting the frozen stored dates (cross-midnight fix).
    from_ts: from_day ? dayjs(from_day).startOf("day").toISOString() : null,
    to_ts: to_day ? dayjs(to_day).endOf("day").toISOString() : null,
    model: s.model || null,
    source: s.source || null,
    device_scope: s.device_scope || null,
  }
}

/**
 * The active dashboard filter as an API-level UsageFilter (selector + toFilter).
 * Replaces the per-view `useMemo(() => toFilter(filter), [filter])` repetition —
 * the dashboard / logs / recent-requests views all bind the same filter, so the
 * selector + memo live once here.
 */
export function useUsageFilter(): UsageFilter {
  const filter = useAppSelector((s) => s.filter.filter)
  // `dayStr()` in the deps: the filter identity must flip at midnight so a
  // dynamic preset re-queries the new day's data. Dropping the memo entirely
  // would rebuild the object every render — request-log-table resets its page
  // offset on filter identity change, so that would clobber pagination.
  // biome-ignore lint/correctness/useExhaustiveDependencies: dayStr() is the cross-midnight rollover trigger — a stable value within a day, a new identity after midnight (that is the point)
  return useMemo(() => toFilter(filter), [filter, dayStr()])
}

/**
 * A "today only" UsageFilter scoped to one device — the lightweight card's
 * per-device today snapshot. `today` is passed in (not derived here) so the
 * caller controls when the day rolls over and tests can pin a date. Reuses
 * toFilter so the local-day → UTC bounds match the dashboard's "today" preset.
 */
export function todayFilter(deviceScope: string, today: string): UsageFilter {
  return toFilter({
    ...EMPTY_FILTER,
    from_day: today,
    to_day: today,
    device_scope: deviceScope,
  })
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
    clearFilterKey(
      state,
      action: PayloadAction<Exclude<keyof FilterState, "range_preset">>,
    ) {
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
