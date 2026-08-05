// Pure time-range derivations shared by the usage filter slice, the sessions
// browser hook, and the shared DateRangeChip component. Slices own the filter
// STATE; this file owns the math — what day bounds a preset means, and what a
// stored filter effectively means right now. Lives in lib/ so the shared
// component and both features depend on one neutral layer, never on an app
// slice.

import dayjs from "dayjs"

/**
 * Persisted time-range preset. The dynamic ones (today / 7d / 30d / all) are
 * the source of truth — their day bounds are recomputed on every load, so
 * "today" stays today across a restart. Storing concrete dates instead would
 * drift to "yesterday" after midnight. "custom" keeps the user-picked
 * from_day / to_day verbatim.
 */
export type Preset = "today" | "7d" | "30d" | "all" | "custom"

/** The time-range half of a filter state: preset + stored day bounds. */
export interface DayRange {
  range_preset: Preset
  from_day: string
  to_day: string
}

/** Today's local date offset by `offset` days, as "YYYY-MM-DD". */
export function dayStr(offset = 0): string {
  return dayjs().add(offset, "day").format("YYYY-MM-DD")
}

/** Concrete day bounds for a dynamic preset. "custom" / "all" return empty —
 *  "custom" uses the user-picked from_day / to_day, "all" means no bounds. */
export function presetDays(p: Preset): Pick<DayRange, "from_day" | "to_day"> {
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
  f: Pick<DayRange, "range_preset" | "from_day" | "to_day">,
): Pick<DayRange, "from_day" | "to_day"> {
  if (
    f.range_preset === "today" ||
    f.range_preset === "7d" ||
    f.range_preset === "30d"
  ) {
    return presetDays(f.range_preset)
  }
  return { from_day: f.from_day, to_day: f.to_day }
}

/** Type guard for a persisted preset value — anything else is legacy data
 *  (stored before presets existed) and maps to "custom". */
export function isPreset(v: unknown): v is Preset {
  return (
    v === "today" || v === "7d" || v === "30d" || v === "all" || v === "custom"
  )
}
