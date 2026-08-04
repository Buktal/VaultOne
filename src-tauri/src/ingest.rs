//! Ingest pipeline: RawUsage → UsageRecord (cost computed) and RawTurnDuration
//! → TurnDuration, written to the SQLite Local Store, with the rows' days
//! flagged dirty for the push path.
//!
//! The provider emits raw per-call events + raw per-turn durations (no cost, no
//! device). Here we attach the owning device_id, derive the day bucket and
//! pricing_model, compute cost via the pure CostCalculator, and write the new
//! rows to SQLite (deduped by the `(uuid, device_id)` primary key). The JSONL
//! Artifact is a derived snapshot the push path recomputes from the store —
//! collect never touches it (see `recompute_usage_day` / `recompute_turns_day`).

use std::path::Path;

use crate::config::Paths;
use crate::db::Store;
use crate::error::AppResult;
use crate::model::{TurnDuration, UsageRecord};
use crate::pricing::{CostCalculator, PricingBook};
use crate::providers::{CollectResult, RawTurnDuration, RawUsage};

/// Summary of one ingest run.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct IngestReport {
    pub source: String,
    pub events_collected: u32,
    pub rows_inserted: u32,
    pub turn_durations_collected: u32,
    pub turn_durations_inserted: u32,
    pub files_scanned: u32,
    pub lines_skipped: u32,
}

/// Turn a raw per-call event into a full stored record (cost + device + day).
/// Pure: given the same book, deterministic.
pub fn recordify(raw: &RawUsage, device_id: &str, book: &PricingBook) -> UsageRecord {
    let pricing_model = crate::model::normalize_pricing_key(&raw.model);
    let rate = book.resolve(&raw.model);
    let cost = CostCalculator::calc(raw.tokens, rate);
    UsageRecord {
        uuid: raw.uuid.clone(),
        day: UsageRecord::day_from_timestamp(&raw.timestamp),
        timestamp: raw.timestamp.clone(),
        model: raw.model.clone(),
        pricing_model,
        source: raw.source.clone(),
        device_id: device_id.to_string(),
        tokens: raw.tokens,
        server_tool_use: raw.server_tool_use,
        stop_reason: raw.stop_reason.clone(),
        service_tier: raw.service_tier.clone(),
        iterations: raw.iterations,
        cost,
    }
}

/// Turn a raw per-turn duration into a stored TurnDuration (attach device + day).
pub fn turn_durationify(raw: &RawTurnDuration, device_id: &str) -> TurnDuration {
    TurnDuration {
        uuid: raw.uuid.clone(),
        day: UsageRecord::day_from_timestamp(&raw.timestamp),
        timestamp: raw.timestamp.clone(),
        device_id: device_id.to_string(),
        duration_ms: raw.duration_ms,
    }
}

/// Ingest a provider's collect result: compute cost + day, then write the rows
/// to the SQLite Local Store, flagging each new row's day dirty in the same
/// transaction. The JSONL Artifact is NOT touched here — it is now a derived
/// snapshot the push path recomputes from the store per dirty day (see
/// [`recompute_usage_day`] / [`recompute_turns_day`]). SQLite is the single
/// source of truth; the scan cursor advances only after the ingest commits, so
/// a failed ingest re-parses the same source lines next collect (store dedup).
/// Returns a summary.
pub fn ingest_collected(
    store: &Store,
    device_id: &str,
    book: &PricingBook,
    result: CollectResult,
) -> AppResult<IngestReport> {
    let events_collected = result.events.len() as u32;
    let turn_durations_collected = result.turn_durations.len() as u32;
    let source = result.source.clone();

    // Per-call usage records → store (+ mark their days dirty, same tx).
    let records: Vec<UsageRecord> = result
        .events
        .iter()
        .map(|r| recordify(r, device_id, book))
        .collect();
    let inserted = store.ingest_marking_dirty(&records)?;

    // Per-turn durations (separate grain) → store (+ mark dirty, same tx).
    let turns: Vec<TurnDuration> = result
        .turn_durations
        .iter()
        .map(|t| turn_durationify(t, device_id))
        .collect();
    let turns_inserted = store.ingest_turn_durations_marking_dirty(&turns)?;

    Ok(IngestReport {
        source,
        events_collected,
        rows_inserted: inserted.len() as u32,
        turn_durations_collected,
        turn_durations_inserted: turns_inserted.len() as u32,
        files_scanned: result.files_scanned,
        lines_skipped: result.lines_skipped,
    })
}

// ---------------- Per-day JSONL Artifact (derived snapshot) ----------------
//
// Two grains share this machinery: per-call UsageRecord (`usage-<day>.jsonl`)
// and per-turn TurnDuration (`turns-<day>.jsonl`). The Artifact is a DERIVED
// snapshot of the store: collect only writes SQLite (+ marks days dirty); the
// push path rewrites each dirty day's file from the store
// (`recompute_usage_day` / `recompute_turns_day`), and pull reads peers' files
// back into the store. Only the row type, file-name prefix, and day accessor
// differ — captured by [`ArtifactGrain`] so the policy lives in one place.

/// One JSONL Artifact grain: its row type and file-name prefix. The per-day
/// split is driven by the `day` column in SQL (`usage_for_day_device`), not by a
/// trait method, so the trait stays minimal.
trait ArtifactGrain {
    type Row: serde::Serialize + serde::de::DeserializeOwned;
    /// File-name prefix; the Artifact is `<prefix>-<day>.jsonl`.
    const PREFIX: &'static str;
}

/// Per-call usage records → `usage-<day>.jsonl`.
struct UsageGrain;
impl ArtifactGrain for UsageGrain {
    type Row = UsageRecord;
    const PREFIX: &'static str = "usage";
}

/// Per-turn durations → `turns-<day>.jsonl`.
struct TurnGrain;
impl ArtifactGrain for TurnGrain {
    type Row = TurnDuration;
    const PREFIX: &'static str = "turns";
}

/// `<device_data_dir>/<deviceId>/<prefix>-<day>.jsonl`.
fn day_path<A: ArtifactGrain>(paths: &Paths, device_id: &str, day: &str) -> std::path::PathBuf {
    paths
        .device_data_dir(device_id)
        .join(format!("{}-{day}.jsonl", A::PREFIX))
}

/// Full rewrite of one per-day Artifact file from its rows: truncate and write
/// every row as its own JSON line. Byte-stable by construction — the caller
/// supplies the rows in a deterministic order (`ORDER BY uuid`) and serde emits
/// fields in declaration order, so the same rows always serialize to the same
/// bytes (no git churn once a day is settled). An empty row set means the device
/// has no data for this day, so the file is removed rather than left empty.
fn rewrite_day_file<T: serde::Serialize>(path: &Path, rows: &[T]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if rows.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let mut out = String::new();
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Recompute one device's per-day usage Artifact from the store: every
/// `usage_records` row for (day, device) in uuid order, as a full file rewrite.
/// The push-side writer — collect no longer touches the Artifact; the store is
/// the single source of truth and this materializes the derived snapshot a peer
/// pulls. Byte-stable across pushes (uuid order + field declaration order).
/// Returns the row count — the caller (push) uses it as the recompute-time
/// snapshot to decide whether the day is still clearable after the push lands.
pub fn recompute_usage_day(
    store: &Store,
    paths: &Paths,
    device_id: &str,
    day: &str,
) -> AppResult<usize> {
    let rows = store.usage_for_day_device(day, device_id)?;
    rewrite_day_file(&day_path::<UsageGrain>(paths, device_id, day), &rows)?;
    Ok(rows.len())
}

/// Recompute one device's per-day turn-duration Artifact from the store (mirrors
/// [`recompute_usage_day`] for the per-turn grain; same row-count snapshot role).
pub fn recompute_turns_day(
    store: &Store,
    paths: &Paths,
    device_id: &str,
    day: &str,
) -> AppResult<usize> {
    let rows = store.turns_for_day_device(day, device_id)?;
    rewrite_day_file(&day_path::<TurnGrain>(paths, device_id, day), &rows)?;
    Ok(rows.len())
}

// ---------------- Test-only Artifact append fixtures ----------------
//
// collect no longer appends to the Artifact — the push path rewrites it from
// the store. These idempotent append helpers survive only as test fixtures
// (e.g. the sync round-trip tests stand up a device's Artifact directly to
// exercise pull/import without driving a full collect+push), so they are
// `#[cfg(test)]`.

/// Open once in append mode, serialize + writeln each row. Test fixture only.
#[cfg(test)]
fn write_jsonl_day<T: serde::Serialize>(path: &Path, rows: &[&T]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}

/// Stand up a device's usage Artifact for the sync round-trip tests — group the
/// records by day and append each day's file idempotently (a row already in the
/// file is skipped). Test fixture only: production writes the Artifact via
/// [`recompute_usage_day`], not append.
#[cfg(test)]
pub(crate) fn append_jsonl(
    paths: &Paths,
    device_id: &str,
    records: &[UsageRecord],
) -> AppResult<()> {
    use std::collections::{BTreeMap, HashSet};
    let mut by_day: BTreeMap<String, Vec<&UsageRecord>> = BTreeMap::new();
    for r in records {
        by_day.entry(r.day.clone()).or_default().push(r);
    }
    for (day, day_rows) in by_day {
        let path = day_path::<UsageGrain>(paths, device_id, &day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing: HashSet<String> = read_jsonl_file_of::<UsageRecord>(&path)
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.uuid)
            .collect();
        let missing: Vec<&UsageRecord> = day_rows
            .into_iter()
            .filter(|r| !existing.contains(&r.uuid))
            .collect();
        if missing.is_empty() {
            continue;
        }
        write_jsonl_day(&path, &missing)?;
    }
    Ok(())
}

/// Read every row from one JSONL Artifact file. Unparseable lines are skipped.
fn read_jsonl_file_of<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<Vec<T>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(r) = serde_json::from_str::<T>(line) {
            out.push(r);
        }
    }
    Ok(out)
}

/// `<prefix>-*.jsonl` under the device dir?
fn is_artifact_of<A: ArtifactGrain>(path: &Path) -> bool {
    let prefix_dash = format!("{}-", A::PREFIX);
    path.extension().and_then(|e| e.to_str()) == Some("jsonl")
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with(&prefix_dash))
            .unwrap_or(false)
}

/// Read every `<prefix>-*.jsonl` Artifact for one device.
fn read_device_artifacts_of<A: ArtifactGrain>(
    paths: &Paths,
    device_id: &str,
) -> AppResult<Vec<A::Row>> {
    let dir = paths.device_data_dir(device_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if is_artifact_of::<A>(&p) {
            out.extend(read_jsonl_file_of::<A::Row>(&p)?);
        }
    }
    Ok(out)
}

/// Read every device's `<prefix>-*.jsonl` Artifacts (all known devices).
fn read_all_artifacts_of<A: ArtifactGrain>(paths: &Paths) -> AppResult<Vec<A::Row>> {
    let root = &paths.repo_data;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if crate::config::is_valid_device_id(name) {
                    out.extend(read_device_artifacts_of::<A>(paths, name)?);
                }
            }
        }
    }
    Ok(out)
}

// Typed read entry points (production — pull imports peers' Artifacts through
// these). The append fixture lives in the test-only block above.

/// Read every device's usage artifacts.
pub fn read_all_artifacts(paths: &Paths) -> AppResult<Vec<UsageRecord>> {
    read_all_artifacts_of::<UsageGrain>(paths)
}

/// Read every device's turn-duration artifacts.
pub fn read_all_turn_artifacts(paths: &Paths) -> AppResult<Vec<TurnDuration>> {
    read_all_artifacts_of::<TurnGrain>(paths)
}

// The device-name artifact I/O (`ensure_own_device_artifact` /
// `read_all_device_artifacts`) lived here once; it moved to `crate::devices`,
// the registry module that owns device membership + naming + the name
// artifact. The per-call/per-turn JSONL Artifact machinery above
// (`ArtifactGrain`) is a separate concern and stays here.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ServerToolUse, TokenCounts};
    use crate::pricing::seed_book;
    use crate::providers::RawTurnDuration;

    fn raw(uuid: &str, model: &str) -> RawUsage {
        RawUsage {
            uuid: uuid.into(),
            timestamp: "2026-07-13T16:55:22.467Z".into(),
            model: model.into(),
            source: "claude_code".into(),
            tokens: TokenCounts {
                input: 1000,
                output: 500,
                cache_creation: 0,
                cache_read: 0,
            },
            server_tool_use: ServerToolUse::default(),
            stop_reason: "end_turn".into(),
            service_tier: "standard".into(),
            iterations: 0,
        }
    }

    fn raw_turn(uuid: &str) -> RawTurnDuration {
        RawTurnDuration {
            uuid: uuid.into(),
            timestamp: "2026-07-13T16:55:00Z".into(),
            duration_ms: 123_456,
        }
    }

    #[test]
    fn recordify_attaches_day_pricing_model_and_cost() {
        let book = seed_book();
        let r = recordify(&raw("u1", "glm-5.2[1m]"), "0123456789ab", &book);
        assert_eq!(r.uuid, "u1");
        assert_eq!(r.device_id, "0123456789ab");
        assert_eq!(r.day, "2026-07-13");
        assert_eq!(
            r.pricing_model, "glm-5.2",
            "bracket stripped for pricing lookup"
        );
        assert_eq!(r.model, "glm-5.2[1m]", "original billed model preserved");
        // New per-call fields pass through.
        assert_eq!(r.stop_reason, "end_turn");
        assert_eq!(r.service_tier, "standard");
        // glm-5.2: input 0.60/1M × 1000 + output 2.20/1M × 500 = 0.0006 + 0.0011.
        assert!(
            (r.cost.total_f64() - 0.0017).abs() < 1e-9,
            "cost = {}",
            r.cost.total_f64()
        );
    }

    #[test]
    fn recordify_is_zero_cost_for_unknown_model() {
        let book = seed_book();
        let r = recordify(&raw("u2", "no-such-model"), "0123456789ab", &book);
        assert_eq!(r.cost.total_f64(), 0.0);
    }

    /// The collect path flags the days of newly ingested rows dirty (in the same
    /// tx as the write) AND leaves the Artifact unwritten — the store is the
    /// single source of truth now; the push path materializes files. Proves both:
    /// days flagged, and no file appears from collect.
    #[test]
    fn ingest_collected_flags_dirty_days_and_writes_no_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let dev = "0123456789ab";
        let d1 = raw("u1", "glm-5.2");
        let d2 = RawUsage {
            timestamp: "2026-07-14T16:55:22.467Z".into(),
            ..raw("u2", "glm-5.2")
        };
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![d1, d2],
            turn_durations: vec![raw_turn("td1")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, dev, &book, result).unwrap();
        assert_eq!(
            store.dirty_days().unwrap(),
            vec!["2026-07-13".to_string(), "2026-07-14".to_string()],
            "D1 (usage + turn) and D2 flagged, deduped + sorted"
        );
        // collect writes the store, NOT the Artifact — no file exists yet.
        assert!(
            !paths
                .device_data_dir(dev)
                .join("usage-2026-07-13.jsonl")
                .exists(),
            "collect must not write the Artifact (push recomputes it)"
        );
    }

    #[test]
    fn ingest_collected_dedups_via_store_pk() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("dup", "glm-5.2")],
            turn_durations: vec![raw_turn("td1")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        let rep1 = ingest_collected(&store, "0123456789ab", &book, result.clone()).unwrap();
        assert_eq!(rep1.rows_inserted, 1);
        assert_eq!(rep1.events_collected, 1);
        assert_eq!(rep1.turn_durations_collected, 1);
        assert_eq!(rep1.turn_durations_inserted, 1);
        // Same uuids again ⇒ fully deduped.
        let rep2 = ingest_collected(&store, "0123456789ab", &book, result).unwrap();
        assert_eq!(rep2.rows_inserted, 0);
        assert_eq!(rep2.turn_durations_inserted, 0);
    }

    /// Recompute is byte-stable: the same store yields identical file bytes every
    /// time, and rows land in uuid order (not collect order). This is what keeps
    /// a settled day from churning git across pushes.
    #[test]
    fn recompute_usage_day_is_byte_stable_and_uuid_ordered() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let dev = "0123456789ab";
        // Ingest "zzz" before "aaa": collect order is unstable, uuid order fixed.
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("zzz", "glm-5.2"), raw("aaa", "glm-5.2")],
            turn_durations: vec![],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, dev, &book, result).unwrap();
        let day_file = paths.device_data_dir(dev).join("usage-2026-07-13.jsonl");

        recompute_usage_day(&store, &paths, dev, "2026-07-13").unwrap();
        let bytes1 = std::fs::read(&day_file).unwrap();
        let text = String::from_utf8(bytes1.clone()).unwrap();
        assert!(
            text.find("\"aaa\"").unwrap() < text.find("\"zzz\"").unwrap(),
            "rows emitted in uuid order, not collect order"
        );

        // Recompute again ⇒ identical bytes (idempotent / byte-stable).
        recompute_usage_day(&store, &paths, dev, "2026-07-13").unwrap();
        let bytes2 = std::fs::read(&day_file).unwrap();
        assert_eq!(bytes1, bytes2, "recompute is byte-stable across calls");
    }

    /// collect leaves the Artifact unwritten; recompute materializes the day's
    /// full content from the store (the push step). Also covers gap self-heal: a
    /// row in the store but absent from the file is filled by recompute.
    #[test]
    fn recompute_materializes_the_day_collect_left_unwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let dev = "0123456789ab";
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("a", "glm-5.2")],
            turn_durations: vec![],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, dev, &book, result).unwrap();
        let day_file = paths.device_data_dir(dev).join("usage-2026-07-13.jsonl");
        assert!(!day_file.exists(), "collect does not write the Artifact");
        recompute_usage_day(&store, &paths, dev, "2026-07-13").unwrap();
        let read = read_device_artifacts_of::<UsageGrain>(&paths, dev).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].uuid, "a");
    }

    /// usage and turns are separate grains/files; recomputing a day writes each,
    /// each holding only its own grain (usage read never picks up turns, etc.).
    #[test]
    fn recompute_keeps_usage_and_turn_grains_separate() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let dev = "0123456789ab";
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("a", "glm-5.2")],
            turn_durations: vec![raw_turn("td1"), raw_turn("td2")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, dev, &book, result).unwrap();
        recompute_usage_day(&store, &paths, dev, "2026-07-13").unwrap();
        recompute_turns_day(&store, &paths, dev, "2026-07-13").unwrap();
        let usage = read_device_artifacts_of::<UsageGrain>(&paths, dev).unwrap();
        let turns = read_device_artifacts_of::<TurnGrain>(&paths, dev).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(turns.len(), 2);
    }

    /// A day with no store rows for the device ⇒ recompute removes any stale file
    /// rather than leaving an empty Artifact behind.
    #[test]
    fn recompute_drops_a_day_file_with_no_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let dev = "0123456789ab";
        let day_file = paths.device_data_dir(dev).join("usage-2026-07-13.jsonl");
        std::fs::create_dir_all(day_file.parent().unwrap()).unwrap();
        std::fs::write(&day_file, "stale\n").unwrap();
        // No rows in the store for this day/device ⇒ recompute clears the file.
        recompute_usage_day(&store, &paths, dev, "2026-07-13").unwrap();
        assert!(!day_file.exists(), "empty day ⇒ stale file removed");
    }
}
