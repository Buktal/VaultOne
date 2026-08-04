//! OpenCode (`~/.local/share/opencode/opencode.db`) session-log provider.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

use super::{
    metadata_modified_nanos, CollectResult, FileCursor, Provider, RawUsage, ScanProgress,
    ScanProgressDelta,
};

/// OpenCode (`~/.local/share/opencode/opencode.db`) session-log provider.
///
/// OpenCode stores sessions in a SQLite db (WAL mode). `message.data` is a JSON
/// string with Anthropic-style tokens: `input` is fresh, `cache.{read,write}`
/// are separate, `reasoning` folds into `output`. The provider opens the db
/// read-only and queries per session. The main db file only updates on
/// checkpoint, so fresh commits in `-wal` are merged into the mtime gate; a
/// two-level watermark (file + per-session `time_updated`) skips unchanged work.
pub struct OpenCodeProvider {
    db_path: Option<PathBuf>,
}

impl OpenCodeProvider {
    /// Default provider rooted at the resolved opencode db path (absent ⇒ the
    /// provider discovers nothing).
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            db_path: opencode_db_path(),
        })
    }

    /// Test/override constructor with an explicit db path.
    #[cfg(test)]
    pub(crate) fn with_db(path: PathBuf) -> Self {
        Self {
            db_path: Some(path),
        }
    }
}

impl Provider for OpenCodeProvider {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn discover(&self) -> AppResult<Vec<PathBuf>> {
        match &self.db_path {
            Some(p) if p.exists() => Ok(vec![p.clone()]),
            _ => Ok(Vec::new()),
        }
    }

    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult> {
        let mut events = Vec::new();
        let mut skipped = 0u32;
        let mut files_scanned = 0u32;
        for db_path in files {
            files_scanned += 1;
            let conn = match open_opencode_readonly(db_path) {
                Ok(c) => c,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            match collect_all_messages(&conn) {
                Ok(ev) => events.extend(ev),
                Err(_) => skipped += 1,
            }
        }
        events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations: Vec::new(),
            files_scanned,
            lines_skipped: skipped,
        })
    }

    /// Two-level watermark incremental: file-level mtime gate (db + `-wal`
    /// merged) skips an unchanged db; per-session `time_updated` skips sessions
    /// already synced. A session with an in-progress message (no `time.completed`)
    /// does not advance its cursor, so it retries next collect.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        let mut result = CollectResult {
            source: self.name().to_string(),
            ..CollectResult::default()
        };
        let mut delta = ScanProgressDelta::new();
        let Some(db_path) = &self.db_path else {
            return Ok((result, delta));
        };
        let db_path_str = db_path.to_string_lossy().into_owned();

        let Some(merged_mtime) = merged_db_mtime(db_path) else {
            return Ok((result, delta));
        };
        result.files_scanned = 1;
        let prev_file = progress.get(&db_path_str).copied().unwrap_or_default();
        if prev_file.last_modified != 0 && merged_mtime <= prev_file.last_modified {
            return Ok((result, delta));
        }

        let conn = match open_opencode_readonly(db_path) {
            Ok(c) => c,
            Err(_) => {
                result.lines_skipped = 1;
                return Ok((result, delta));
            }
        };
        let sessions = match query_sessions(&conn) {
            Ok(s) => s,
            Err(_) => {
                result.lines_skipped = 1;
                return Ok((result, delta));
            }
        };
        for (session_id, watermark) in &sessions {
            let sync_key = format!("{db_path_str}:{session_id}");
            let prev_sess = progress.get(&sync_key).copied().unwrap_or_default();
            if *watermark <= prev_sess.last_modified {
                continue;
            }
            match query_assistant_messages(&conn, session_id) {
                Ok(qr) => {
                    for (message_id, msg) in &qr.messages {
                        result
                            .events
                            .push(opencode_raw_usage(session_id, message_id, msg));
                    }
                    if !qr.has_incomplete_usage {
                        delta.insert(
                            sync_key.clone(),
                            FileCursor {
                                last_modified: *watermark,
                                last_line_offset: 0,
                            },
                        );
                    }
                }
                Err(_) => result.lines_skipped += 1,
            }
        }
        result
            .events
            .sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        delta.insert(
            db_path_str,
            FileCursor {
                last_modified: merged_mtime,
                last_line_offset: 0,
            },
        );
        Ok((result, delta))
    }
}

/// Resolve the opencode db path: `OPENCODE_DB` (absolute) > `XDG_DATA_HOME` >
/// `~/.local/share/opencode/opencode.db`. OpenCode uses xdg-basedir uniformly
/// across platforms, so this is the same path on Windows as on Linux.
fn opencode_db_path() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("OPENCODE_DB") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return Some(p);
        }
    }
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return Some(p.join("opencode").join("opencode.db"));
        }
    }
    let home = dirs::home_dir()?;
    Some(
        home.join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db"),
    )
}

/// max(db mtime, db-wal mtime) — the main db only updates on checkpoint, so
/// fresh commits in the `-wal` side file must be considered or they're missed.
fn merged_db_mtime(db_path: &Path) -> Option<i64> {
    let db_meta = std::fs::metadata(db_path).ok()?;
    let mut m = metadata_modified_nanos(&db_meta);
    let wal = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = std::fs::metadata(&wal) {
        m = m.max(metadata_modified_nanos(&wal_meta));
    }
    Some(m)
}

fn open_opencode_readonly(db_path: &Path) -> AppResult<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Db(format!("cannot open opencode.db read-only: {e}")))
}

/// All sessions' completed assistant messages (full scan, no watermark gate).
/// Reached only from `Provider::parse` — the test/diagnostic full-scan path.
#[allow(dead_code)] // production runs collect_incremental (per-session, watermarked); this is parse-only
fn collect_all_messages(conn: &rusqlite::Connection) -> AppResult<Vec<RawUsage>> {
    let sessions = query_sessions(conn)?;
    let mut events = Vec::new();
    for (session_id, _) in &sessions {
        if let Ok(qr) = query_assistant_messages(conn, session_id) {
            for (message_id, msg) in &qr.messages {
                events.push(opencode_raw_usage(session_id, message_id, msg));
            }
        }
    }
    Ok(events)
}

/// Per-session (id, sync watermark) — the max of the session's own
/// `time_updated` and all its messages' `time_updated`.
fn query_sessions(conn: &rusqlite::Connection) -> AppResult<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id,
                    MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
             FROM session s
             LEFT JOIN message m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY sync_watermark",
        )
        .map_err(|e| AppError::Db(format!("opencode session query prepare: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| AppError::Db(format!("opencode session query: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| AppError::Db(format!("opencode session row: {e}")))?);
    }
    Ok(out)
}

/// A session's completed assistant messages, plus whether an in-progress
/// message (no `time.completed`) was seen — the caller retries that session.
struct OpenCodeMessageQuery {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

/// Parsed `message.data` token fields (Anthropic-style: fresh input, cache split).
struct OpenCodeMessageData {
    input_tokens: u32,
    output_tokens: u32,
    reasoning_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    model_id: String,
    timestamp_ms: i64,
}

fn query_assistant_messages(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> AppResult<OpenCodeMessageQuery> {
    let mut stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created")
        .map_err(|e| AppError::Db(format!("opencode message query prepare: {e}")))?;
    let rows = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| AppError::Db(format!("opencode message query: {e}")))?;
    let mut messages = Vec::new();
    let mut has_incomplete_usage = false;
    for row in rows {
        let (message_id, data_json) =
            row.map_err(|e| AppError::Db(format!("opencode message row: {e}")))?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&data_json) else {
            continue;
        };
        if value.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        if value.get("tokens").is_none() {
            continue;
        }
        // In-progress messages carry half-formed tokens and no `time.completed`;
        // skip them and signal the caller to retry the session.
        if value.get("time").and_then(|t| t.get("completed")).is_none() {
            has_incomplete_usage = true;
            continue;
        }
        if let Some(msg) = parse_opencode_message_data(&value) {
            messages.push((message_id, msg));
        }
    }
    Ok(OpenCodeMessageQuery {
        messages,
        has_incomplete_usage,
    })
}

/// Parse a `message.data` JSON value into token fields. Returns `None` for an
/// all-zero message. OpenCode's self-reported `cost` is deliberately ignored —
/// VaultOne recomputes cost from its own pricing so the four-bucket split stays
/// consistent across providers.
fn parse_opencode_message_data(value: &serde_json::Value) -> Option<OpenCodeMessageData> {
    let tokens = value.get("tokens")?;
    let n = |k: &str| tokens.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let input_tokens = n("input");
    let output_tokens = n("output");
    let reasoning_tokens = n("reasoning");
    let cache_obj = tokens.get("cache");
    let cache_read_tokens = cache_obj
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_write_tokens = cache_obj
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if input_tokens == 0
        && output_tokens == 0
        && reasoning_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
    {
        return None;
    }
    let model_id = value
        .get("modelID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let timestamp_ms = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        model_id,
        timestamp_ms,
    })
}

fn opencode_raw_usage(session_id: &str, message_id: &str, msg: &OpenCodeMessageData) -> RawUsage {
    let timestamp = if msg.timestamp_ms > 0 {
        chrono::DateTime::from_timestamp_millis(msg.timestamp_ms)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(crate::time::now_iso)
    } else {
        crate::time::now_iso()
    };
    RawUsage {
        uuid: format!("opencode:{session_id}:{message_id}"),
        timestamp,
        model: msg.model_id.clone(),
        source: "opencode".to_string(),
        tokens: TokenCounts {
            input: msg.input_tokens,
            output: msg.output_tokens + msg.reasoning_tokens,
            cache_creation: msg.cache_write_tokens,
            cache_read: msg.cache_read_tokens,
        },
        server_tool_use: ServerToolUse::default(),
        stop_reason: String::new(),
        service_tier: String::new(),
        iterations: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opencode_data_json(
        input: u32,
        output: u32,
        reasoning: u32,
        cache_read: u32,
        cache_write: u32,
        model: &str,
        completed: bool,
    ) -> String {
        let time = if completed {
            r#""time":{"created":1779755333700,"completed":1779755350639}"#.to_string()
        } else {
            r#""time":{"created":1779755333700}"#.to_string()
        };
        format!(
            r#"{{"role":"assistant","tokens":{{"input":{input},"output":{output},"reasoning":{reasoning},"cache":{{"read":{cache_read},"write":{cache_write}}}}},"modelID":"{model}",{time}}}"#
        )
    }

    #[test]
    fn opencode_parse_message_data_variants() {
        let full: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "total": 56554, "input": 3272, "output": 383, "reasoning": 419,
                        "cache": { "write": 0, "read": 52480 } },
            "modelID": "deepseek-v4-pro",
            "providerID": "deepseek",
            "time": { "created": 1779755333700i64, "completed": 1779755350639i64 }
        });
        let d = parse_opencode_message_data(&full).unwrap();
        assert_eq!(d.input_tokens, 3272);
        assert_eq!(d.output_tokens, 383);
        assert_eq!(d.reasoning_tokens, 419);
        assert_eq!(d.cache_read_tokens, 52480);
        assert_eq!(d.cache_write_tokens, 0);
        assert_eq!(d.model_id, "deepseek-v4-pro");
        assert_eq!(d.timestamp_ms, 1779755333700);
        // missing cache ⇒ zeros.
        let no_cache: serde_json::Value = serde_json::json!({
            "role": "assistant", "tokens": { "input": 1000, "output": 200 },
            "modelID": "m", "time": { "created": 1, "completed": 2 }
        });
        let d = parse_opencode_message_data(&no_cache).unwrap();
        assert_eq!(d.cache_read_tokens, 0);
        assert_eq!(d.cache_write_tokens, 0);
        // all-zero ⇒ None.
        let zero: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 0, "output": 0, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "modelID": "t", "time": { "created": 1, "completed": 2 }
        });
        assert!(parse_opencode_message_data(&zero).is_none());
    }

    #[test]
    fn opencode_query_skips_incomplete_messages() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        let done = opencode_data_json(1000, 200, 0, 0, 0, "m", true);
        let wip = opencode_data_json(500, 0, 0, 0, 0, "m", false);
        conn.execute(
            "INSERT INTO message VALUES ('done','s1',1,?1),('wip','s1',2,?2)",
            rusqlite::params![done, wip],
        )
        .unwrap();
        let qr = query_assistant_messages(&conn, "s1").unwrap();
        assert_eq!(qr.messages.len(), 1);
        assert_eq!(qr.messages[0].0, "done");
        assert!(qr.has_incomplete_usage);
    }

    #[test]
    fn opencode_query_sessions_uses_message_watermark() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT, time_updated INTEGER);
             CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
             INSERT INTO session VALUES ('s1', 100);
             INSERT INTO message VALUES ('m1', 's1', 90, 200, '{}');",
        )
        .unwrap();
        let sessions = query_sessions(&conn).unwrap();
        assert_eq!(sessions, vec![("s1".to_string(), 200)]);
    }

    #[test]
    fn opencode_parses_db_into_four_buckets() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("opencode.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT, time_updated INTEGER);
                 CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
                 INSERT INTO session VALUES ('s1', 100);
                 INSERT INTO message (id, session_id, time_created, time_updated) VALUES ('m1','s1',90,200);
                 INSERT INTO message (id, session_id, time_created, time_updated) VALUES ('m2','s1',91,201);",
            )
            .unwrap();
            conn.execute(
                "UPDATE message SET data = ?1 WHERE id = 'm1'",
                rusqlite::params![opencode_data_json(
                    3272,
                    383,
                    419,
                    52480,
                    0,
                    "deepseek-v4-pro",
                    true
                )],
            )
            .unwrap();
            conn.execute(
                "UPDATE message SET data = ?1 WHERE id = 'm2'",
                rusqlite::params![opencode_data_json(
                    10,
                    5,
                    0,
                    0,
                    100,
                    "anthropic/claude-opus-4-6",
                    true
                )],
            )
            .unwrap();
        }
        let p = OpenCodeProvider::with_db(db.clone());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.source, "opencode");
        assert_eq!(result.events.len(), 2);
        let by_id: std::collections::HashMap<&str, &RawUsage> =
            result.events.iter().map(|e| (e.uuid.as_str(), e)).collect();
        let m1 = by_id["opencode:s1:m1"];
        assert_eq!(m1.tokens.input, 3272);
        assert_eq!(
            m1.tokens.output, 802,
            "output folds in reasoning (383 + 419)"
        );
        assert_eq!(m1.tokens.cache_read, 52480);
        assert_eq!(m1.tokens.cache_creation, 0);
        assert_eq!(m1.model, "deepseek-v4-pro");
        let m2 = by_id["opencode:s1:m2"];
        assert_eq!(
            m2.tokens.cache_creation, 100,
            "cache.write maps to cache_creation"
        );
    }

    #[test]
    fn opencode_incremental_skips_already_synced_session() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("opencode.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT, time_updated INTEGER);
                 CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
                 INSERT INTO session VALUES ('s1', 100);
                 INSERT INTO message (id, session_id, time_created, time_updated) VALUES ('m1','s1',90,200);",
            )
            .unwrap();
            let data = opencode_data_json(3272, 383, 419, 52480, 0, "deepseek-v4-pro", true);
            conn.execute(
                "UPDATE message SET data = ?1 WHERE id = 'm1'",
                rusqlite::params![data],
            )
            .unwrap();
        }
        let p = OpenCodeProvider::with_db(db);
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        // Same db, no changes ⇒ file mtime gate skips it entirely.
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 0);
    }
}
