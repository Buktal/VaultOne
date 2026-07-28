//! Ingest pipeline: RawUsage → UsageRecord (cost computed) and RawTurnDuration
//! → TurnDuration, each written to the SQLite Local Store + a per-day JSONL
//! Artifact.
//!
//! The provider emits raw per-call events + raw per-turn durations (no cost, no
//! device). Here we attach the owning device_id, derive the day bucket and
//! pricing_model, compute cost via the pure CostCalculator, write new rows to
//! SQLite (ledger dedup), and append the same new rows to per-day JSONL
//! Artifacts. SQLite is the query source of truth; JSONL is the human-readable
//! backup / sync medium.

use std::fs::OpenOptions;
use std::io::Write;
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
    let pricing_model = crate::pricing::normalize_key(&raw.model);
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

/// Ingest a provider's collect result: compute cost, write new rows to SQLite,
/// append new rows to the JSONL Artifacts. Returns a summary.
pub fn ingest_collected(
    store: &Store,
    paths: &Paths,
    device_id: &str,
    book: &PricingBook,
    result: CollectResult,
) -> AppResult<IngestReport> {
    let events_collected = result.events.len() as u32;
    let turn_durations_collected = result.turn_durations.len() as u32;
    let source = result.source.clone();

    // Per-call usage records: SQLite first (transactional, ledger dedup).
    let records: Vec<UsageRecord> = result
        .events
        .iter()
        .map(|r| recordify(r, device_id, book))
        .collect();
    let inserted = store.ingest(&records)?;
    if !inserted.is_empty() {
        append_jsonl(paths, device_id, &inserted)?;
    }

    // Per-turn durations (separate grain, dedup by uuid).
    let turns: Vec<TurnDuration> = result
        .turn_durations
        .iter()
        .map(|t| turn_durationify(t, device_id))
        .collect();
    let turns_inserted = if turns.is_empty() {
        Vec::new()
    } else {
        // Only newly-inserted turns are appended to the JSONL — mirroring the
        // usage path above. Previously ALL turns were re-appended each collect,
        // duplicating them in the Artifact under full rescans.
        let inserted = store.ingest_turn_durations(&turns)?;
        if !inserted.is_empty() {
            append_turn_jsonl(paths, device_id, &inserted)?;
        }
        inserted
    };

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

// ---------------- Per-day JSONL Artifact (generic over the grain) ----------------
//
// Two grains share this machinery: per-call UsageRecord (`usage-<day>.jsonl`)
// and per-turn TurnDuration (`turns-<day>.jsonl`). Both append grouped by day,
// both skip unparseable lines on read, both leave the SQLite write intact when
// the JSONL append fails (JSONL is the backup medium). Only the row type, the
// file-name prefix, and the day accessor differ — captured by [`ArtifactGrain`]
// so the policy lives in one place.

/// One JSONL Artifact grain: its row type, file-name prefix, diagnostic label,
/// and the day bucket that drives the per-day file split.
trait ArtifactGrain {
    type Row: serde::Serialize + serde::de::DeserializeOwned;
    /// File-name prefix; the Artifact is `<prefix>-<day>.jsonl`.
    const PREFIX: &'static str;
    /// Label for append-failure log lines.
    const LABEL: &'static str;
    /// Day bucket this row belongs to.
    fn day(row: &Self::Row) -> &str;
}

/// Per-call usage records → `usage-<day>.jsonl`.
struct UsageGrain;
impl ArtifactGrain for UsageGrain {
    type Row = UsageRecord;
    const PREFIX: &'static str = "usage";
    const LABEL: &'static str = "jsonl";
    fn day(r: &UsageRecord) -> &str {
        &r.day
    }
}

/// Per-turn durations → `turns-<day>.jsonl`.
struct TurnGrain;
impl ArtifactGrain for TurnGrain {
    type Row = TurnDuration;
    const PREFIX: &'static str = "turns";
    const LABEL: &'static str = "turn jsonl";
    fn day(t: &TurnDuration) -> &str {
        &t.day
    }
}

/// `<device_data_dir>/<deviceId>/<prefix>-<day>.jsonl`.
fn day_path<A: ArtifactGrain>(paths: &Paths, device_id: &str, day: &str) -> std::path::PathBuf {
    paths
        .device_data_dir(device_id)
        .join(format!("{}-{day}.jsonl", A::PREFIX))
}

/// Open once in append mode, serialize + writeln each row.
fn write_jsonl_day<T: serde::Serialize>(path: &Path, rows: &[&T]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}

/// Group rows by day and append each day's file. Append errors are logged but
/// do NOT undo the caller's SQLite write (JSONL is a backup medium).
fn append_artifact_jsonl<A: ArtifactGrain>(
    paths: &Paths,
    device_id: &str,
    rows: &[A::Row],
) -> AppResult<()> {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<String, Vec<&A::Row>> = BTreeMap::new();
    for r in rows {
        by_day.entry(A::day(r).to_string()).or_default().push(r);
    }
    for (day, day_rows) in by_day {
        let path = day_path::<A>(paths, device_id, &day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Err(e) = write_jsonl_day(&path, &day_rows) {
            eprintln!("[vaultone] {} append failed for {day}: {e}", A::LABEL);
        }
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

// Typed entry points (stable public API; each delegates to the generic core).

/// Append usage records to the per-day Artifact.
pub fn append_jsonl(paths: &Paths, device_id: &str, records: &[UsageRecord]) -> AppResult<()> {
    append_artifact_jsonl::<UsageGrain>(paths, device_id, records)
}

/// Append turn durations to the per-day Artifact.
pub fn append_turn_jsonl(
    paths: &Paths,
    device_id: &str,
    turns: &[TurnDuration],
) -> AppResult<()> {
    append_artifact_jsonl::<TurnGrain>(paths, device_id, turns)
}

/// Read every device's usage artifacts.
pub fn read_all_artifacts(paths: &Paths) -> AppResult<Vec<UsageRecord>> {
    read_all_artifacts_of::<UsageGrain>(paths)
}

/// Read every device's turn-duration artifacts.
pub fn read_all_turn_artifacts(paths: &Paths) -> AppResult<Vec<TurnDuration>> {
    read_all_artifacts_of::<TurnGrain>(paths)
}

// ---------------- DeviceArtifact (device-name sync, one file per device) ----

/// Idempotently publish THIS device's identity to `config/devices/<id>.json`
/// (device-name sync ADR). Writes only when the file is missing or its
/// `display_name` is stale, so repeated calls (boot, every sync) don't churn
/// the worktree. `first_seen` is preserved across rewrites. Returns whether a
/// write actually happened.
///
/// No network: the file is merely staged in the worktree — the normal Git sync
/// (`commit_all` + `push`) carries the whole repo, so this file rides along.
pub fn ensure_own_device_artifact(
    paths: &Paths,
    device_id: &str,
    display_name: &str,
) -> AppResult<bool> {
    // Flat layout: repo/config/devices_<id>.json (no devices/ subdir).
    let path = paths.devices_file_path(device_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    // Preserve first_seen across rewrites; seed on first publish.
    let first_seen = existing
        .as_deref()
        .and_then(|t| serde_json::from_str::<crate::model::DeviceArtifact>(t).ok())
        .map(|a| a.first_seen)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let artifact = crate::model::DeviceArtifact {
        device_id: device_id.to_string(),
        display_name: display_name.to_string(),
        first_seen,
    };
    let desired = serde_json::to_string_pretty(&artifact)?;
    if existing.as_deref().map(str::trim_end) == Some(desired.as_str()) {
        return Ok(false);
    }
    std::fs::write(&path, format!("{desired}\n"))?;
    Ok(true)
}

/// Read every device's identity artifact under `config/devices/`. Skips entries
/// whose stem isn't a valid 12-hex device id and files that fail to parse, so a
/// stray/broken file never blocks the rest from loading.
pub fn read_all_device_artifacts(paths: &Paths) -> Vec<crate::model::DeviceArtifact> {
    let mut out: Vec<crate::model::DeviceArtifact> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // New flat layout: config/devices_<id>.json. Strip the `devices_` prefix
    // and `.json` suffix; the remainder must be a valid device id.
    if let Ok(entries) = std::fs::read_dir(&paths.repo_config) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(id) = name
                .strip_prefix("devices_")
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if !crate::config::is_valid_device_id(id) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(a) = serde_json::from_str::<crate::model::DeviceArtifact>(&text) {
                    if seen.insert(a.device_id.clone()) {
                        out.push(a);
                    }
                }
            }
        }
    }

    // Legacy layout: config/devices/<id>.json (read-only fallback; new wins).
    if let Ok(entries) = std::fs::read_dir(paths.legacy_devices_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !crate::config::is_valid_device_id(stem) || seen.contains(stem) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(a) = serde_json::from_str::<crate::model::DeviceArtifact>(&text) {
                    if seen.insert(a.device_id.clone()) {
                        out.push(a);
                    }
                }
            }
        }
    }

    out
}

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

    #[test]
    fn jsonl_append_then_read_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let book = seed_book();
        let r1 = recordify(&raw("a", "glm-5.2"), "0123456789ab", &book);
        let r2 = recordify(&raw("b", "glm-5.2"), "0123456789ab", &book);
        append_jsonl(&paths, "0123456789ab", &[r1, r2]).unwrap();
        let read = read_device_artifacts_of::<UsageGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].uuid, "a");
        assert_eq!(read[1].uuid, "b");
    }

    #[test]
    fn ingest_collected_dedups_via_store_ledger() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("dup", "glm-5.2")],
            turn_durations: vec![raw_turn("td1")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        let rep1 = ingest_collected(&store, &paths, "0123456789ab", &book, result.clone()).unwrap();
        assert_eq!(rep1.rows_inserted, 1);
        assert_eq!(rep1.events_collected, 1);
        assert_eq!(rep1.turn_durations_collected, 1);
        assert_eq!(rep1.turn_durations_inserted, 1);
        // Same uuids again ⇒ fully deduped.
        let rep2 = ingest_collected(&store, &paths, "0123456789ab", &book, result).unwrap();
        assert_eq!(rep2.rows_inserted, 0);
        assert_eq!(rep2.turn_durations_inserted, 0);
    }

    #[test]
    fn turn_artifacts_round_trip_separately_from_usage() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("a", "glm-5.2")],
            turn_durations: vec![raw_turn("td1"), raw_turn("td2")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, &paths, "0123456789ab", &book, result).unwrap();
        // usage read must NOT pick up turns-*.jsonl, and vice versa.
        let usage = read_device_artifacts_of::<UsageGrain>(&paths, "0123456789ab").unwrap();
        let turns = read_device_artifacts_of::<TurnGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(turns.len(), 2);
    }

    #[test]
    fn turn_jsonl_appends_only_new_turns_on_reingest() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![],
            turn_durations: vec![raw_turn("td1"), raw_turn("td2")],
            files_scanned: 1,
            lines_skipped: 0,
        };
        ingest_collected(&store, &paths, "0123456789ab", &book, result.clone()).unwrap();
        // Re-ingest the SAME turns: DB dedups (inserted == 0) and the JSONL
        // Artifact must NOT gain duplicate rows (regression: previously every
        // turn was re-appended each collect under full rescans).
        let rep = ingest_collected(&store, &paths, "0123456789ab", &book, result).unwrap();
        assert_eq!(rep.turn_durations_inserted, 0);
        let turns = read_device_artifacts_of::<TurnGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(turns.len(), 2, "JSONL holds each turn once, not doubled");
    }

    #[test]
    fn device_artifact_flat_layout_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Writes to the new flat path (config/devices_<id>.json).
        assert!(ensure_own_device_artifact(&paths, "0123456789ab", "Laptop").unwrap());
        // Idempotent: identical content ⇒ no rewrite.
        assert!(!ensure_own_device_artifact(&paths, "0123456789ab", "Laptop").unwrap());
        // Reads back from the flat path.
        let read = read_all_device_artifacts(&paths);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].device_id, "0123456789ab");
        assert_eq!(read[0].display_name, "Laptop");
        // Path is flat — no legacy devices/ subdir was created.
        assert!(paths.devices_file_path("0123456789ab").exists());
        assert!(!paths
            .legacy_devices_dir()
            .join("0123456789ab.json")
            .exists());
    }

    #[test]
    fn read_all_device_artifacts_reads_legacy_layout_too() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Seed a legacy file under config/devices/<id>.json (old layout peer).
        let legacy = paths.legacy_devices_dir().join("abcdef012345.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(
            &legacy,
            r#"{"device_id":"abcdef012345","display_name":"OldPeer","first_seen":"2026-01-01T00:00:00.000Z"}"#,
        )
        .unwrap();
        // And a flat file for a different device (new layout).
        ensure_own_device_artifact(&paths, "0123456789ab", "NewPeer").unwrap();

        let mut ids: Vec<String> = read_all_device_artifacts(&paths)
            .into_iter()
            .map(|a| a.device_id)
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["0123456789ab".to_string(), "abcdef012345".to_string()],
            "both layouts are read"
        );
    }
}
