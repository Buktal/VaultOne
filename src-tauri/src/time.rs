//! Shared timestamp helpers.

/// ISO8601 UTC "now" with millisecond precision (e.g. `2026-07-28T01:57:02.123Z`).
/// Used as a last-resort timestamp when a source omits one, and as the
/// written-at marker for DB rows.
pub(crate) fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
