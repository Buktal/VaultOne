//! Collect orchestration: parse Sources into the Local Store, then sync with
//! peer devices.
//!
//! `collect_into` is the single ingest path shared by the manual actions and
//! the background scheduler. `align` is the full manual action (collect, then
//! pull+push in Synced mode); `sync_round` is one pull+push pass, shared by
//! `align` (which retries it) and the background scheduler (which runs it once
//! per push interval). Collect and sync are DECOUPLED at the scheduler —
//! collect is a short seconds-level local cadence, sync is a longer
//! minutes-level Git cadence (Synced only), so the scheduler triggers them on
//! independent deadlines rather than chaining them.

pub mod artifact;
pub mod ingest;
pub mod jsonl;

use std::time::Duration;

use self::ingest::IngestReport;
use crate::config::ConfigStore;
use crate::db::Store;
use crate::error::AppResult;
use crate::sync;

/// Parse Source → Local Store (+ JSONL Artifact). No network.
/// Shared by the manual `collect_now` command and the background scheduler so
/// both follow the exact same ingest path.
///
/// Iterates every enabled provider. The per-file cursor table is loaded once
/// and shared (keys are file paths, disjoint across providers); each provider's
/// cursor advances are merged and persisted AFTER all ingests — so a failed
/// ingest leaves cursors untouched (next collect re-parses the same lines; the
/// store's primary-key dedup absorbs the re-read). First run / empty table ⇒
/// full scan.
pub fn collect_into(store: &Store, config: &ConfigStore) -> AppResult<IngestReport> {
    let providers = crate::providers::all_providers()?;
    let cfg = config.get();
    let paths = config.paths();
    let progress = store.load_scan_progress()?;
    crate::devices::touch_self(store, &cfg)?;
    let book = store.load_pricing_book()?;

    let mut merged = IngestReport::default();
    let mut merged_delta = crate::providers::ScanProgressDelta::new();
    let mut sources_with_rows: Vec<String> = Vec::new();
    for provider in &providers {
        let (result, delta) = provider.collect_incremental(&progress)?;
        let report = ingest::ingest_collected(store, &paths, &cfg.device_id, &book, result)?;
        if report.rows_inserted > 0 {
            sources_with_rows.push(report.source.clone());
        }
        merged.events_collected += report.events_collected;
        merged.rows_inserted += report.rows_inserted;
        merged.turn_durations_collected += report.turn_durations_collected;
        merged.turn_durations_inserted += report.turn_durations_inserted;
        merged.files_scanned += report.files_scanned;
        merged.lines_skipped += report.lines_skipped;
        merged_delta.extend(delta);
    }
    merged.source = sources_with_rows.join(",");
    // Self-heal: backfill device rows for any device that has usage but was
    // never published (no name artifact) so it still appears in the picker. Runs
    // here, on the collect path — not on the read-only list_devices command — so
    // a query never mutates the DB. Worst-case latency to surface a new device
    // is one collect interval.
    store.discover_devices_from_usage()?;
    store.save_scan_progress(&merged_delta)?;
    // Drop devices the local repo no longer backs (e.g. a peer deleted itself
    // and its data is gone, or a regenerated-id residue). The local repo
    // filesystem is the source of truth and is always available, so this runs
    // on every collect — not only on a sync pull.
    crate::devices::reconcile_devices(store, &paths, &cfg)?;
    Ok(merged)
}

/// Outcome of one「采集 / 同步」action, surfaced to the UI. Best-effort: every
/// step runs independently, so `errors` carries per-step failures rather than
/// aborting early. Empty on full success.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct AlignReport {
    /// Local collect outcome (zeroed if collect itself failed — see `errors`).
    pub collected: IngestReport,
    /// Rows imported from peer devices this round (Synced only).
    pub imported: u32,
    /// True iff a local change was committed and pushed (Synced only).
    pub pushed: bool,
    /// Per-step failures (`collect: …`, `pull: …`, `push: …`). Empty on success.
    pub errors: Vec<String>,
}

/// Outcome of [`sync_round`] — the pull/push half of an [`AlignReport`].
#[derive(Debug, Clone, Default)]
pub(crate) struct SyncRoundOutcome {
    pub(crate) imported: u32,
    pub(crate) pushed: bool,
    pub(crate) errors: Vec<String>,
}

/// One best-effort sync round: pull peer devices' Artifacts, then push this
/// device's. Both steps run independently — a pull failure does NOT skip push
/// (a failed pull usually means nothing new to push, but push may still succeed
/// on a flaky network). Errors land in `errors` rather than aborting the round.
/// Shared by the manual [`align`] (which retries it) and the background
/// scheduler (which runs it once per push interval — the cadence IS the retry).
/// Synced only; a no-op (zeroed outcome) in Standalone.
pub(crate) fn sync_round(store: &Store, config: &ConfigStore) -> SyncRoundOutcome {
    let mut out = SyncRoundOutcome::default();
    let cfg = config.get();
    if !cfg.is_synced() {
        return out;
    }
    let paths = config.paths();
    match sync::pull_and_import(store, &paths, &cfg) {
        Ok(n) => out.imported = n,
        Err(e) => out.errors.push(format!("pull: {e}")),
    }
    match sync::push_usage(store, &paths, &cfg) {
        Ok(p) => out.pushed = p,
        Err(e) => out.errors.push(format!("push: {e}")),
    }
    out
}

/// Bounded retry over a sync round, factored out of [`align`] so the
/// retry-aggregation logic is unit-testable without real git IO or real time.
///
/// `round` produces one [`SyncRoundOutcome`] per call; `sleep` backs off
/// between attempts (production passes [`std::thread::sleep`]; tests inject a
/// no-op so the 3-attempt retry is instant). Stops early once a round returns
/// no errors; otherwise runs `max_attempts` times.
///
/// `imported` is SUMMED across retries: pull is uuid-deduped, so a row pulled
/// on attempt 1 (then lost to a push failure) reads 0 on attempt 2 — taking
/// only the last round's imported would report "0 imported" despite real new
/// rows. The returned outcome carries the sum in `imported`, plus the final
/// round's `pushed` / `errors`.
fn retry_rounds<R, S>(mut round: R, max_attempts: u32, mut sleep: S) -> SyncRoundOutcome
where
    R: FnMut() -> SyncRoundOutcome,
    S: FnMut(Duration),
{
    let mut last = SyncRoundOutcome::default();
    let mut imported = 0u32;
    for attempt in 0u32..max_attempts {
        last = round();
        imported += last.imported;
        if last.errors.is_empty() {
            break;
        }
        // Back off before the next attempt (1 s, 2 s); skip after the last.
        if attempt + 1 < max_attempts {
            sleep(Duration::from_secs(1u64 << attempt));
        }
    }
    last.imported = imported;
    last
}

/// Full manual「同步 / 采集」: collect locally, then (Synced only) run
/// [`sync_round`] with a bounded retry — up to 3 attempts with a short backoff
/// (1 s, 2 s). Retry covers only the network steps (pull/push); collect runs
/// once (a local disk failure won't fix itself on retry). Best-effort: every
/// step's outcome is reported independently in `errors`, none aborts the others.
///
/// Shared by the dashboard button and the Settings「立即同步」entry — the run
/// mode decides what it means (Standalone ⇒ collect only; Synced ⇒ collect +
/// sync). The caller emits `usage_changed` after this returns.
pub fn align(store: &Store, config: &ConfigStore) -> AlignReport {
    let mut report = AlignReport::default();
    match collect_into(store, config) {
        Ok(r) => report.collected = r,
        Err(e) => report.errors.push(format!("collect: {e}")),
    }
    if config.get().is_synced() {
        // The retry loop + backoff lives in `retry_rounds` (testable with a
        // no-op sleeper); production passes the real `sync_round` + sleep.
        let outcome = retry_rounds(|| sync_round(store, config), 3, std::thread::sleep);
        report.imported = outcome.imported;
        report.pushed = outcome.pushed;
        report.errors.extend(outcome.errors);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::{retry_rounds, SyncRoundOutcome};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    // Scripted always-errors round; `imported = n` makes aggregation observable.
    fn err_round(n: u32) -> SyncRoundOutcome {
        SyncRoundOutcome {
            imported: n,
            pushed: false,
            errors: vec!["e".to_string()],
        }
    }

    /// On the first clean round we stop, having aggregated imported across the
    /// retries that ran — and we only slept between attempts that actually
    /// happened (1→2), not after the terminating clean round.
    #[test]
    fn retry_rounds_breaks_on_clean_round_and_aggregates_imported() {
        let script = [
            SyncRoundOutcome {
                imported: 5,
                pushed: false,
                errors: vec!["pull: x".to_string()],
            },
            SyncRoundOutcome {
                imported: 0,
                pushed: true,
                errors: vec![],
            },
        ];
        let idx = Cell::new(0usize);
        let sleeps = Cell::new(0u32);
        let out = retry_rounds(
            || {
                let i = idx.get();
                idx.set(i + 1);
                script[i].clone()
            },
            3,
            |_| sleeps.set(sleeps.get() + 1),
        );
        assert_eq!(idx.get(), 2, "stopped after the clean 2nd round, no 3rd");
        assert_eq!(sleeps.get(), 1, "slept once between attempts 1→2 only");
        assert_eq!(out.imported, 5, "imported aggregated across retries");
        assert!(out.pushed, "final round's pushed carried through");
        assert!(
            out.errors.is_empty(),
            "final round's clean errors carried through"
        );
    }

    /// When every round errors we exhaust all attempts, sleeping only between
    /// them (not after the last), and imported accumulates from every attempt.
    #[test]
    fn retry_rounds_exhausts_attempts_when_always_errors() {
        let calls = Cell::new(0u32);
        let sleeps = Cell::new(0u32);
        let out = retry_rounds(
            || {
                calls.set(calls.get() + 1);
                err_round(1)
            },
            3,
            |_| sleeps.set(sleeps.get() + 1),
        );
        assert_eq!(calls.get(), 3, "all 3 attempts used");
        assert_eq!(
            sleeps.get(),
            2,
            "slept between attempts only, not after the last"
        );
        assert_eq!(out.imported, 3, "1 imported per attempt × 3");
        assert_eq!(out.errors, vec!["e".to_string()]);
    }

    /// The backoff doubles (1 s, 2 s) and never fires after the final attempt.
    #[test]
    fn retry_rounds_backoff_is_1s_then_2s() {
        let sleeps: Rc<RefCell<Vec<Duration>>> = Rc::new(RefCell::new(Vec::new()));
        let cap = sleeps.clone();
        let _out = retry_rounds(|| err_round(0), 3, move |d| cap.borrow_mut().push(d));
        assert_eq!(
            *sleeps.borrow(),
            vec![Duration::from_secs(1), Duration::from_secs(2)],
            "backoff doubles (1s, 2s); nothing after the last attempt"
        );
    }
}
