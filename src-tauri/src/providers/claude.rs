//! Claude Code session-log provider.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

use super::{
    collect_jsonl_incremental, CollectResult, Provider, RawTurnDuration, RawUsage, ScanProgress,
    ScanProgressDelta,
};

/// Claude Code session-log provider.
///
/// Reads `~/.claude/projects/**/*.jsonl`; each line is a JSON event. Assistant
/// events carry `message.usage` (token four-pack + server tool use + service
/// tier + iterations) and `message.stop_reason`. `system` events with
/// `subtype:"turn_duration"` carry `durationMs`. Top-level `timestamp` and
/// `uuid` identify each event.
pub struct ClaudeCodeProvider {
    /// Root of the Claude projects dir (overridable for tests).
    projects_dir: PathBuf,
}

impl ClaudeCodeProvider {
    /// Default provider rooted at `~/.claude/projects`.
    pub fn new() -> AppResult<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| AppError::Provider("cannot resolve home dir".into()))?;
        Ok(Self {
            projects_dir: home.join(".claude").join("projects"),
        })
    }

    /// Test/override constructor with an explicit projects dir.
    #[cfg(test)]
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { projects_dir: dir }
    }

    /// Fold one JSONL line into the running accumulators, returning whether it
    /// was skipped (unparseable). Shared by the full `parse` and the gated
    /// `collect_incremental` so the message-id dedup + event classification
    /// policy lives in one place (one assistant response may span several
    /// content-block events that all repeat the full usage; one message id ⇒
    /// one record, falling back to the event uuid when the source omits one).
    fn fold_line(
        raw: &str,
        events_by_mid: &mut std::collections::HashMap<String, RawUsage>,
        turn_durations: &mut Vec<RawTurnDuration>,
    ) -> bool {
        let line = raw.trim();
        if line.is_empty() {
            return false;
        }
        match serde_json::from_str::<SessionEvent>(line) {
            Ok(ev) => {
                let mid = ev.message.as_ref().and_then(|m| m.id.clone());
                match ev.classify() {
                    Parsed::Usage(u) => {
                        let key = mid.unwrap_or_else(|| u.uuid.clone());
                        // One message id ⇒ one record, but pick the BEST snapshot:
                        // a `message_start` event (output=1, no stop_reason) often
                        // lands before the final block (full output + stop_reason).
                        // First-wins would freeze the snapshot and systematically
                        // undercount output. Prefer a non-empty stop_reason; on a
                        // tie take the larger output_tokens.
                        events_by_mid
                            .entry(key)
                            .and_modify(|e| {
                                if should_replace(e, &u) {
                                    *e = u.clone();
                                }
                            })
                            .or_insert(u);
                    }
                    Parsed::TurnDuration(td) => turn_durations.push(td),
                    Parsed::Skip => {}
                }
                false
            }
            Err(_) => true,
        }
    }

    /// Fold one JSONL file's text into a per-file parse outcome. Lines at or
    /// before `start_line` (1-based, the incremental cursor) are skipped. Dedup
    /// is scoped per file: Claude Code writes one session per jsonl and a
    /// message id is unique within that file, so per-file is the correct scope —
    /// and it is the scope the production incremental path uses. Sharing this
    /// fold between `parse` and `collect_incremental` makes the test and
    /// production paths run the same code (architecture review #10: previously
    /// `parse` cross-file-deduped while `collect_incremental` per-file-deduped).
    ///
    /// Invariant: the stored `RawUsage.uuid` is the source **event** uuid, NOT
    /// the dedup key (the message id) — re-keying stored rows to the message id
    /// would mass-duplicate on first run. The message id is the map key only.
    fn fold_file(text: &str, start_line: i64) -> super::FileParseOutcome {
        let mut events_by_mid: std::collections::HashMap<String, RawUsage> =
            std::collections::HashMap::new();
        let mut turn_durations = Vec::new();
        let mut skipped = 0u32;
        for (idx, line) in text.lines().enumerate() {
            let line_no = idx as i64 + 1; // 1-based, matching the cursor
            if line_no <= start_line {
                continue;
            }
            if Self::fold_line(line, &mut events_by_mid, &mut turn_durations) {
                skipped += 1;
            }
        }
        super::FileParseOutcome {
            events: events_by_mid.into_values().collect(),
            turn_durations,
            skipped,
        }
    }
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            projects_dir: PathBuf::from(".claude/projects"),
        })
    }
}

impl Provider for ClaudeCodeProvider {
    fn name(&self) -> &'static str {
        "claude_code"
    }

    fn discover(&self) -> AppResult<Vec<PathBuf>> {
        if !self.projects_dir.exists() {
            // No Claude Code sessions on this machine yet — not an error.
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&self.projects_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path.to_path_buf());
            }
        }
        Ok(out)
    }

    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult> {
        // Dedup key = Anthropic message id. Claude Code writes each content
        // block of one assistant response (thinking / text / each tool_use) as
        // a separate event that repeats the full message.usage; without dedup
        // one API call becomes N records and tokens/cost inflate N× (observed
        // ~3.6× on CC-Switch/GLM transit logs). One message id ⇒ one record.
        // Per-file dedup via `fold_file` — the same fold the production
        // incremental path uses, so test and production run identical logic.
        // A message id never spans files (one session per jsonl), so per-file
        // vs cross-file is observably identical here; sharing the fold is what
        // matters.
        let mut events = Vec::new();
        let mut turn_durations = Vec::new();
        let mut skipped = 0u32;
        for file in files {
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let outcome = Self::fold_file(&text, 0);
            events.extend(outcome.events);
            turn_durations.extend(outcome.turn_durations);
            skipped += outcome.skipped;
        }
        // Deterministic order (timestamp, then uuid) so repeated parses of the
        // same sources yield identical artifact lines.
        events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations,
            files_scanned: files.len() as u32,
            lines_skipped: skipped,
        })
    }

    /// Incremental collect: parse only lines past each file's recorded cursor
    /// and return the advanced cursors to persist. The mtime gate skips
    /// unchanged files (no IO/serde); a never-seen file ({0,0}) falls through to
    /// a full parse on first sight. Delegates to the shared JSONL driver — the
    /// per-file fold is `fold_file`, the same one `parse` uses, so dedup scope
    /// and event classification are identical between the two paths.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        collect_jsonl_incremental(self, progress, |_file: &Path, text, start_line| {
            Self::fold_file(text, start_line)
        })
    }
}

// ---- Lenient session-log deserialization ----
//
// Tolerant by design: every field is optional and unknown fields are ignored,
// so a malformed or schema-drifted line is skipped (counted), never fatal.

#[derive(serde::Deserialize)]
struct SessionEvent {
    #[serde(rename = "type")]
    typ: Option<String>,
    timestamp: Option<String>,
    uuid: Option<String>,
    subtype: Option<String>,
    /// `durationMs` on `system/turn_duration` events.
    #[serde(rename = "durationMs", default)]
    duration_ms: Option<u32>,
    message: Option<SessionMessage>,
}

#[derive(serde::Deserialize)]
struct SessionMessage {
    /// Anthropic message id (e.g. `msg_…`). Shared by every content-block event
    /// of one assistant response — the per-call dedup key (one API call ⇒ one
    /// message id).
    id: Option<String>,
    model: Option<String>,
    usage: Option<SessionUsage>,
    stop_reason: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct SessionUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
    #[serde(default)]
    server_tool_use: Option<SessionServerTool>,
    #[serde(default)]
    service_tier: Option<String>,
    /// Iteration entries; we keep only the count (lean).
    #[serde(default)]
    iterations: Option<Vec<serde_json::Value>>,
}

#[derive(serde::Deserialize, Default)]
struct SessionServerTool {
    #[serde(default)]
    web_search_requests: u32,
    #[serde(default)]
    web_fetch_requests: u32,
}

/// How a parsed event should be routed.
enum Parsed {
    Usage(RawUsage),
    TurnDuration(RawTurnDuration),
    Skip,
}

impl SessionEvent {
    /// Classify this event into a usage record, a turn duration, or skip.
    fn classify(self) -> Parsed {
        // Per-turn duration: system event tagged turn_duration.
        if self.typ.as_deref() == Some("system") && self.subtype.as_deref() == Some("turn_duration")
        {
            return match (self.uuid, self.duration_ms) {
                (Some(uuid), Some(duration_ms)) => Parsed::TurnDuration(RawTurnDuration {
                    uuid,
                    timestamp: self.timestamp.unwrap_or_else(crate::time::now_iso),
                    duration_ms,
                }),
                _ => Parsed::Skip,
            };
        }
        // Per-call usage: assistant event with a usable usage block.
        if self.typ.as_deref() == Some("assistant") {
            if let Some(raw) = self.into_usage() {
                return Parsed::Usage(raw);
            }
        }
        Parsed::Skip
    }

    /// Convert to a `RawUsage` iff this assistant event has a usable usage
    /// block. Drops events with no tokens (e.g. pure tool results).
    fn into_usage(self) -> Option<RawUsage> {
        let msg = self.message?;
        let usage = msg.usage?;
        let tokens = TokenCounts {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_creation: usage.cache_creation_input_tokens,
            cache_read: usage.cache_read_input_tokens,
        };
        // Skip degenerate events with zero tokens (no real API usage recorded).
        if tokens.total() == 0 {
            return None;
        }
        let uuid = self.uuid?;
        let timestamp = self.timestamp.unwrap_or_else(crate::time::now_iso);
        let st = usage.server_tool_use.unwrap_or_default();
        Some(RawUsage {
            uuid,
            timestamp,
            model: msg.model.unwrap_or_else(|| "unknown".to_string()),
            source: "claude_code".to_string(),
            tokens,
            server_tool_use: ServerToolUse {
                web_search: st.web_search_requests,
                web_fetch: st.web_fetch_requests,
            },
            stop_reason: msg.stop_reason.unwrap_or_default(),
            service_tier: usage.service_tier.unwrap_or_default(),
            iterations: usage.iterations.map(|v| v.len() as u32).unwrap_or(0),
        })
    }
}

/// Message-id dedup winner policy: prefer the snapshot with a non-empty
/// stop_reason (the final block of an assistant response); on a tie (both or
/// neither have one) take the larger `output_tokens`. Mirrors CC-Switch's
/// Claude session dedup — a `message_start` snapshot otherwise freezes early
/// and undercounts output.
fn should_replace(existing: &RawUsage, candidate: &RawUsage) -> bool {
    let cand_has_reason = !candidate.stop_reason.is_empty();
    let existing_has_reason = !existing.stop_reason.is_empty();
    if cand_has_reason && !existing_has_reason {
        true
    } else if cand_has_reason == existing_has_reason {
        candidate.tokens.output > existing.tokens.output
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    fn write_lines(path: &Path, lines: &[impl AsRef<str>]) {
        let mut f = fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{}", l.as_ref()).unwrap();
        }
    }

    #[test]
    fn parses_assistant_events_and_skips_noise() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        let assistant = r#"{"type":"assistant","timestamp":"2026-07-13T16:55:22.467Z","uuid":"abc-1","message":{"model":"glm-5.2","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5,"service_tier":"standard","iterations":[{},{}],"server_tool_use":{"web_search_requests":2}}}}"#;
        let user = r#"{"type":"user","uuid":"abc-2","message":{}}"#;
        write_lines(&file, &[assistant, user, "", "{not json"]);

        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let files = p.discover().unwrap();
        assert_eq!(files.len(), 1);
        let result = p.parse(&files).unwrap();
        assert_eq!(result.source, "claude_code");
        assert_eq!(result.events.len(), 1);
        assert!(result.turn_durations.is_empty());
        assert_eq!(result.files_scanned, 1);
        // Only the malformed line counts as skipped: the empty line is ignored,
        // and the user row parses but yields no event (silently dropped).
        assert_eq!(result.lines_skipped, 1);

        let ev = &result.events[0];
        assert_eq!(ev.uuid, "abc-1");
        assert_eq!(ev.model, "glm-5.2");
        assert_eq!(ev.tokens.input, 100);
        assert_eq!(ev.tokens.cache_read, 10);
        assert_eq!(ev.server_tool_use.web_search, 2);
        assert_eq!(ev.stop_reason, "tool_use");
        assert_eq!(ev.service_tier, "standard");
        assert_eq!(ev.iterations, 2);
    }

    #[test]
    fn parses_turn_duration_events() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        let td = r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-07-13T16:55:00Z","uuid":"td-1","durationMs":209499}"#;
        let not_td = r#"{"type":"system","subtype":"other","uuid":"x","durationMs":10}"#;
        write_lines(&file, &[td, not_td]);

        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.turn_durations.len(), 1);
        assert_eq!(result.events.len(), 0);
        let td = &result.turn_durations[0];
        assert_eq!(td.uuid, "td-1");
        assert_eq!(td.duration_ms, 209_499);
        assert_eq!(td.timestamp, "2026-07-13T16:55:00Z");
    }

    #[test]
    fn drops_assistant_event_with_zero_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        let zero = concat!(
            r#"{"type":"assistant","timestamp":"2026-07-13T16:55:22.467Z","uuid":"z","#,
            r#""message":{"model":"glm-5.2","usage":{"input_tokens":0,"output_tokens":0}}}"#
        );
        write_lines(&file, &[zero]);
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn dedups_assistant_events_by_message_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        // One assistant call (msg_A) split into a thinking + a tool_use event,
        // both repeating the full usage; a second call (msg_B) is one event.
        // Distinct message ids must NOT merge.
        let a1 = r#"{"type":"assistant","timestamp":"2026-07-21T15:56:07.000Z","uuid":"u1","message":{"id":"msg_A","model":"glm-5.2","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10,"cache_read_input_tokens":1000}}}"#;
        let a2 = r#"{"type":"assistant","timestamp":"2026-07-21T15:56:08.000Z","uuid":"u2","message":{"id":"msg_A","model":"glm-5.2","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10,"cache_read_input_tokens":1000}}}"#;
        let b1 = r#"{"type":"assistant","timestamp":"2026-07-21T16:00:00.000Z","uuid":"u3","message":{"id":"msg_B","model":"glm-5.2","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":20,"cache_read_input_tokens":2000}}}"#;
        write_lines(&file, &[a1, a2, b1]);

        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(
            result.events.len(),
            2,
            "msg_A's two content-block events collapse; msg_B stays separate"
        );
        // Deterministic order by timestamp.
        assert_eq!(result.events[0].tokens.input, 100);
        assert_eq!(result.events[0].tokens.cache_read, 1000);
        assert_eq!(result.events[1].tokens.input, 200);
    }

    #[test]
    fn dedup_picks_final_block_over_message_start_snapshot() {
        // One assistant call (msg_A) written as a `message_start` snapshot
        // (output=1, no stop_reason) followed by the final block (full output +
        // stop_reason). The snapshot must NOT win — otherwise output is frozen
        // at 1 and systematically undercounted.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        let start = r#"{"type":"assistant","timestamp":"2026-07-21T15:56:07.000Z","uuid":"u1","message":{"id":"msg_A","model":"glm-5.2","usage":{"input_tokens":100,"output_tokens":1,"cache_read_input_tokens":1000}}}"#;
        let final_block = r#"{"type":"assistant","timestamp":"2026-07-21T15:56:08.000Z","uuid":"u2","message":{"id":"msg_A","model":"glm-5.2","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":1349,"cache_read_input_tokens":1000}}}"#;
        // Snapshot first, then final.
        write_lines(&file, &[start, final_block]);
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].tokens.output, 1349);
        assert_eq!(result.events[0].stop_reason, "end_turn");

        // Order-independent: final block first, then a late snapshot — final still wins.
        write_lines(&file, &[final_block, start]);
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].tokens.output, 1349);
        assert_eq!(result.events[0].stop_reason, "end_turn");
    }

    #[test]
    fn discover_on_missing_dir_returns_empty_not_error() {
        let base = tempfile::tempdir().unwrap();
        let p = ClaudeCodeProvider::with_dir(base.path().join("does-not-exist"));
        assert!(p.discover().unwrap().is_empty());
    }

    // ---- incremental collect ----

    /// One assistant event line (with message id) for incremental tests.
    fn assistant_line(uuid: &str, mid: &str, out: u32) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-07-21T15:56:07.000Z","uuid":"{uuid}","message":{{"id":"{mid}","model":"glm-5.2","stop_reason":"tool_use","usage":{{"input_tokens":100,"output_tokens":{out}}}}}}}"#
        )
    }

    #[test]
    fn incremental_empty_progress_parses_all_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        write_lines(&file, &[assistant_line("u1", "msg_A", 10)]);
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let (result, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(result.events.len(), 1, "first run is a full parse");
        assert_eq!(delta.len(), 1, "a cursor is recorded for the file");
        let key = file.to_string_lossy().into_owned();
        let cursor = delta.get(&key).unwrap();
        assert!(cursor.last_line_offset >= 1);
        assert!(cursor.last_modified > 0);
    }

    #[test]
    fn incremental_skips_unchanged_file_via_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        write_lines(&file, &[assistant_line("u1", "msg_A", 10)]);
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        // Second collect, file untouched → mtime gate skips it entirely.
        let (r2, delta2) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 0, "unchanged file yields no events");
        assert!(delta2.is_empty(), "unchanged file advances no cursor");
    }

    #[test]
    fn incremental_parses_only_appended_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        write_lines(&file, &[assistant_line("u1", "msg_A", 10)]);
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let (_, progress) = p.collect_incremental(&ScanProgress::new()).unwrap();
        // Append a new event — content change bumps mtime past the gate.
        std::thread::sleep(std::time::Duration::from_millis(20));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file)
                .unwrap();
            writeln!(f, "{}", assistant_line("u2", "msg_B", 20)).unwrap();
        }
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 1, "only the appended event is parsed");
        assert_eq!(r2.events[0].uuid, "u2");
    }

    #[test]
    fn incremental_truncation_resets_offset() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        write_lines(
            &file,
            &[
                assistant_line("u1", "msg_A", 10),
                assistant_line("u2", "msg_B", 20),
                assistant_line("u3", "msg_C", 30),
            ],
        );
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let (_, progress) = p.collect_incremental(&ScanProgress::new()).unwrap();
        // Simulate a truncation: rewrite with fewer lines + a new message id.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_lines(&file, &[assistant_line("u9", "msg_NEW", 999)]);
        let (r2, _) = p.collect_incremental(&progress).unwrap();
        // Truncation detected (total < prev offset) → re-read from 0 → the new
        // message is parsed despite the shrunken file.
        assert_eq!(r2.events.len(), 1);
        assert_eq!(r2.events[0].uuid, "u9");
    }

    #[test]
    fn incremental_partial_last_line_not_advanced_past() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        let complete = assistant_line("u1", "msg_A", 10);
        // One complete line (with newline) then a partial JSON line WITHOUT a
        // trailing newline — as if Claude is mid-write.
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&file).unwrap();
            writeln!(f, "{complete}").unwrap();
            write!(f, r#"{{"type":"assistant","#).unwrap();
        }
        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        let key = file.to_string_lossy().into_owned();
        let cursor = delta.get(&key).unwrap();
        // 2 lines visible (1 complete + 1 partial), but no trailing newline ⇒
        // cursor stops at line 1, leaving the partial line for next collect.
        assert_eq!(cursor.last_line_offset, 1);
        assert_eq!(r1.events.len(), 1, "complete line parsed, partial skipped");
    }
}
