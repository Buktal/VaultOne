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
use crate::model::{RawSession, SessionMessage, SessionMetaSync, SessionSystemData, TurnDuration, UsageRecord};
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
        session_id: raw.session_id.clone(),
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

/// Ingest a provider's collect result: compute cost, append the rows to the
/// JSONL Artifacts, then write them to SQLite. JSONL is written first so a
/// failed append aborts before SQLite commits — the scan cursor stays put and
/// the next collect re-parses these same source lines (the ledger dedups).
/// Returns a summary.
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

    // Per-call usage records. JSONL first, then SQLite: the Artifact is the
    // sync medium peers pull, so a failed append must abort the whole ingest
    // before SQLite commits — otherwise the ledger dedup would silence the
    // failure and permanently drop those rows from the Artifact. The append is
    // idempotent, so a retried collect after a later SQLite failure adds no
    // duplicate.
    let records: Vec<UsageRecord> = result
        .events
        .iter()
        .map(|r| recordify(r, device_id, book))
        .collect();
    append_jsonl(paths, device_id, &records)?;
    let inserted = store.ingest(&records)?;

    // Per-turn durations (separate grain). Same JSONL-first ordering as the
    // usage path above, for the same reason.
    let turns: Vec<TurnDuration> = result
        .turn_durations
        .iter()
        .map(|t| turn_durationify(t, device_id))
        .collect();
    let turns_inserted = if turns.is_empty() {
        Vec::new()
    } else {
        append_turn_jsonl(paths, device_id, &turns)?;
        store.ingest_turn_durations(&turns)?
    };

    // Sessions (Claude only in this phase; empty for other sources). Writes the
    // session-meta grain + refreshes the sessions table (system data; user data
    // preserved by UPSERT) + appends transcripts for favorited sessions.
    ingest_sessions(store, paths, device_id, &result.sessions, &result.messages)?;

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
// both skip unparseable lines on read, and both treat the JSONL Artifact as the
// sync medium: appends are idempotent and a failure propagates. The caller
// writes JSONL before SQLite so a failed append leaves the scan cursor — and
// thus a source re-scan — to recover it. Only the row type, file-name prefix,
// and day accessor differ — captured by [`ArtifactGrain`] so the policy lives
// in one place.

/// One JSONL Artifact grain: its row type, file-name prefix, diagnostic label,
/// and the day bucket that drives the per-day file split.
trait ArtifactGrain {
    type Row: serde::Serialize + serde::de::DeserializeOwned;
    /// File-name prefix; the Artifact is `<prefix>-<day>.jsonl`.
    const PREFIX: &'static str;
    /// Day bucket this row belongs to.
    fn day(row: &Self::Row) -> &str;
    /// Dedup key. A row whose uuid is already in the day's file is skipped on
    /// append, so a retried collect (scan cursor unchanged) writes no duplicate.
    fn uuid(row: &Self::Row) -> &str;
}

/// Per-call usage records → `usage-<day>.jsonl`.
struct UsageGrain;
impl ArtifactGrain for UsageGrain {
    type Row = UsageRecord;
    const PREFIX: &'static str = "usage";
    fn day(r: &UsageRecord) -> &str {
        &r.day
    }
    fn uuid(r: &UsageRecord) -> &str {
        &r.uuid
    }
}

/// Per-turn durations → `turns-<day>.jsonl`.
struct TurnGrain;
impl ArtifactGrain for TurnGrain {
    type Row = TurnDuration;
    const PREFIX: &'static str = "turns";
    fn day(t: &TurnDuration) -> &str {
        &t.day
    }
    fn uuid(t: &TurnDuration) -> &str {
        &t.uuid
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

/// Group rows by day and append each day's file. Idempotent: a row whose uuid
/// is already in the day's file is skipped, so a retried collect (scan cursor
/// unchanged after a failure) writes no duplicate. An append error propagates —
/// the JSONL Artifact is the sync medium peers pull, so a row missing here is a
/// row the other devices never see; surfacing the error leaves the scan cursor
/// untouched so the next collect re-parses the same source lines.
fn append_artifact_jsonl<A: ArtifactGrain>(
    paths: &Paths,
    device_id: &str,
    rows: &[A::Row],
) -> AppResult<()> {
    use std::collections::{BTreeMap, HashSet};
    let mut by_day: BTreeMap<String, Vec<&A::Row>> = BTreeMap::new();
    for r in rows {
        by_day.entry(A::day(r).to_string()).or_default().push(r);
    }
    for (day, day_rows) in by_day {
        let path = day_path::<A>(paths, device_id, &day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Idempotent: keep only rows this day's file does not already hold.
        let existing: HashSet<String> = read_jsonl_file_of::<A::Row>(&path)
            .unwrap_or_default()
            .iter()
            .map(|r| A::uuid(r).to_string())
            .collect();
        let missing: Vec<&A::Row> = day_rows
            .into_iter()
            .filter(|r| !existing.contains(A::uuid(*r)))
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

// Typed entry points (stable public API; each delegates to the generic core).

/// Append usage records to the per-day Artifact.
pub fn append_jsonl(paths: &Paths, device_id: &str, records: &[UsageRecord]) -> AppResult<()> {
    append_artifact_jsonl::<UsageGrain>(paths, device_id, records)
}

/// Append turn durations to the per-day Artifact.
pub fn append_turn_jsonl(paths: &Paths, device_id: &str, turns: &[TurnDuration]) -> AppResult<()> {
    append_artifact_jsonl::<TurnGrain>(paths, device_id, turns)
}

/// Detect rows that reached the SQLite store but never the JSONL Artifact
/// (residue from a pre-1.5.1 append failure) and, on a gap, clear the scan
/// cursors so the next collect re-parses every source line. The idempotent
/// append then backfills the gaps from the still-present AI CLI logs. Returns
/// whether a gap was found (and cursors cleared). A no-op once store and
/// Artifact agree, so it is cheap to run on every collect.
pub fn reconcile_artifact_gaps(store: &Store, paths: &Paths, device_id: &str) -> AppResult<bool> {
    let db_uuids = store.usage_uuids_for_device(device_id)?;
    if db_uuids.is_empty() {
        return Ok(false);
    }
    let artifact_uuids: std::collections::HashSet<String> =
        read_device_artifacts_of::<UsageGrain>(paths, device_id)?
            .into_iter()
            .map(|r| r.uuid)
            .collect();
    let has_gap = db_uuids.iter().any(|u| !artifact_uuids.contains(u));
    if has_gap {
        store.clear_scan_progress()?;
    }
    Ok(has_gap)
}

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

// ---------------- Sessions (session-meta grain + transcript) ----------------
//
// The session layer reuses the same JSONL-then-SQLite ordering and the same
// per-day file pattern as the usage/turn grains above, with two differences:
//   1. session-meta snapshots are written per day (derived from last_active_at)
//      but one session may appear in many day-files (cross-day sessions); a
//      merge-by-id pass on read collapses them (latest system data, COALESCE'd
//      user data);
//   2. transcripts (the heavy原文) are one file per SESSION (`sessions/<id>.jsonl`),
//      not per day — a conversation spans days, so per-day would shatter it.

/// Per-session transcript soft cap (5 MiB). Exceeded ⇒ log warning only; not
/// enforced (design §8: the main strategy is "favorites only", this is an
/// observability backstop).
const TRANSCRIPT_SOFT_CAP_BYTES: u64 = 5 * 1024 * 1024;

/// Build the grain row to write for a freshly-collected session, PRESERVING the
/// existing user-data columns. The invariant — "re-extract never overwrites
/// user data" (architecture.md) — lives here as a pure function: `fresh_system`
/// supplies every refreshable system column, while `existing`'s `custom_title`
/// / `favorited` / `synced_group_id` are carried through verbatim. The DB's
/// UPSERT mirrors this on the SQLite side (only system columns in the ON
/// CONFLICT update). Tested in isolation so the invariant cannot silently
/// regress.
pub fn merge_session_user_data(
    existing: Option<&SessionMetaSync>,
    fresh_system: &SessionSystemData,
) -> SessionMetaSync {
    SessionMetaSync {
        id: fresh_system.id.clone(),
        source: fresh_system.source.clone(),
        project_dir: fresh_system.project_dir.clone(),
        title_orig: fresh_system.title_orig.clone(),
        started_at: fresh_system.started_at.clone(),
        last_active_at: fresh_system.last_active_at.clone(),
        custom_title: existing.map(|e| e.custom_title.clone()).unwrap_or_default(),
        favorited: existing.map(|e| e.favorited).unwrap_or(false),
        synced_group_id: existing.map(|e| e.synced_group_id.clone()).unwrap_or_default(),
    }
}

/// Merge session-meta snapshots that span multiple day-files by `id`. System
/// data (source / project_dir / title_orig / started_at / last_active_at) is
/// taken from the snapshot with the latest `last_active_at`; the syncable user-
/// data strings (`custom_title` / `synced_group_id`) take the latest NON-EMPTY
/// value (COALESCE semantics — a field absent on the newest snapshot falls back
/// to the newest snapshot that carried it); `favorited` is a boolean, not a
/// string, so it takes the latest snapshot's value (latest-wins). Pure and
/// table-tested. This cannot reuse `read_all_artifacts_of` directly: that
/// returns a flat Vec, while sessions need the by-id merge to collapse cross-
/// day snapshots (the design's "session-meta-<day>.jsonl 按天 snapshot" rule).
pub fn merge_session_snapshots(rows: Vec<SessionMetaSync>) -> Vec<SessionMetaSync> {
    use std::collections::BTreeMap;
    let mut by_id: BTreeMap<String, Vec<SessionMetaSync>> = BTreeMap::new();
    for r in rows {
        by_id.entry(r.id.clone()).or_default().push(r);
    }
    by_id
        .into_values()
        .map(|group| merge_one_session_group(&group))
        .collect()
}

/// Merge one session's snapshots. See [`merge_session_snapshots`].
fn merge_one_session_group(group: &[SessionMetaSync]) -> SessionMetaSync {
    // Base = snapshot with the latest last_active_at (ties → the last in
    // iteration order; both carry the same id, so the system-data choice is
    // observably equivalent).
    let base = group
        .iter()
        .max_by(|a, b| a.last_active_at.cmp(&b.last_active_at))
        .expect("merge group is non-empty");
    // COALESCE: newest non-empty value for the user-data strings.
    let custom_title = group
        .iter()
        .filter(|r| !r.custom_title.is_empty())
        .max_by(|a, b| a.last_active_at.cmp(&b.last_active_at))
        .map(|r| r.custom_title.clone())
        .unwrap_or_default();
    let synced_group_id = group
        .iter()
        .filter(|r| !r.synced_group_id.is_empty())
        .max_by(|a, b| a.last_active_at.cmp(&b.last_active_at))
        .map(|r| r.synced_group_id.clone())
        .unwrap_or_default();
    SessionMetaSync {
        id: base.id.clone(),
        source: base.source.clone(),
        project_dir: base.project_dir.clone(),
        title_orig: base.title_orig.clone(),
        started_at: base.started_at.clone(),
        last_active_at: base.last_active_at.clone(),
        custom_title,
        favorited: base.favorited,
        synced_group_id,
    }
}

/// `<device_data_dir>/sessions/<session_id>.jsonl` — one file per session.
fn transcript_path(paths: &Paths, device_id: &str, session_id: &str) -> std::path::PathBuf {
    paths.device_data_dir(device_id).join("sessions").join(format!("{session_id}.jsonl"))
}

/// `<device_data_dir>/session-meta-<day>.jsonl`.
fn session_meta_day_path(paths: &Paths, device_id: &str, day: &str) -> std::path::PathBuf {
    paths
        .device_data_dir(device_id)
        .join(format!("session-meta-{day}.jsonl"))
}

/// Idempotent append of session-meta rows to per-day files. The day bucket is
/// derived from each row's `last_active_at`; rows already present (by `id`) are
/// skipped so a retried collect writes no duplicate. Mirrors
/// `append_artifact_jsonl`'s grouping + dedup, but computes the day from
/// `last_active_at` (the trait-based grain returns a borrowed `&str` and cannot
/// surface a computed value, and polluting the wire format with a redundant
/// `day` field would be worse than this localized helper).
fn append_session_meta_jsonl(
    paths: &Paths,
    device_id: &str,
    rows: &[SessionMetaSync],
) -> AppResult<()> {
    use std::collections::{BTreeMap, HashSet};
    let mut by_day: BTreeMap<String, Vec<&SessionMetaSync>> = BTreeMap::new();
    for r in rows {
        let day = UsageRecord::day_from_timestamp(&r.last_active_at);
        by_day.entry(day).or_default().push(r);
    }
    for (day, day_rows) in by_day {
        let path = session_meta_day_path(paths, device_id, &day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing: HashSet<String> = read_jsonl_file_of::<SessionMetaSync>(&path)
            .unwrap_or_default()
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let missing: Vec<&SessionMetaSync> = day_rows
            .into_iter()
            .filter(|r| !existing.contains(&r.id))
            .collect();
        if missing.is_empty() {
            continue;
        }
        write_jsonl_day(&path, &missing)?;
    }
    Ok(())
}

/// Read every device's merged session-meta snapshots (cross-day, by-id merge).
/// Reads each valid device dir's `session-meta-*.jsonl`, merges by id, then
/// concatenates. Used by tests / diagnostics; the pull path uses
/// [`import_peer_sessions`] so each peer's rows import under that peer's
/// device_id (SessionMetaSync itself carries no device_id field).
#[allow(dead_code)]
pub fn read_all_session_meta(paths: &Paths) -> AppResult<Vec<SessionMetaSync>> {
    let root = &paths.repo_data;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name_owned = entry.file_name();
        let Some(name) = name_owned.to_str() else {
            continue;
        };
        if !crate::config::is_valid_device_id(name) {
            continue;
        }
        out.extend(read_one_device_session_meta(paths, name)?);
    }
    Ok(out)
}

/// Read one device's session-meta grain files and merge by id (cross-day
/// snapshots). Shared between `read_all_session_meta` and
/// `import_peer_sessions`.
fn read_one_device_session_meta(paths: &Paths, device_id: &str) -> AppResult<Vec<SessionMetaSync>> {
    let dir = paths.device_data_dir(device_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut device_rows = Vec::new();
    for f in std::fs::read_dir(&dir)? {
        let f = f?;
        let p = f.path();
        if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let is_meta = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("session-meta-"))
            .unwrap_or(false);
        if !is_meta {
            continue;
        }
        device_rows.extend(read_jsonl_file_of::<SessionMetaSync>(&p)?);
    }
    Ok(merge_session_snapshots(device_rows))
}

/// Import PEER devices' session-meta grains into the Store. Each peer's merged
/// rows import under THAT peer's device_id (sessions are keyed by (id,
/// device_id)). This device's own id is skipped — its own user data is
/// authoritative locally, and re-importing a staler own-grain would revert a
/// just-applied edit. Returns the number of rows upserted. Used by
/// `pull_and_import`.
pub fn import_peer_sessions(
    store: &Store,
    paths: &Paths,
    own_device_id: &str,
) -> AppResult<u32> {
    let root = &paths.repo_data;
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0u32;
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name_owned = entry.file_name();
        let Some(name) = name_owned.to_str() else {
            continue;
        };
        if !crate::config::is_valid_device_id(name) || name == own_device_id {
            continue;
        }
        for m in read_one_device_session_meta(paths, name)? {
            store.import_session_grain(name, &m)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Append transcript messages to `sessions/<id>.jsonl`, deduping by message
/// `uuid` (idempotent re-collect writes no duplicate). The caller MUST ensure
/// the session is favorited — the invariant "原文仅 favorited 才采集" is
/// asserted at the ingest layer (`ingest_sessions` checks before calling).
/// Soft cap (5 MiB) warns but does not truncate (design §8).
pub fn append_session_transcript(
    paths: &Paths,
    device_id: &str,
    session_id: &str,
    messages: &[SessionMessage],
) -> AppResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let path = transcript_path(paths, device_id, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing: std::collections::HashSet<String> = read_jsonl_file_of::<SessionMessage>(&path)
        .unwrap_or_default()
        .iter()
        .map(|m| m.uuid.clone())
        .collect();
    let missing: Vec<&SessionMessage> = messages
        .iter()
        .filter(|m| !existing.contains(&m.uuid))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    write_jsonl_day(&path, &missing)?;
    // Soft-cap observability: warn (don't truncate) when a transcript file
    // crosses 5 MiB — the favorites-only policy is the real cap.
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > TRANSCRIPT_SOFT_CAP_BYTES {
            eprintln!(
                "[vaultone] session {session_id} transcript exceeds 5 MiB soft cap ({} bytes)",
                meta.len()
            );
        }
    }
    Ok(())
}

/// Read one device's transcript for a session. A missing file (a non-favorited
/// session, or one never synced here) returns an empty Vec — the transcript's
/// absence is a normal state, not an error.
pub fn read_session_transcript(
    paths: &Paths,
    device_id: &str,
    session_id: &str,
) -> AppResult<Vec<SessionMessage>> {
    let path = transcript_path(paths, device_id, session_id);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut out = Vec::new();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(m) = serde_json::from_str::<SessionMessage>(line) {
                    out.push(m);
                }
            }
            Ok(out)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Read every device's transcript for a session and merge by message uuid (the
/// `get_session_transcript` command's read path: own device first, then peers'
/// pulled-in files). Dedup keeps the first occurrence per uuid (own device
/// wins on conflict — it is the source of truth for a session it collected).
pub fn read_all_transcripts(paths: &Paths, session_id: &str) -> Vec<SessionMessage> {
    let mut out: Vec<SessionMessage> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let root = &paths.repo_data;
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    // Own device first so its messages win dedup; then peers in stable order.
    let mut device_ids: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            if crate::config::is_valid_device_id(name) {
                device_ids.push(name.to_string());
            }
        }
    }
    device_ids.sort();
    for did in device_ids {
        if let Ok(msgs) = read_session_transcript(paths, &did, session_id) {
            for m in msgs {
                if seen.insert(m.uuid.clone()) {
                    out.push(m);
                }
            }
        }
    }
    out
}

/// Ingest a provider's session output: refresh system data in the SQLite table
/// (UPSERT preserves user data), write the per-day session-meta grain with the
/// preserved user data, and append transcripts for favorited sessions only.
///
/// JSONL-first then SQLite: the grain is the sync medium peers pull, so a
/// failed append aborts before the DB commit — matching the usage/turn path.
pub fn ingest_sessions(
    store: &Store,
    paths: &Paths,
    device_id: &str,
    sessions: &[RawSession],
    messages: &[SessionMessage],
) -> AppResult<()> {
    if sessions.is_empty() && messages.is_empty() {
        return Ok(());
    }

    // Build the grain rows: merge each fresh system-data session with its
    // existing user data (read from the DB) so the grain snapshot carries the
    // current custom_title/favorited/synced_group_id. `existing_for` reads the
    // stored user-data fields; `merge_session_user_data` is the pure invariant
    // that preserves them. Writes the grain FIRST (the sync medium).
    let mut grain_rows: Vec<SessionMetaSync> = Vec::with_capacity(sessions.len());
    let mut existing_map: std::collections::HashMap<String, SessionMetaSync> =
        std::collections::HashMap::new();
    for s in sessions {
        let existing = store
            .get_session_meta_sync(device_id, &s.id)?
            .or_else(|| existing_map.get(&s.id).cloned());
        let merged = merge_session_user_data(existing.as_ref(), s);
        existing_map.insert(s.id.clone(), merged.clone());
        grain_rows.push(merged);
    }
    append_session_meta_jsonl(paths, device_id, &grain_rows)?;

    // SQLite: refresh system data only (UPSERT preserves user data).
    for s in sessions {
        store.upsert_session(device_id, s)?;
    }

    // Transcripts: group messages by session, append ONLY for favorited ones.
    // The invariant "原文仅 favorited 才采集" is enforced here — a session must
    // be favorited in the DB (just refreshed above with preserved user data)
    // before its messages land in `sessions/<id>.jsonl`.
    if !messages.is_empty() {
        let mut by_session: std::collections::HashMap<String, Vec<SessionMessage>> =
            std::collections::HashMap::new();
        for m in messages {
            by_session
                .entry(m.session_id.clone())
                .or_default()
                .push(m.clone());
        }
        for (sid, msgs) in by_session {
            let favorited = store
                .get_session_meta_sync(device_id, &sid)?
                .map(|m| m.favorited)
                .unwrap_or(false);
            if favorited {
                append_session_transcript(paths, device_id, &sid, &msgs)?;
            }
        }
    }
    Ok(())
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
            session_id: String::new(),
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
    fn append_jsonl_is_idempotent_so_a_retried_collect_writes_no_duplicate() {
        // A retried collect (scan cursor unchanged after a SQLite failure, say)
        // re-parses the same source lines and calls append_jsonl again. The
        // append must be idempotent — the day's file gains no duplicate row.
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let book = seed_book();
        let r1 = recordify(&raw("a", "glm-5.2"), "0123456789ab", &book);
        let r2 = recordify(&raw("b", "glm-5.2"), "0123456789ab", &book);

        append_jsonl(&paths, "0123456789ab", &[r1.clone(), r2.clone()]).unwrap();
        // Re-append the same batch (simulating the retried collect).
        append_jsonl(&paths, "0123456789ab", &[r1, r2]).unwrap();

        let read = read_device_artifacts_of::<UsageGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(
            read.len(),
            2,
            "no duplicate rows after a re-append: {read:?}"
        );
    }

    #[test]
    fn ingest_collected_backfills_artifact_gaps_on_a_source_rescan() {
        // Regression: a row that landed in SQLite but not the JSONL Artifact
        // (an append failure under the old db-first order) used to be locked out
        // forever — the ledger dedup silenced every later collect. With JSONL
        // written first and idempotently, a rescan of the same source lines
        // backfills the missing row into the Artifact.
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("gap1", "glm-5.2"), raw("gap2", "glm-5.2")],
            turn_durations: vec![],
            files_scanned: 1,
            lines_skipped: 0,
            sessions: vec![],
            messages: vec![],
        };
        // First ingest: both rows in SQLite AND the Artifact.
        ingest_collected(&store, &paths, "0123456789ab", &book, result.clone()).unwrap();
        // Simulate the old failure mode: drop `gap1` from the Artifact only.
        let day_file = paths
            .device_data_dir("0123456789ab")
            .join("usage-2026-07-13.jsonl");
        let pruned: String = std::fs::read_to_string(&day_file)
            .unwrap()
            .lines()
            .filter(|l| !l.contains("\"gap1\""))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&day_file, pruned).unwrap();
        let after_drop = read_device_artifacts_of::<UsageGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(after_drop.len(), 1, "gap1 removed from Artifact");

        // Rescan the same source: the idempotent append backfills gap1.
        ingest_collected(&store, &paths, "0123456789ab", &book, result).unwrap();
        let read = read_device_artifacts_of::<UsageGrain>(&paths, "0123456789ab").unwrap();
        assert_eq!(
            read.len(),
            2,
            "rescan backfilled the Artifact gap: {read:?}"
        );
    }

    #[test]
    fn reconcile_artifact_gaps_flags_missing_rows_and_quiets_once_backfilled() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = seed_book();
        let dev = "0123456789ab";
        let result = CollectResult {
            source: "claude_code".into(),
            events: vec![raw("x", "glm-5.2")],
            turn_durations: vec![],
            files_scanned: 1,
            lines_skipped: 0,
            sessions: vec![],
            messages: vec![],
        };
        // Row lands in store + Artifact.
        ingest_collected(&store, &paths, dev, &book, result.clone()).unwrap();
        // Wipe the Artifact to mimic a pre-1.5.1 append failure.
        let day_file = paths.device_data_dir(dev).join("usage-2026-07-13.jsonl");
        std::fs::write(&day_file, "").unwrap();
        let gapped = reconcile_artifact_gaps(&store, &paths, dev).unwrap();
        assert!(gapped, "gap detected (row in store, not Artifact)");

        // A rescan backfills the Artifact; reconcile then reports no gap.
        ingest_collected(&store, &paths, dev, &book, result).unwrap();
        let clean = reconcile_artifact_gaps(&store, &paths, dev).unwrap();
        assert!(!clean, "no gap once the Artifact is backfilled");
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
            sessions: vec![],
            messages: vec![],
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
            sessions: vec![],
            messages: vec![],
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
            sessions: vec![],
            messages: vec![],
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

    // ---- session invariants (architecture.md: invariants in code + tests) ----

    fn sys_session(id: &str, last_active_at: &str) -> RawSession {
        RawSession {
            id: id.into(),
            source: "claude_code".into(),
            project_dir: "/proj".into(),
            title_orig: "orig-title".into(),
            started_at: "2026-08-01T00:00:00.000Z".into(),
            last_active_at: last_active_at.into(),
        }
    }

    fn msg(uuid: &str, session_id: &str, content: &str) -> SessionMessage {
        SessionMessage {
            uuid: uuid.into(),
            session_id: session_id.into(),
            role: crate::model::SessionMessageRole::User,
            ts: "2026-08-01T00:00:00.000Z".into(),
            model: None,
            name: None,
            content: content.into(),
        }
    }

    /// Invariant: re-extract refreshes system data but NEVER overwrites user
    /// data. `merge_session_user_data` (the grain-layer pure function) keeps
    /// the existing custom_title / favorited / synced_group_id.
    #[test]
    fn merge_session_user_data_preserves_existing_user_data() {
        let existing = SessionMetaSync {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/old".into(),
            title_orig: "old-title".into(),
            started_at: "2026-08-01T00:00:00.000Z".into(),
            last_active_at: "2026-08-01T01:00:00.000Z".into(),
            custom_title: "My Rename".into(),
            favorited: true,
            synced_group_id: "aabbccddeeff-11111111".into(),
        };
        let fresh = sys_session("s1", "2026-08-02T09:00:00.000Z");
        let merged = merge_session_user_data(Some(&existing), &fresh);
        // System data refreshed from fresh.
        assert_eq!(merged.last_active_at, "2026-08-02T09:00:00.000Z");
        assert_eq!(merged.project_dir, "/proj");
        assert_eq!(merged.title_orig, "orig-title");
        // User data preserved from existing.
        assert_eq!(merged.custom_title, "My Rename");
        assert!(merged.favorited);
        assert_eq!(merged.synced_group_id, "aabbccddeeff-11111111");
    }

    /// Invariant (SQLite side): `upsert_session` refreshes only the system-data
    /// columns on conflict; user-data columns set by the user must survive a
    /// re-extract. Regression test for the ON CONFLICT clause.
    #[test]
    fn upsert_session_does_not_overwrite_user_data_on_reextract() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let dev = "0123456789ab";
        // First collect: creates the row with default user data.
        store
            .upsert_session(dev, &sys_session("s1", "2026-08-01T01:00:00.000Z"))
            .unwrap();
        // User edits: custom_title, favorited, local_group_id, synced_group_id.
        store.set_session_custom_title(dev, "s1", Some("Renamed")).unwrap();
        store.set_session_favorited(dev, "s1", true).unwrap();
        store.set_session_local_group(dev, "s1", Some("lg1")).unwrap();
        store
            .set_session_synced_group(dev, "s1", Some("sg1"))
            .unwrap();
        // Re-extract (next collect): system data refresh, must NOT clobber edits.
        store
            .upsert_session(dev, &sys_session("s1", "2026-08-02T09:00:00.000Z"))
            .unwrap();
        let m = store.get_session_meta_sync(dev, "s1").unwrap().unwrap();
        assert_eq!(m.last_active_at, "2026-08-02T09:00:00.000Z", "system refreshed");
        assert_eq!(m.title_orig, "orig-title");
        assert_eq!(m.custom_title, "Renamed", "custom_title preserved");
        assert!(m.favorited, "favorited preserved");
        assert_eq!(m.synced_group_id, "sg1", "synced_group_id preserved");
        // local_group_id is not on SessionMetaSync; query_sessions surfaces it.
        let rows = store.query_sessions(None).unwrap();
        let row = rows.iter().find(|r| r.id == "s1").unwrap();
        assert_eq!(row.local_group_id, "lg1", "local_group_id preserved");
    }

    /// Invariant: transcripts are written ONLY for favorited sessions.
    /// `ingest_sessions` checks favorited in the DB before appending.
    #[test]
    fn ingest_sessions_writes_transcript_only_when_favorited() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let dev = "0123456789ab";

        // Two sessions, both collected; messages for both.
        let fav = sys_session("fav", "2026-08-01T01:00:00.000Z");
        let plain = sys_session("plain", "2026-08-01T01:00:00.000Z");
        ingest_sessions(
            &store,
            &paths,
            dev,
            &[fav.clone(), plain.clone()],
            &[],
        )
        .unwrap();
        // Favorite only `fav`.
        store.set_session_favorited(dev, "fav", true).unwrap();
        // Next collect: messages for both arrive.
        ingest_sessions(
            &store,
            &paths,
            dev,
            &[fav, plain],
            &[
                msg("m1", "fav", "hello"),
                msg("m2", "plain", "world"),
            ],
        )
        .unwrap();
        // `fav` transcript exists; `plain` does not.
        assert!(
            read_session_transcript(&paths, dev, "fav").unwrap().len() == 1,
            "favorited session's transcript was written"
        );
        assert!(
            read_session_transcript(&paths, dev, "plain")
                .unwrap()
                .is_empty(),
            "non-favorited session collected NO transcript"
        );
    }

    /// Invariant: cross-day session snapshots merge by id (latest system data,
    /// COALESCE'd user data). Same session in two day-files collapses to one.
    #[test]
    fn merge_session_snapshots_collapses_cross_day_snapshots() {
        let day1 = SessionMetaSync {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/p".into(),
            title_orig: "orig".into(),
            started_at: "2026-08-01T00:00:00.000Z".into(),
            last_active_at: "2026-08-01T09:00:00.000Z".into(),
            custom_title: "Day1 Rename".into(),
            favorited: false,
            synced_group_id: String::new(),
        };
        let day2 = SessionMetaSync {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/p".into(),
            title_orig: "orig".into(),
            started_at: "2026-08-01T00:00:00.000Z".into(),
            last_active_at: "2026-08-02T10:00:00.000Z".into(),
            custom_title: String::new(), // day2 snapshot dropped it
            favorited: true,
            synced_group_id: "aabb-11111111".into(),
        };
        let merged = merge_session_snapshots(vec![day1, day2]);
        assert_eq!(merged.len(), 1, "two day-snapshots collapse to one");
        let m = &merged[0];
        // System data from the latest (day2).
        assert_eq!(m.last_active_at, "2026-08-02T10:00:00.000Z");
        // custom_title COALESCE'd: day2 empty ⇒ fall back to day1's non-empty.
        assert_eq!(m.custom_title, "Day1 Rename");
        // favorited latest-wins (day2 = true).
        assert!(m.favorited);
        assert_eq!(m.synced_group_id, "aabb-11111111");
    }

    /// `read_all_session_meta` reads + merges per device; a session present in
    /// two day-files for the same device comes back once.
    #[test]
    fn read_all_session_meta_merges_one_device_cross_day() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let dev = "0123456789ab";
        let dir = paths.device_data_dir(dev);
        std::fs::create_dir_all(&dir).unwrap();
        // Same session, two day-files.
        let d1 = SessionMetaSync {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/p".into(),
            title_orig: "o".into(),
            started_at: "2026-08-01T00:00:00.000Z".into(),
            last_active_at: "2026-08-01T09:00:00.000Z".into(),
            custom_title: String::new(),
            favorited: false,
            synced_group_id: String::new(),
        };
        let d2 = SessionMetaSync {
            last_active_at: "2026-08-02T09:00:00.000Z".into(),
            favorited: true,
            ..d1.clone()
        };
        std::fs::write(
            dir.join("session-meta-2026-08-01.jsonl"),
            format!("{}\n", serde_json::to_string(&d1).unwrap()),
        )
        .unwrap();
        std::fs::write(
            dir.join("session-meta-2026-08-02.jsonl"),
            format!("{}\n", serde_json::to_string(&d2).unwrap()),
        )
        .unwrap();
        let all = read_all_session_meta(&paths).unwrap();
        assert_eq!(all.len(), 1, "cross-day snapshots merged into one");
        assert_eq!(all[0].last_active_at, "2026-08-02T09:00:00.000Z");
        assert!(all[0].favorited);
    }
}
