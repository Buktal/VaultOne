// Display formatting helpers (ADR-0007 shared lib). The JS layer never computes
// cost (ADR-0009) — these are display-only shapers for numbers, currency, dates.
//
// Locale policy (ADR-0016): token counts are ALWAYS K/M/B (international-neutral,
// language-independent); cost is always USD `$`; dates are always the compact
// numeric `MM/DD HH:mm`. Only the relative-time words (`fromNow`) follow the
// language — driven by the dayjs locale set in `@/i18n/languages`. So nothing
// here hard-codes a dayjs locale.

import dayjs from "dayjs"

/** Compact a token count to K/M/B: `3.61M`, `1.2B`, `856`. Language-independent. */
export function formatTokens(n: number | null | undefined): string {
  const v = Number(n ?? 0)
  if (!Number.isFinite(v)) return "0"
  if (v >= 1e9) return `${trim(v / 1e9)}B`
  if (v >= 1e6) return `${trim(v / 1e6)}M`
  if (v >= 1e3) return `${trim(v / 1e3)}K`
  return v.toLocaleString("en-US")
}

/** USD cost with 4 decimals, e.g. `$1.7564`. Null/0 → `$0.0000`. */
export function formatCost(usd: number | null | undefined): string {
  const v = Number(usd ?? 0)
  if (!Number.isFinite(v)) return "$0.0000"
  return `$${v.toFixed(4)}`
}

/** Integer with thousands separators. */
export function formatInt(n: number | null | undefined): string {
  const v = Math.trunc(Number(n ?? 0))
  return v.toLocaleString("en-US")
}

/** Ratio in [0,1] → percent string `90.2%`. */
export function formatPct(rate: number | null | undefined): string {
  const v = Number(rate ?? 0)
  if (!Number.isFinite(v)) return "0%"
  return `${(v * 100).toFixed(1)}%`
}

/** ISO timestamp → `MM/DD HH:mm`. Falls back to the raw string on bad input. */
export function formatTime(ts: string | null | undefined): string {
  if (!ts) return "—"
  const d = dayjs(ts)
  return d.isValid() ? d.format("MM/DD HH:mm") : ts
}

/** ISO day `yyyy-mm-dd` → `MM/DD`. */
export function formatDay(day: string | null | undefined): string {
  if (!day) return "—"
  const d = dayjs(day)
  return d.isValid() ? d.format("MM/DD") : day
}

/** Convert a `<input type="date">` value (yyyy-mm-dd) to a filter day or null. */
export function dateInputToDay(v: string): string | null {
  return v && v.trim() !== "" ? v.trim() : null
}

function trim(n: number): string {
  // 2 decimals, drop trailing zeros for compactness.
  return n
    .toFixed(2)
    .replace(/\.?0+$/, "")
    .trim()
}
