// Generic JSON helpers shared by the JSON editor and the provider form sheet's
// settingsConfig sync. Kept in lib/ (not a feature) so the editor stays
// feature-agnostic — any future settings-snapshot editor can reuse them.

/** Result of parsing JSON text into a plain object. Empty text counts as `{}`
 *  (a blank snapshot); a syntax error and a non-object top level are reported
 *  separately so callers can decide which failure to surface. */
export type JsonObjectResult =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; error: string }

/** Parse JSON text into a plain object, tagging the outcome. The top-level
 *  must be an object — a settings snapshot that parses to an array or a bare
 *  string is a corrupt snapshot, not a valid config. */
export function parseJsonObject(text: string): JsonObjectResult {
  const trimmed = text.trim()
  if (!trimmed) return { ok: true, value: {} }
  try {
    const parsed: unknown = JSON.parse(trimmed)
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed)
    ) {
      return { ok: false, error: "Expected a JSON object" }
    }
    return { ok: true, value: parsed as Record<string, unknown> }
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    }
  }
}

/** Trim → parse → 2-space stringify. Throws on invalid JSON — callers surface
 *  the error (editor format button, tests). */
export function formatJson(text: string): string {
  return JSON.stringify(JSON.parse(text.trim()), null, 2)
}
