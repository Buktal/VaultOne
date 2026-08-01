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

use std::time::Duration;

use crate::config::ConfigStore;
use crate::db::Store;
use crate::error::AppResult;
use crate::ingest::{self, IngestReport};
use crate::sync;

/// Parse Source → Local Store (+ JSONL Artifact). No network.
/// Shared by the manual `collect_now` command and the background scheduler so
/// both follow the exact same ingest path.
///
/// Iterates every enabled provider. The per-file cursor table is loaded once
/// and shared (keys are file paths, disjoint across providers); each provider's
/// cursor advances are merged and persisted AFTER all ingests — so a failed
/// ingest leaves cursors untouched (next collect re-parses the same lines; the
/// ledger dedups). First run / empty table ⇒ full scan.
pub fn collect_into(store: &Store, config: &ConfigStore) -> AppResult<IngestReport> {
    let providers = crate::providers::all_providers()?;
    let cfg = config.get();
    let paths = config.paths();
    // Backfill any Artifact gaps left by a pre-1.5.1 append failure before
    // loading the scan cursors: if the SQLite store holds rows the JSONL
    // Artifact is missing, clear the cursors so this collect is a full rescan
    // that re-appends them (idempotently). No-op when store and Artifact agree.
    crate::ingest::reconcile_artifact_gaps(store, &paths, &cfg.device_id)?;
    let progress = store.load_scan_progress()?;
    store.upsert_device(&cfg.device_id, &cfg.display_name, true)?;
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
    crate::sync::reconcile_devices(store, &paths, &cfg)?;
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
    match sync::commit_and_push(&paths, &cfg, "vaultone: usage sync") {
        Ok(p) => out.pushed = p,
        Err(e) => out.errors.push(format!("push: {e}")),
    }
    out
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
        let mut last = SyncRoundOutcome::default();
        // Sum imported across retries: pull is uuid-deduped, so a row pulled on
        // attempt 1 (then lost to a push failure) reads 0 on attempt 2 — taking
        // only `last.imported` would report "0 imported" despite real new rows.
        let mut imported = 0u32;
        for attempt in 0u32..3 {
            last = sync_round(store, config);
            imported += last.imported;
            if last.errors.is_empty() {
                break;
            }
            // Back off before the next attempt (1 s, 2 s); skip after the last.
            if attempt + 1 < 3 {
                std::thread::sleep(Duration::from_secs(1 << attempt));
            }
        }
        report.imported = imported;
        report.pushed = last.pushed;
        report.errors.extend(last.errors);
    }
    report
}
