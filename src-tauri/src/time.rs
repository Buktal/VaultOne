//! Shared timestamp helpers.

/// ISO8601 UTC "now" with millisecond precision (e.g. `2026-07-28T01:57:02.123Z`).
/// Used as a last-resort timestamp when a source omits one, and as the
/// written-at marker for DB rows.
pub(crate) fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Format epoch seconds as ISO8601 UTC with millisecond precision (e.g.
/// `2026-07-20T13:26:10.000Z`), matching `now_iso`'s format so source-derived
/// timestamps sort alongside "now" fallbacks. Falls back to `now_iso` for
/// out-of-range inputs so a bad source timestamp never breaks ordering.
pub(crate) fn epoch_to_iso(secs: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(now_iso)
}
