//! Shared timestamp helpers.

use std::sync::atomic::{AtomicU64, Ordering};

/// Last emitted millisecond epoch, kept monotonic across calls.
static LAST_MS: AtomicU64 = AtomicU64::new(0);

/// ISO8601 UTC "now" with millisecond precision (e.g. `2026-07-28T01:57:02.123Z`).
/// Used as a last-resort timestamp when a source omits one, and as the
/// written-at marker for DB rows.
///
/// Monotonic: consecutive calls within the same millisecond (or after a
/// backward clock jump) return strictly increasing timestamps. Row
/// `updated_at` values feed latest-wins merge reads — a tie would fall to
/// "first seen" and a same-ms overwrite could silently lose; the invariant
/// lives here, not in callers.
pub(crate) fn now_iso() -> String {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let ms = LAST_MS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            let last = last as i64;
            Some(if now_ms > last {
                now_ms as u64
            } else {
                last as u64 + 1
            })
        })
        .unwrap_or(now_ms as u64);
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
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

/// Format epoch **milliseconds** as ISO8601 UTC with millisecond precision,
/// the ms variant of [`epoch_to_iso`]. OpenCode's `session` / `message` tables
/// store `time_created` / `time_updated` as ms-epoch integers; this keeps the
/// ms→ISO conversion single-source alongside the seconds variant. Same format
/// and `now_iso` fallback as [`epoch_to_iso`].
pub(crate) fn epoch_millis_to_iso(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(now_iso)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The monotonic invariant behind latest-wins merge reads: two consecutive
    /// `now_iso()` calls are strictly increasing even inside the same
    /// millisecond, so a same-ms overwrite can never tie with an earlier row.
    #[test]
    fn now_iso_is_strictly_monotonic() {
        let a = now_iso();
        let b = now_iso();
        assert!(a < b, "same-ms calls must not tie ({a} == {b})");
    }
}
