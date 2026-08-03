// Single error-message seam: turn any error shape into a human-readable,
// i18n-translated string. The structured `AppError { type, data }` that
// crosses the Tauri/RTK-Query boundary (returned, not thrown — see
// `run`/`queryFn` in api.ts) is mapped to an i18n key `errors.<type>` with
// `data` interpolation. Other shapes — a thrown JS `Error` (e.g. from the
// updater plugin, outside the API layer), an RTK-Query-serialised
// `{ message }`, a raw string — fall back to `.message` / String. Returns ""
// when nothing recognisable is found, so callers compose their own fallback:
// `describeError(e, t) || t("common.unknownReason")`.

import type { TFunction } from "i18next"

/** Structural guard for the backend's discriminated error (`{ type, data }`). */
function isAppError(e: unknown): e is { type: string; data: string } {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as Record<string, unknown>).type === "string" &&
    typeof (e as Record<string, unknown>).data === "string"
  )
}

/**
 * Extract a readable, translated reason from an unknown error.
 *
 * `AppError` → `t("errors.<type>", { data })`; other shapes fall back to
 * `.message` / String; "" when nothing recognisable.
 */
export function describeError(e: unknown, t: TFunction): string {
  if (isAppError(e)) return t(`errors.${e.type}`, { data: e.data })
  if (e instanceof Error) return e.message
  if (e && typeof e === "object") {
    const m = e as Record<string, unknown>
    if (typeof m.message === "string") return m.message
    if (typeof m.data === "string") return m.data
    if (typeof m.error === "string") return m.error
  }
  return typeof e === "string" ? e : ""
}
