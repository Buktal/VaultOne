// Session source tag → display label. Single source for the sessions feature so
// the list table, the detail sheet's source line, and the source-filter dropdown
// all agree (architecture.md: 单一事实来源 — previously two copies, providerLabel
// in sessions-view and sourceLabel in session-detail-sheet, had drifted into a
// third use site).
//
// `source` is the stable provider tag on every session row (e.g. "claude_code").
// Sessions use the full product name incl. "CLI" — unlike the usage view
// (features/usage/source-labels.ts, short "Codex"/"Grok"), a session row carries
// no extra context to disambiguate. Unknown tags fall through verbatim so a new
// provider shows up before a mapping is added; an empty tag shows "—".

const SESSION_SOURCE_LABELS: Record<string, string> = {
  claude_code: "Claude Code",
  codex_cli: "Codex CLI",
  gemini_cli: "Gemini CLI",
  grok_cli: "Grok CLI",
  opencode: "OpenCode",
}

/** Map a session source tag to its display name; unknown tags verbatim, empty → "—". */
export function sessionSourceLabel(source: string): string {
  return SESSION_SOURCE_LABELS[source] ?? (source || "—")
}
