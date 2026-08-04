//! Codex (`~/.codex`) session-log provider.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

use super::{
    collect_jsonl_incremental, normalize_cache_inclusive, CollectResult, FileParseOutcome,
    Provider, RawUsage, ScanProgress, ScanProgressDelta,
};

/// Codex (`~/.codex`) session-log provider.
///
/// Reads `<codex_dir>/sessions/**/*.jsonl` (depth ≤ 3, i.e. `YYYY/MM/DD`) and
/// `<codex_dir>/archived_sessions/*.jsonl` (flat). Only `session_meta`,
/// `turn_context`, and `event_msg` (subtype `token_count`) events are consumed.
///
/// Codex's `total_token_usage` is **cumulative** and its `input_tokens` is
/// cache-inclusive, so the provider computes per-call deltas and subtracts
/// `cache_read` to yield a fresh `input` (parse-time fresh-input
/// normalization). Sub-agent / fork logs replay the parent thread's history
/// before their own usage; that replay only re-establishes the cumulative
/// baseline and is never emitted.
pub struct CodexProvider {
    codex_dir: PathBuf,
}

impl CodexProvider {
    /// Default provider rooted at `~/.codex`.
    pub fn new() -> AppResult<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| AppError::Provider("cannot resolve home dir".into()))?;
        Ok(Self {
            codex_dir: home.join(".codex"),
        })
    }

    /// Test/override constructor with an explicit codex dir.
    #[cfg(test)]
    pub(crate) fn with_dir(dir: PathBuf) -> Self {
        Self { codex_dir: dir }
    }

    fn discover_in(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let sessions = self.codex_dir.join("sessions");
        if sessions.is_dir() {
            collect_codex_jsonl_recursive(&sessions, &mut files, 0, 3);
        }
        let archived = self.codex_dir.join("archived_sessions");
        if archived.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&archived) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        files.push(path);
                    }
                }
            }
        }
        files
    }
}

impl Provider for CodexProvider {
    fn name(&self) -> &'static str {
        "codex_cli"
    }

    fn discover(&self) -> AppResult<Vec<PathBuf>> {
        if !self.codex_dir.exists() {
            return Ok(Vec::new());
        }
        Ok(self.discover_in())
    }

    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult> {
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
            let (identity, boundary) = prescan_codex_text(&text);
            let parsed = parse_codex_text(&text, identity, boundary, 0);
            events.extend(parsed.events);
            skipped += parsed.skipped;
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

    /// Incremental collect: mtime-gate unchanged files; for a changed file,
    /// re-parse it fully to rebuild the cumulative baseline + event_index, but
    /// only EMIT events past the recorded cursor. The baseline cannot be cached
    /// (it depends on every prior line), so old lines are still parsed — the
    /// saving is skipping unchanged files entirely + not re-emitting seen rows.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        collect_jsonl_incremental(self, progress, |_file: &Path, text, start_line| {
            let (identity, boundary) = prescan_codex_text(text);
            // `start_line` is the cursor (self-healed on truncation by the
            // driver); parse_codex_text's `emit_after_line` has the same "skip
            // lines at or before it" semantics.
            let parsed = parse_codex_text(text, identity, boundary, start_line);
            FileParseOutcome {
                events: parsed.events,
                turn_durations: Vec::new(),
                skipped: parsed.skipped,
            }
        })
    }
}

// ---- Codex parsing internals (pure, ported from CC-Switch's scanner) ----

/// Cumulative token usage tracked across a file (the `total_token_usage` field).
#[derive(Debug, Clone, Default)]
struct CumulativeTokens {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// Per-call delta derived from two cumulative snapshots.
#[derive(Debug)]
struct DeltaTokens {
    input: u32,
    cached_input: u32,
    output: u32,
}

impl DeltaTokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.cached_input == 0 && self.output == 0
    }
}

/// Per-file parse state advanced line by line.
struct CodexFileState {
    thread_id: Option<String>,
    current_model: String,
    prev_total: Option<CumulativeTokens>,
    event_index: u32,
}

/// A Codex session's identity: its unique thread id + whether it carries a
/// replayed parent-thread history snapshot (sub-agent or fork).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSessionIdentity {
    thread_id: String,
    carries_history_snapshot: bool,
}

/// Result of parsing one Codex file's text.
struct CodexParsed {
    events: Vec<RawUsage>,
    /// History-replay snapshot events beyond the emit cursor (diagnostic).
    skipped: u32,
}

/// One pre-scan pass over the file text: recover the session identity (first
/// `session_meta`) and — only if that session carries a history snapshot — the
/// 1-based line number of the first takeover event (`thread_settings_applied`
/// or `inter_agent_communication*`), before which token events are replay.
fn prescan_codex_text(text: &str) -> (Option<CodexSessionIdentity>, Option<i64>) {
    let mut identity = None;
    let mut boundary = None;
    for (index, line) in text.lines().enumerate() {
        if identity.is_none() && line.contains("\"session_meta\"") {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if value.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
                    if let Some(id) = value.get("payload").and_then(parse_codex_session_identity) {
                        identity = Some(id);
                    }
                }
            }
        }
        if boundary.is_none()
            && (line.contains("\"thread_settings_applied\"")
                || line.contains("\"inter_agent_communication"))
        {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(event_type) = value.get("type").and_then(|v| v.as_str()) {
                    let is_boundary = event_type.starts_with("inter_agent_communication")
                        || (event_type == "event_msg"
                            && value
                                .get("payload")
                                .and_then(|p| p.get("type"))
                                .and_then(|v| v.as_str())
                                == Some("thread_settings_applied"));
                    if is_boundary {
                        boundary = Some(index as i64 + 1);
                    }
                }
            }
        }
    }
    let boundary = identity.as_ref().and_then(|id| {
        if id.carries_history_snapshot {
            boundary
        } else {
            None
        }
    });
    (identity, boundary)
}

/// Parse a file's text into raw events. `emit_after_line` is the 1-based cursor:
/// events at or before it rebuild state but are not re-emitted (0 ⇒ emit all).
fn parse_codex_text(
    text: &str,
    identity: Option<CodexSessionIdentity>,
    history_replay_boundary: Option<i64>,
    emit_after_line: i64,
) -> CodexParsed {
    let lines: Vec<&str> = text.lines().collect();

    let mut state = CodexFileState {
        thread_id: identity.map(|i| i.thread_id),
        current_model: "unknown".to_string(),
        prev_total: None,
        event_index: 0,
    };
    let mut events = Vec::new();
    let mut skipped = 0u32;

    for (idx, line) in lines.iter().enumerate() {
        let line_offset = idx as i64 + 1; // 1-based, matching the cursor
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Fast filter before serde.
        let is_event_msg = line.contains("\"event_msg\"");
        let is_turn_context = line.contains("\"turn_context\"");
        let is_session_meta = line.contains("\"session_meta\"");
        if !is_event_msg && !is_turn_context && !is_session_meta {
            continue;
        }
        if is_event_msg && !line.contains("\"token_count\"") {
            continue;
        }

        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(event_type) = value.get("type").and_then(|t| t.as_str()) else {
            continue;
        };

        match event_type {
            "session_meta" if state.thread_id.is_none() => {
                state.thread_id = value
                    .get("payload")
                    .and_then(parse_codex_session_identity)
                    .map(|i| i.thread_id);
            }
            "turn_context" => {
                if let Some(payload) = value.get("payload") {
                    if let Some(model) = payload
                        .get("model")
                        .or_else(|| payload.get("info").and_then(|i| i.get("model")))
                        .and_then(|v| v.as_str())
                    {
                        state.current_model = crate::model::normalize_model_key(model);
                    }
                }
            }
            "event_msg" => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                    continue;
                }
                let info = match payload.get("info") {
                    Some(i) if !i.is_null() => i,
                    _ => continue, // first event often has null info
                };
                if let Some(model) = info
                    .get("model")
                    .or_else(|| info.get("model_name"))
                    .or_else(|| payload.get("model"))
                    .and_then(|v| v.as_str())
                {
                    state.current_model = crate::model::normalize_model_key(model);
                }
                // Prefer cumulative total_token_usage; fall back to last_token_usage
                // (already a per-call delta).
                let (cumulative, is_total) = if let Some(total) = info.get("total_token_usage") {
                    (parse_cumulative_tokens(total), true)
                } else if let Some(last) = info.get("last_token_usage") {
                    (parse_cumulative_tokens(last), false)
                } else {
                    continue;
                };
                let Some(cumulative) = cumulative else {
                    continue;
                };
                let mut delta = if is_total {
                    let d = compute_delta(&state.prev_total, &cumulative);
                    state.prev_total = Some(cumulative);
                    d
                } else {
                    DeltaTokens {
                        input: cumulative.input as u32,
                        cached_input: cumulative.cached_input as u32,
                        output: cumulative.output as u32,
                    }
                };
                // Clamp before the zero-gate below: an abnormal delta (input 0,
                // cached > 0) must read as zero so it is skipped. The shared
                // normalizer re-clamps (idempotently) when building the event.
                delta.cached_input = delta.cached_input.min(delta.input);
                if delta.is_zero() {
                    continue; // task-boundary snapshot, no new usage
                }
                // Every non-zero event occupies a stable sequence number — line
                // numbers drift if the file is edited, this does not.
                state.event_index += 1;

                // History replay only re-establishes the baseline — never emit.
                if is_history_snapshot_event(history_replay_boundary, line_offset) {
                    if line_offset > emit_after_line {
                        skipped += 1;
                    }
                    continue;
                }
                // Already-synced lines rebuild state but are not re-emitted.
                if line_offset <= emit_after_line {
                    continue;
                }

                let thread_id = state.thread_id.as_deref().unwrap_or("unknown");
                let timestamp = value
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                // Codex input is cache-inclusive — normalize to fresh via the
                // shared helper (clamp already applied above for the zero-gate).
                let (fresh_input, clamped_cache_read) =
                    normalize_cache_inclusive(delta.input, delta.cached_input);
                events.push(RawUsage {
                    uuid: format!("codex:thread-v1:{thread_id}:{}", state.event_index),
                    timestamp: timestamp.unwrap_or_else(crate::time::now_iso),
                    model: state.current_model.clone(),
                    source: "codex_cli".to_string(),
                    tokens: TokenCounts {
                        input: fresh_input,
                        output: delta.output,
                        cache_creation: 0,
                        cache_read: clamped_cache_read,
                    },
                    server_tool_use: ServerToolUse::default(),
                    stop_reason: String::new(),
                    service_tier: String::new(),
                    iterations: 0,
                });
            }
            _ => {}
        }
    }

    CodexParsed { events, skipped }
}

fn is_history_snapshot_event(boundary: Option<i64>, line_offset: i64) -> bool {
    boundary.is_some_and(|b| line_offset < b)
}

/// Recursive `.jsonl` discovery with a depth cap (Codex nests `YYYY/MM/DD`).
fn collect_codex_jsonl_recursive(dir: &Path, files: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_codex_jsonl_recursive(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

/// Extract the session identity from a `session_meta` payload. The `id` is the
/// unique thread id; `session_id` points at the parent thread for sub-agents.
fn parse_codex_session_identity(payload: &serde_json::Value) -> Option<CodexSessionIdentity> {
    let thread_id = payload
        .get("id")
        .or_else(|| payload.get("thread_id"))
        .or_else(|| payload.get("threadId"))
        .or_else(|| payload.get("session_id"))
        .or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str())?
        .to_string();
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str());
    let carries_history_snapshot = payload
        .get("forked_from_id")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.is_empty())
        || payload
            .get("source")
            .and_then(|s| s.get("subagent"))
            .is_some()
        || session_id.is_some_and(|sid| sid != thread_id);
    Some(CodexSessionIdentity {
        thread_id,
        carries_history_snapshot,
    })
}

/// Delta between two cumulative snapshots (saturating to guard against the
/// current falling below the previous — abnormal but non-fatal).
fn compute_delta(prev: &Option<CumulativeTokens>, current: &CumulativeTokens) -> DeltaTokens {
    match prev {
        None => DeltaTokens {
            input: current.input as u32,
            cached_input: current.cached_input as u32,
            output: current.output as u32,
        },
        Some(p) => DeltaTokens {
            input: current.input.saturating_sub(p.input) as u32,
            cached_input: current.cached_input.saturating_sub(p.cached_input) as u32,
            output: current.output.saturating_sub(p.output) as u32,
        },
    }
}

/// Extract cumulative tokens from a `total_token_usage` / `last_token_usage`
/// object. `cached_input_tokens` and `cache_read_input_tokens` are both
/// accepted (field name varies across Codex versions).
fn parse_cumulative_tokens(total_usage: &serde_json::Value) -> Option<CumulativeTokens> {
    if total_usage.is_null() || !total_usage.is_object() {
        return None;
    }
    Some(CumulativeTokens {
        input: total_usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cached_input: total_usage
            .get("cached_input_tokens")
            .or_else(|| total_usage.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output: total_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_jsonl(path: &Path, values: &[serde_json::Value]) {
        let contents = values
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(path, contents).unwrap();
    }

    fn codex_session_meta(thread_id: &str, session_id: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "session_id": session_id,
                "source": if thread_id == session_id {
                    serde_json::Value::String("cli".to_string())
                } else {
                    serde_json::json!({ "subagent": {} })
                }
            }
        })
    }

    fn codex_turn_context(model: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:01Z",
            "type": "turn_context",
            "payload": { "model": model }
        })
    }

    fn codex_token_count(input: u64, cached: u64, output: u64) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:02Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": { "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output
                }}
            }
        })
    }

    /// Codex model names flow through `crate::model::normalize_model_key` (the
    /// shared superset normalizer). This pins the provider's observable
    /// normalization contract: lowercase, strip `provider/` prefix, and strip
    /// `-YYYY-MM-DD` / `-YYYYMMDD` date suffixes.
    #[test]
    fn codex_normalize_model_lowercase_prefix_and_dates() {
        let norm = crate::model::normalize_model_key;
        assert_eq!(norm("GLM-4.6"), "glm-4.6");
        assert_eq!(norm("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(norm("OPENAI/GPT-5.4"), "gpt-5.4");
        assert_eq!(norm("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(norm("gpt-5.4-pro-2026-03-05"), "gpt-5.4-pro");
        assert_eq!(norm("gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(norm("claude-opus-4-6-20260206"), "claude-opus-4-6");
        assert_eq!(norm("openai/GPT-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(norm("openai/gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(norm("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(norm("o3"), "o3");
    }

    #[test]
    fn codex_compute_delta_first_subsequent_zero_saturating() {
        let first = compute_delta(
            &None,
            &CumulativeTokens {
                input: 17934,
                cached_input: 9600,
                output: 454,
            },
        );
        assert_eq!(first.input, 17934);
        assert_eq!(first.cached_input, 9600);
        assert_eq!(first.output, 454);
        let next = compute_delta(
            &Some(CumulativeTokens {
                input: 17934,
                cached_input: 9600,
                output: 454,
            }),
            &CumulativeTokens {
                input: 36722,
                cached_input: 27904,
                output: 804,
            },
        );
        assert_eq!(next.input, 36722 - 17934);
        assert_eq!(next.cached_input, 27904 - 9600);
        assert_eq!(next.output, 804 - 454);
        // task boundary: identical cumulative ⇒ zero delta.
        let zero = compute_delta(
            &Some(CumulativeTokens {
                input: 58346,
                cached_input: 46976,
                output: 1045,
            }),
            &CumulativeTokens {
                input: 58346,
                cached_input: 46976,
                output: 1045,
            },
        );
        assert!(zero.is_zero());
        // abnormal: current < previous ⇒ saturates to zero.
        let sat = compute_delta(
            &Some(CumulativeTokens {
                input: 100,
                cached_input: 50,
                output: 30,
            }),
            &CumulativeTokens {
                input: 80,
                cached_input: 40,
                output: 20,
            },
        );
        assert!(sat.is_zero());
    }

    #[test]
    fn codex_parse_cumulative_tokens_variants() {
        let v: serde_json::Value = serde_json::json!({
            "input_tokens": 17934, "cached_input_tokens": 9600, "output_tokens": 454,
            "reasoning_output_tokens": 233, "total_tokens": 18388
        });
        let t = parse_cumulative_tokens(&v).unwrap();
        assert_eq!(t.input, 17934);
        assert_eq!(t.cached_input, 9600);
        assert_eq!(t.output, 454);
        assert!(parse_cumulative_tokens(&serde_json::Value::Null).is_none());
        // alt field name cache_read_input_tokens.
        let alt: serde_json::Value = serde_json::json!({
            "input_tokens": 1000, "cache_read_input_tokens": 500, "output_tokens": 200
        });
        assert_eq!(parse_cumulative_tokens(&alt).unwrap().cached_input, 500);
    }

    #[test]
    fn codex_cached_clamped_to_input() {
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 0,
            output: 50,
        });
        let current = CumulativeTokens {
            input: 110,
            cached_input: 80,
            output: 60,
        };
        let mut delta = compute_delta(&prev, &current);
        // before clamp: input delta 10, cached delta 80 (abnormal, > input)
        assert_eq!(delta.input, 10);
        assert_eq!(delta.cached_input, 80);
        delta.cached_input = delta.cached_input.min(delta.input);
        assert_eq!(delta.cached_input, 10);
    }

    #[test]
    fn codex_discover_missing_dir_returns_empty() {
        let base = tempfile::tempdir().unwrap();
        let p = CodexProvider::with_dir(base.path().join("nope"));
        assert!(p.discover().unwrap().is_empty());
    }

    #[test]
    fn codex_subagent_identity_prefers_unique_thread_id() {
        let id = parse_codex_session_identity(
            codex_session_meta("child", "parent")
                .get("payload")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(id.thread_id, "child");
        assert!(id.carries_history_snapshot);
    }

    /// CC-Switch's `test_subagent_replay_only_establishes_token_baseline`:
    /// the replayed history (lines before `thread_settings_applied`) only sets
    /// the cumulative baseline; the child's own usage is the post-boundary delta.
    /// CC-Switch stores input=100 (cache-inclusive); VaultOne normalizes to
    /// fresh at parse ⇒ input = 100 − 50 = 50 (the documented Codex divergence).
    #[test]
    fn codex_subagent_replay_emits_only_child_usage_with_fresh_input() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("07")
            .join("child.jsonl");
        std::fs::create_dir_all(child.parent().unwrap()).unwrap();
        write_jsonl(
            &child,
            &[
                codex_session_meta("child", "parent"),
                codex_turn_context("gpt-5.6-sol"),
                codex_token_count(1_000, 900, 100),
                codex_token_count(1_200, 1_000, 120),
                serde_json::json!({
                    "timestamp": "2026-07-10T03:00:03Z",
                    "type": "event_msg",
                    "payload": { "type": "thread_settings_applied" }
                }),
                codex_token_count(1_300, 1_050, 150),
            ],
        );
        let p = CodexProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.source, "codex_cli");
        assert_eq!(
            result.events.len(),
            1,
            "only the post-boundary event is emitted"
        );
        // 2 replay snapshots counted as skipped.
        assert_eq!(result.lines_skipped, 2);
        let ev = &result.events[0];
        assert_eq!(ev.uuid, "codex:thread-v1:child:3");
        assert_eq!(ev.model, "gpt-5.6-sol");
        // fresh input = cache-inclusive delta (100) − cache_read (50).
        assert_eq!(ev.tokens.input, 50);
        assert_eq!(ev.tokens.cache_read, 50);
        assert_eq!(ev.tokens.output, 30);
        assert_eq!(ev.tokens.cache_creation, 0);
    }

    #[test]
    fn codex_subagents_under_same_parent_get_distinct_ids() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let a = sessions.join("a.jsonl");
        let b = sessions.join("b.jsonl");
        write_jsonl(
            &a,
            &[
                codex_session_meta("child-a", "parent"),
                codex_turn_context("gpt-5.6-sol"),
                codex_token_count(100, 50, 10),
            ],
        );
        write_jsonl(
            &b,
            &[
                codex_session_meta("child-b", "parent"),
                codex_turn_context("gpt-5.6-sol"),
                codex_token_count(200, 100, 20),
            ],
        );
        let p = CodexProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        let mut uuids: Vec<String> = result.events.iter().map(|e| e.uuid.clone()).collect();
        uuids.sort();
        assert_eq!(
            uuids,
            vec![
                "codex:thread-v1:child-a:1".to_string(),
                "codex:thread-v1:child-b:1".to_string()
            ]
        );
        // fresh inputs: 100−50=50, 200−100=100.
        let by_thread: std::collections::HashMap<&str, u32> = result
            .events
            .iter()
            .map(|e| (e.uuid.rsplit(':').nth(1).unwrap(), e.tokens.input))
            .collect();
        assert_eq!(by_thread["child-a"], 50);
        assert_eq!(by_thread["child-b"], 100);
    }

    #[test]
    fn codex_incremental_emits_only_appended_events() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sessions").join("s.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        write_jsonl(
            &file,
            &[
                codex_session_meta("t", "t"),
                codex_turn_context("gpt-5.6-sol"),
                codex_token_count(100, 50, 10),
            ],
        );
        let p = CodexProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        // Append a second token event — content change bumps mtime past the gate.
        std::thread::sleep(std::time::Duration::from_millis(20));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file)
                .unwrap();
            writeln!(f, "{}", codex_token_count(300, 100, 40)).unwrap();
        }
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        // Only the appended event is emitted (fresh input 200−50=150).
        assert_eq!(r2.events.len(), 1);
        assert!(r2.events[0].uuid.ends_with(":2"));
        assert_eq!(r2.events[0].tokens.input, 150);
        assert_eq!(r2.events[0].tokens.cache_read, 50);
        assert_eq!(r2.events[0].tokens.output, 30);
    }

    #[test]
    fn codex_incremental_truncation_self_heals() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sessions").join("s.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        write_jsonl(
            &file,
            &[
                codex_session_meta("t", "t"),
                codex_turn_context("gpt-5.6-sol"),
                codex_token_count(100, 50, 10),
            ],
        );
        let p = CodexProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        // Truncate to fewer lines than the cursor (3) and rewrite with a fresh
        // token event. Without self-heal the stale cursor would make every line
        // "already synced" and the new event would be silently dropped.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_jsonl(
            &file,
            &[codex_session_meta("t", "t"), codex_token_count(200, 0, 20)],
        );
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 1);
        assert_eq!(r2.events[0].tokens.input, 200);
        assert_eq!(r2.events[0].tokens.output, 20);
    }
}
