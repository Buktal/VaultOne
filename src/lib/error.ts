// Single error-message seam: turn any thrown/returned error shape into a
// human-readable string. Covers the shapes that cross the Tauri/RTK-Query
// boundary — a thrown `Error` (run() throws `new Error("Type: detail")`), an
// RTK-Query-serialised `{ message }`, and the `{ data }` / `{ error }` envelope
// shapes. Returns "" when nothing recognisable is found, so callers compose
// their own fallback: `describeError(e) || t("...")`.

/** Extract a readable reason from an unknown error, or "" if none found. */
export function describeError(e: unknown): string {
  if (e instanceof Error) return e.message
  if (e && typeof e === "object") {
    const m = e as Record<string, unknown>
    if (typeof m.message === "string") return m.message
    if (typeof m.data === "string") return m.data
    if (typeof m.error === "string") return m.error
  }
  return typeof e === "string" ? e : ""
}
