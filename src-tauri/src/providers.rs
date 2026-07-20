//! Source-log providers (parse local session logs).
//!
//! Plugin trait + the Claude Code provider. A provider discovers Source files
//! and parses them into two raw streams:
//!   - per-call [`RawUsage`] (one per `assistant` event = one API request), and
//!   - per-turn [`RawTurnDuration`] (from `system/turn_duration` events).
//!
//! Both are pre-device / pre-cost — the provider does NOT know about deviceId
//! or pricing. That is applied by the ingest layer, so the same provider output
//! can land in the Local Store (Standalone) and the JSONL Artifact.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

/// A single parsed per-call usage event (provider output, pre-cost / pre-device).
#[derive(Debug, Clone, PartialEq)]
pub struct RawUsage {
    /// Globally-unique id from the Source log — the dedup key.
    pub uuid: String,
    /// ISO8601 UTC timestamp from the Source log.
    pub timestamp: String,
    /// Billed / mapped model string, e.g. `glm-5.2`.
    pub model: String,
    /// Provider tag, e.g. `claude_code`.
    pub source: String,
    pub tokens: TokenCounts,
    pub server_tool_use: ServerToolUse,
    /// Semantic termination reason (`tool_use` / `end_turn` / …). NOT an HTTP status.
    pub stop_reason: String,
    /// Service tier label, e.g. `standard`.
    pub service_tier: String,
    /// Reasoning/thinking iteration count (source array length).
    pub iterations: u32,
}

/// A single parsed per-turn duration (provider output, pre-device). Sourced from
/// the `system/turn_duration` event's `durationMs`.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTurnDuration {
    /// Dedup key (the source event's uuid).
    pub uuid: String,
    pub timestamp: String,
    /// Turn wall-clock in milliseconds.
    pub duration_ms: u32,
}

/// Outcome of parsing one provider's sources.
#[derive(Debug, Clone, Default)]
pub struct CollectResult {
    pub source: String,
    pub events: Vec<RawUsage>,
    /// Per-turn durations (from `system/turn_duration` events).
    pub turn_durations: Vec<RawTurnDuration>,
    /// Files scanned.
    pub files_scanned: u32,
    /// Lines that failed to parse (skipped, not fatal).
    pub lines_skipped: u32,
}

/// Provider plugin interface (extensible to Codex / Gemini / …).
pub trait Provider: Send + Sync {
    /// Stable provider tag, e.g. `claude_code`. Becomes `RawUsage.source`.
    fn name(&self) -> &'static str;

    /// Discover Source files for this provider.
    fn discover(&self) -> AppResult<Vec<PathBuf>>;

    /// Parse discovered files into usage events + turn durations.
    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult>;

    /// Convenience: discover + parse.
    fn collect(&self) -> AppResult<CollectResult> {
        let files = self.discover()?;
        self.parse(&files)
    }
}

// ---------------------------------------------------------------------------
// Claude Code provider
// ---------------------------------------------------------------------------

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
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<SessionEvent>(line) {
                    Ok(ev) => match ev.classify() {
                        Parsed::Usage(u) => events.push(u),
                        Parsed::TurnDuration(td) => turn_durations.push(td),
                        Parsed::Skip => {}
                    },
                    Err(_) => skipped += 1,
                }
            }
        }
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations,
            files_scanned: files.len() as u32,
            lines_skipped: skipped,
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
                    timestamp: self.timestamp.unwrap_or_else(now_iso),
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
        let timestamp = self.timestamp.unwrap_or_else(now_iso);
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

/// ISO8601 UTC "now", used as a last-resort timestamp when the source omits one.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Resolve the default projects dir for diagnostics (used by commands).
pub fn default_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    fn write_lines(path: &Path, lines: &[&str]) {
        let mut f = fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
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
    fn discover_on_missing_dir_returns_empty_not_error() {
        let base = tempfile::tempdir().unwrap();
        let p = ClaudeCodeProvider::with_dir(base.path().join("does-not-exist"));
        assert!(p.discover().unwrap().is_empty());
    }
}
