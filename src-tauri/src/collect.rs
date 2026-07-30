//! Collect orchestration: parse Sources into the Local Store, then push the
//! resulting Artifact to the sync repo.
//!
//! `collect_into` is the single ingest path shared by the manual `collect_now`
//! command and the background scheduler. `push_if_synced` is the best-effort
//! push backstop. Collect and push are DECOUPLED here — collect is a short
//! seconds-level local cadence, push is a longer minutes-level Git cadence
//! (Synced only), so the scheduler triggers them on independent deadlines
//! rather than chaining them.

use crate::config::ConfigStore;
use crate::db::Store;
use crate::error::AppResult;
use crate::ingest::{self, IngestReport};

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
    let progress = store.load_scan_progress()?;
    let cfg = config.get();
    store.upsert_device(&cfg.device_id, &cfg.display_name, true)?;
    let book = store.load_pricing_book()?;
    let paths = config.paths();

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

/// Best-effort push of the current Artifact to the sync repo (Synced only).
/// Errors are logged, never propagated — push is a backstop.
pub fn push_if_synced(config: &ConfigStore) {
    let cfg = config.get();
    let paths = config.paths();
    crate::sync::commit_and_push_best_effort(&paths, &cfg, "vaultone: usage sync");
}
