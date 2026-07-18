//! Ingest pipeline (ADR-0009): RawUsage → UsageRecord (cost computed) →
//! SQLite Local Store + JSONL Artifact append.
//!
//! The provider emits `RawUsage` (no cost, no device). Here we attach the
//! owning `device_id`, derive the `day` bucket and `pricing_model`, compute cost
//! via the pure `CostCalculator`, write new rows to SQLite (ledger dedup), and
//! append the same new rows to the per-day JSONL Artifact. SQLite is the query
//! source of truth; JSONL is the human-readable backup / sync medium (ADR-0004).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::config::Paths;
use crate::db::Store;
use crate::error::AppResult;
use crate::model::UsageRecord;
use crate::pricing::{CostCalculator, PricingBook};
use crate::providers::{CollectResult, RawUsage};

/// Summary of one ingest run.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct IngestReport {
    pub source: String,
    pub events_collected: u32,
    pub rows_inserted: u32,
    pub files_scanned: u32,
    pub lines_skipped: u32,
}

/// Turn a raw provider event into a full stored record (cost + device + day).
/// Pure: given the same book, deterministic. This is the heart of ADR-0009.
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
        cost,
    }
}

/// Ingest a provider's collect result: compute cost, write new rows to SQLite,
/// append new rows to the JSONL Artifact. Returns a summary.
pub fn ingest_collected(
    store: &Store,
    paths: &Paths,
    device_id: &str,
    book: &PricingBook,
    result: CollectResult,
) -> AppResult<IngestReport> {
    let events_collected = result.events.len() as u32;
    let source = result.source.clone();

    let records: Vec<UsageRecord> = result
        .events
        .iter()
        .map(|r| recordify(r, device_id, book))
        .collect();

    // SQLite first (transactional, ledger dedup) — source of truth for queries.
    let inserted = store.ingest(&records)?;

    // JSONL append only for newly-inserted rows (no duplicates in the Artifact).
    if !inserted.is_empty() {
        append_jsonl(paths, device_id, &inserted)?;
    }

    Ok(IngestReport {
        source,
        events_collected,
        rows_inserted: inserted.len() as u32,
        files_scanned: result.files_scanned,
        lines_skipped: result.lines_skipped,
    })
}

/// Append records to the per-day JSONL Artifact (`repo/data/<deviceId>/usage-<day>.jsonl`).
/// Records are grouped by day; each line is one JSON-serialized `UsageRecord`.
/// Errors here are logged but do not undo the SQLite write (JSONL is a backup).
pub fn append_jsonl(paths: &Paths, device_id: &str, records: &[UsageRecord]) -> AppResult<()> {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<String, Vec<&UsageRecord>> = BTreeMap::new();
    for r in records {
        by_day.entry(r.day.clone()).or_default().push(r);
    }
    for (day, rows) in by_day {
        let path = paths.artifact_path(device_id, &day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match write_day(&path, &rows) {
            Ok(()) => {}
            Err(e) => eprintln!("[vaultone] jsonl append failed for {day}: {e}"),
        }
    }
    Ok(())
}

fn write_day(path: &Path, rows: &[&UsageRecord]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}

/// Read all records from a single JSONL Artifact file (used by the sync pull
/// path, ADR-0005). Skips unparseable lines.
pub fn read_jsonl_file(path: &Path) -> AppResult<Vec<UsageRecord>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<UsageRecord>(line) {
            Ok(r) => out.push(r),
            Err(_) => continue,
        }
    }
    Ok(out)
}

/// Read every JSONL Artifact for a device under `repo/data/<deviceId>/`.
pub fn read_device_artifacts(paths: &Paths, device_id: &str) -> AppResult<Vec<UsageRecord>> {
    let dir = paths.device_data_dir(device_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.extend(read_jsonl_file(&p)?);
        }
    }
    Ok(out)
}

/// Read every device's artifacts (all known devices under `repo/data/`).
pub fn read_all_artifacts(paths: &Paths) -> AppResult<Vec<UsageRecord>> {
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
                    out.extend(read_device_artifacts(paths, name)?);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ServerToolUse, TokenCounts};
    use crate::pricing::seed_book;

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
        let read = read_device_artifacts(&paths, "0123456789ab").unwrap();
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
            files_scanned: 1,
            lines_skipped: 0,
        };
        let rep1 = ingest_collected(&store, &paths, "0123456789ab", &book, result.clone()).unwrap();
        assert_eq!(rep1.rows_inserted, 1);
        assert_eq!(rep1.events_collected, 1);
        // Same uuid again ⇒ fully deduped by the ledger.
        let rep2 = ingest_collected(&store, &paths, "0123456789ab", &book, result).unwrap();
        assert_eq!(rep2.rows_inserted, 0);
    }
}
