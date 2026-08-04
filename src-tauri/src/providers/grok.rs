//! Grok CLI ("Grok Build") session-log provider.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

use super::{
    collect_jsonl_incremental, normalize_cache_inclusive, CollectResult, FileParseOutcome,
    Provider, RawUsage, ScanProgress, ScanProgressDelta,
};

/// Grok CLI ("Grok Build") session-log provider.
///
/// Reads `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl`.
/// Each line is a JSON-RPC notification; only `_x.ai/session/update` whose
/// `params.update.sessionUpdate` is `turn_completed` (absent ⇒ backward-
/// compatible passthrough) carries usage. A turn's usage is an **independent
/// per-turn total** — not a cumulative snapshot like Codex — so every event is
/// recorded at face value and never diffed against its neighbor.
///
/// `inputTokens` is cache-inclusive (it contains `cachedReadTokens`), so it is
/// normalized to fresh at parse; `outputTokens` already includes reasoning (do
/// not add `reasoningTokens`); `cache_creation` is always 0 (Grok exposes no
/// write bucket). One turn may span multiple models (`usage.modelUsage`), each
/// emitted as its own record. The CLI's `costUsdTicks` / `apiDurationMs` are
/// ignored — cost is recomputed from local pricing at ingest.
pub struct GrokProvider {
    grok_dir: PathBuf,
}

impl GrokProvider {
    /// Default provider rooted at `~/.grok`.
    pub fn new() -> AppResult<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| AppError::Provider("cannot resolve home dir".into()))?;
        Ok(Self {
            grok_dir: home.join(".grok"),
        })
    }

    /// Test/override constructor with an explicit Grok dir.
    #[cfg(test)]
    pub(crate) fn with_dir(dir: PathBuf) -> Self {
        Self { grok_dir: dir }
    }

    /// Recursively collect every `updates.jsonl` under `sessions/` and
    /// `archived_sessions/`. Layout depth varies (`<enc-cwd>/<session-id>/…`),
    /// so discovery is by filename, mirroring Grok's session browser.
    fn discover_in(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for sub in ["sessions", "archived_sessions"] {
            let root = self.grok_dir.join(sub);
            if root.is_dir() {
                collect_grok_updates(&root, &mut files, 0);
            }
        }
        files
    }
}

impl Provider for GrokProvider {
    fn name(&self) -> &'static str {
        "grok_cli"
    }

    fn discover(&self) -> AppResult<Vec<PathBuf>> {
        if !self.grok_dir.exists() {
            return Ok(Vec::new());
        }
        Ok(self.discover_in())
    }

    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult> {
        // Full-parse path. Each turn is independent — no cross-line state to
        // rebuild, unlike Codex's cumulative delta.
        let mut events = Vec::new();
        let mut skipped = 0u32;
        for file in files {
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let outcome = parse_grok_file(file, &text, 0);
            events.extend(outcome.events);
            skipped += outcome.skipped;
        }
        events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations: Vec::new(),
            files_scanned: files.len() as u32,
            lines_skipped: skipped,
        })
    }

    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        collect_jsonl_incremental(self, progress, parse_grok_file)
    }
}

/// Recursively collect every `updates.jsonl` under `root`. Symlinked dirs are
/// not followed (file_type is non-following) and a depth cap guards against
/// pathological nesting.
fn collect_grok_updates(root: &Path, files: &mut Vec<PathBuf>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            collect_grok_updates(&path, files, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("updates.jsonl") {
            files.push(path);
        }
    }
}

/// Parse one Grok `updates.jsonl` file into per-call events, skipping lines at
/// or before `start_line` (the incremental cursor). `session_id` is the session
/// directory name — the stable scoping dimension for the per-turn dedup key.
fn parse_grok_file(file: &Path, text: &str, start_line: i64) -> FileParseOutcome {
    let session_id = file
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let mut events = Vec::new();
    let mut skipped = 0u32;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx as i64 + 1; // 1-based
        if line_no <= start_line {
            continue;
        }
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            skipped += 1; // malformed JSON — a genuine parse failure
            continue;
        };
        // Non-qualifying lines (other notifications / mid-turn snapshots) are
        // normal noise, silently filtered — not counted as skipped.
        if let Some(ev) = parse_grok_notification(&record, session_id, line_no) {
            events.extend(ev);
        }
    }
    FileParseOutcome {
        events,
        turn_durations: Vec::new(),
        skipped,
    }
}

/// Parse one JSON-RPC notification into per-model raw usages, or `None` if the
/// line is not a `turn_completed` usage notification (filtered as noise, not an
/// error). `inputTokens` is cache-inclusive and normalized to fresh here.
fn parse_grok_notification(
    record: &serde_json::Value,
    session_id: &str,
    line_no: i64,
) -> Option<Vec<RawUsage>> {
    if record.get("method").and_then(|v| v.as_str()) != Some("_x.ai/session/update") {
        return None;
    }
    let update = record.get("params").and_then(|p| p.get("update"))?;
    // Only turn_completed carries billable usage; mid-turn snapshots
    // (usage_snapshot) are dropped to avoid double-counting a partial turn.
    // Absent sessionUpdate is passed through for backward compatibility.
    let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
    if kind.is_some() && kind != Some("turn_completed") {
        return None;
    }
    let usage = update.get("usage").filter(|u| u.is_object())?;
    let timestamp = parse_grok_timestamp(record.get("timestamp"))?;

    let prompt_id = update
        .get("prompt_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // prompt_id is the per-turn UUIDv7 (globally unique) — anchoring the dedup
    // key to it (not the file line) survives updates.jsonl rewrites: a rewind
    // truncation shifts surviving events' line numbers, but their prompt_id
    // keys still collide with the store's `(uuid, device_id)` primary key
    // instead of double-counting.
    let turn_key = if prompt_id.is_empty() {
        format!("line{line_no}")
    } else {
        prompt_id.to_string()
    };

    // modelUsage map → one record per model; absent ⇒ top-level counters under
    // an unknown model (pricing layer reconciles the alias). Sorted for
    // deterministic emit order across rescans (object iteration is unspecified).
    let mut per_model: Vec<(String, &serde_json::Value)> = usage
        .get("modelUsage")
        .and_then(|m| m.as_object())
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v)).collect())
        .unwrap_or_default();
    if per_model.is_empty() {
        per_model.push(("unknown".to_string(), usage));
    }
    per_model.sort_by(|a, b| a.0.cmp(&b.0));

    let mut events = Vec::new();
    for (model, counters) in per_model {
        let n = |k: &str| counters.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let input = n("inputTokens");
        let output = n("outputTokens");
        let cached = n("cachedReadTokens");
        // inputTokens is cache-inclusive; normalize to fresh. outputTokens
        // already includes reasoningTokens — do not add them.
        let (fresh_input, clamped_cache_read) = normalize_cache_inclusive(input, cached);
        if fresh_input == 0 && output == 0 && clamped_cache_read == 0 {
            continue; // nothing billable for this model this turn
        }
        events.push(RawUsage {
            uuid: format!("grok:turn:{session_id}:{turn_key}:{model}"),
            timestamp: timestamp.clone(),
            model,
            source: "grok_cli".to_string(),
            tokens: TokenCounts {
                input: fresh_input,
                output,
                cache_creation: 0,
                cache_read: clamped_cache_read,
            },
            server_tool_use: ServerToolUse::default(),
            stop_reason: String::new(),
            service_tier: String::new(),
            iterations: 0,
        });
    }
    Some(events)
}

/// Parse the notification's top-level `timestamp`. Grok writes epoch seconds as
/// a number (defensively treating >1e11 as milliseconds); an RFC3339 string is
/// accepted as a fallback. Returns `None` if absent or unparseable.
fn parse_grok_timestamp(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        let secs = if n > 100_000_000_000 { n / 1000 } else { n };
        return Some(crate::time::epoch_to_iso(secs));
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| {
            dt.with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn grok_event_line(epoch: i64, prompt_id: &str, model_usage: &str) -> String {
        format!(
            r#"{{"timestamp":{epoch},"method":"_x.ai/session/update","params":{{"update":{{"sessionUpdate":"turn_completed","prompt_id":"{prompt_id}","usage":{{"modelUsage":{{{model_usage}}}}}}}}}}}"#
        )
    }

    /// One model's counters, deliberately carrying `reasoningTokens` (must NOT
    /// be added to output), `apiDurationMs`, and `costUsdTicks` (both ignored).
    fn grok_model(model: &str, input: u64, output: u64, cached: u64) -> String {
        format!(
            r#""{model}":{{"inputTokens":{input},"outputTokens":{output},"cachedReadTokens":{cached},"reasoningTokens":3,"modelCalls":1,"apiDurationMs":1000,"costUsdTicks":999}}"#
        )
    }

    fn write_grok_session(dir: &Path, session_id: &str, lines: &[String]) -> PathBuf {
        let session_dir = dir.join("sessions").join("enc-project").join(session_id);
        std::fs::create_dir_all(&session_dir).unwrap();
        let path = session_dir.join("updates.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        path
    }

    #[test]
    fn grok_discover_missing_dir_returns_empty() {
        let base = tempfile::tempdir().unwrap();
        let p = GrokProvider::with_dir(base.path().join("nope"));
        assert!(p.discover().unwrap().is_empty());
    }

    #[test]
    fn grok_parses_turn_completed_and_ignores_noise() {
        let dir = tempfile::tempdir().unwrap();
        let lines = vec![
            // Wrong method → filtered.
            r#"{"timestamp":1700000000,"method":"session/update","params":{"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":1}}}}"#.to_string(),
            // Mid-turn snapshot: has usage but is not turn_completed → dropped
            // (would double-count a partial turn alongside its turn_completed).
            r#"{"timestamp":1700000000,"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"usage_snapshot","prompt_id":"px","usage":{"inputTokens":9999,"outputTokens":9}}}}"#.to_string(),
            // Malformed JSON → counts as skipped.
            "not json".to_string(),
            grok_event_line(1_700_000_000, "p1", &grok_model("grok-4.5-build", 16632, 104, 0)),
        ];
        write_grok_session(dir.path(), "s1", &lines);
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.source, "grok_cli");
        assert_eq!(result.events.len(), 1);
        assert_eq!(
            result.lines_skipped, 1,
            "only the malformed line counts as skipped"
        );
        let ev = &result.events[0];
        assert_eq!(ev.uuid, "grok:turn:s1:p1:grok-4.5-build");
        assert_eq!(ev.model, "grok-4.5-build");
        assert_eq!(ev.tokens.input, 16632);
        assert_eq!(ev.tokens.output, 104, "reasoningTokens (3) NOT added");
        assert_eq!(ev.tokens.cache_read, 0);
        assert_eq!(ev.tokens.cache_creation, 0);
        assert_eq!(ev.timestamp, "2023-11-14T22:13:20.000Z");
    }

    /// Each turn_completed is an independent per-turn total — never diffed.
    /// Diffing would shrink turn 2 to a tiny delta (the bug CC-Switch hit).
    #[test]
    fn grok_records_each_turn_at_face_value_no_diff() {
        let dir = tempfile::tempdir().unwrap();
        let lines = vec![
            grok_event_line(
                1_700_000_000,
                "p1",
                &grok_model("grok-4.5-build", 17294, 28, 11136),
            ),
            grok_event_line(
                1_700_000_060,
                "p2",
                &grok_model("grok-4.5-build", 17347, 56, 17280),
            ),
        ];
        write_grok_session(dir.path(), "s2", &lines);
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 2);
        let by_prompt: std::collections::HashMap<&str, &RawUsage> =
            result.events.iter().map(|e| (e.uuid.as_str(), e)).collect();
        // Face value, cache-inclusive input normalized to fresh.
        let p1 = by_prompt["grok:turn:s2:p1:grok-4.5-build"];
        let p2 = by_prompt["grok:turn:s2:p2:grok-4.5-build"];
        assert_eq!(p1.tokens.input, 17294 - 11136);
        assert_eq!(p1.tokens.cache_read, 11136);
        assert_eq!(p2.tokens.input, 17347 - 17280);
        assert_eq!(p2.tokens.cache_read, 17280);
    }

    #[test]
    fn grok_identical_turns_both_counted() {
        let dir = tempfile::tempdir().unwrap();
        let lines = vec![
            grok_event_line(
                1_700_000_000,
                "p1",
                &grok_model("grok-4.5-build", 100, 10, 0),
            ),
            grok_event_line(
                1_700_000_060,
                "p2",
                &grok_model("grok-4.5-build", 100, 10, 0),
            ),
        ];
        write_grok_session(dir.path(), "s3", &lines);
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(
            result.events.len(),
            2,
            "identical turns are two real usages, not a zero delta"
        );
    }

    #[test]
    fn grok_multi_model_emits_one_row_per_model() {
        let dir = tempfile::tempdir().unwrap();
        let both = format!(
            "{},{}",
            grok_model("grok-4.5-build", 100, 10, 0),
            grok_model("grok-4.3", 30, 3, 10),
        );
        let lines = vec![grok_event_line(1_700_000_000, "p1", &both)];
        write_grok_session(dir.path(), "s4", &lines);
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 2);
        // Deterministic order: sorted by model name.
        assert!(result.events[0].uuid.ends_with(":grok-4.3"));
        assert!(result.events[1].uuid.ends_with(":grok-4.5-build"));
        let g43 = &result.events[0];
        assert_eq!(g43.tokens.input, 20, "cache-inclusive 30 minus cached 10");
        assert_eq!(g43.tokens.cache_read, 10);
    }

    #[test]
    fn grok_missing_model_usage_falls_back_to_top_level() {
        let dir = tempfile::tempdir().unwrap();
        let line = r#"{"timestamp":1700000000,"method":"_x.ai/session/update","params":{"update":{"prompt_id":"p1","usage":{"inputTokens":100,"outputTokens":10,"cachedReadTokens":5}}}}"#.to_string();
        write_grok_session(dir.path(), "s5", &[line]);
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].model, "unknown");
        assert_eq!(result.events[0].tokens.input, 95);
        assert_eq!(result.events[0].tokens.cache_read, 5);
    }

    #[test]
    fn grok_archived_sessions_are_also_discovered() {
        let dir = tempfile::tempdir().unwrap();
        // Same filename under archived_sessions/ must be picked up too.
        let arch = dir
            .path()
            .join("archived_sessions")
            .join("enc")
            .join("arch1");
        std::fs::create_dir_all(&arch).unwrap();
        std::fs::write(
            arch.join("updates.jsonl"),
            grok_event_line(1_700_000_000, "p1", &grok_model("grok-4.5-build", 10, 1, 0)),
        )
        .unwrap();
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].uuid, "grok:turn:arch1:p1:grok-4.5-build");
    }

    #[test]
    fn grok_incremental_emits_only_appended_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_grok_session(
            dir.path(),
            "s6",
            &[grok_event_line(
                1_700_000_000,
                "p1",
                &grok_model("grok-4.5-build", 100, 10, 0),
            )],
        );
        let p = GrokProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        std::thread::sleep(std::time::Duration::from_millis(20));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(
                f,
                "{}",
                grok_event_line(
                    1_700_000_060,
                    "p2",
                    &grok_model("grok-4.5-build", 250, 30, 0)
                )
            )
            .unwrap();
        }
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 1);
        assert_eq!(r2.events[0].uuid, "grok:turn:s6:p2:grok-4.5-build");
    }
}
