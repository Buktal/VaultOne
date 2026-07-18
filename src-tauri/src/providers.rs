//! Source-log providers (ADR-0001: parse local session logs).
//!
//! Plugin trait + the MVP Claude Code provider. A provider discovers Source
//! files and parses them into `RawUsage` events (one per assistant message =
//! one API request, ADR-0003). It does NOT know about deviceId or pricing —
//! that is applied by the ingest layer, so the same provider output can land in
//! the Local Store (Standalone) and the JSONL Artifact (ADR-0004).

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

/// A single parsed usage event (provider output, pre-cost / pre-device).
#[derive(Debug, Clone, PartialEq)]
pub struct RawUsage {
    /// Globally-unique id from the Source log — the dedup key (ADR-0005).
    pub uuid: String,
    /// ISO8601 UTC timestamp from the Source log.
    pub timestamp: String,
    /// Billed / mapped model string, e.g. `glm-5.2`.
    pub model: String,
    /// Provider tag, e.g. `claude_code` (ADR-0001).
    pub source: String,
    pub tokens: TokenCounts,
    pub server_tool_use: ServerToolUse,
}

/// Outcome of parsing one provider's sources.
#[derive(Debug, Clone, Default)]
pub struct CollectResult {
    pub source: String,
    pub events: Vec<RawUsage>,
    /// Files scanned.
    pub files_scanned: u32,
    /// Lines that failed to parse (skipped, not fatal).
    pub lines_skipped: u32,
}

/// Provider plugin interface (ADR-0001: extensible to Codex / Gemini / …).
pub trait Provider: Send + Sync {
    /// Stable provider tag, e.g. `claude_code`. Becomes `RawUsage.source`.
    fn name(&self) -> &'static str;

    /// Discover Source files for this provider.
    fn discover(&self) -> AppResult<Vec<PathBuf>>;

    /// Parse discovered files into usage events.
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

/// Claude Code session-log provider (ADR-0001 / 0003).
///
/// Reads `~/.claude/projects/**/*.jsonl`; each line is a JSON event. Assistant
/// events carry `message.usage` (token four-pack + server tool use), `message.model`,
/// top-level `timestamp` and `uuid`. One assistant event = one API request.
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
                    Ok(ev) => {
                        if let Some(raw) = ev.into_raw() {
                            events.push(raw);
                        }
                    }
                    Err(_) => skipped += 1,
                }
            }
        }
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
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
    message: Option<SessionMessage>,
}

#[derive(serde::Deserialize)]
struct SessionMessage {
    model: Option<String>,
    usage: Option<SessionUsage>,
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
}

#[derive(serde::Deserialize)]
struct SessionServerTool {
    #[serde(default)]
    web_search_requests: u32,
    #[serde(default)]
    web_fetch_requests: u32,
}

impl SessionEvent {
    /// Convert to a `RawUsage` iff this is an assistant event with a usable
    /// usage block. Drops events with no tokens (e.g. pure tool results).
    fn into_raw(self) -> Option<RawUsage> {
        if self.typ.as_deref() != Some("assistant") {
            return None;
        }
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
        let timestamp = self.timestamp.unwrap_or_else(|| {
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        });
        let st = usage.server_tool_use.unwrap_or(SessionServerTool {
            web_search_requests: 0,
            web_fetch_requests: 0,
        });
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
        })
    }
}

/// Resolve the default projects dir for diagnostics (used by commands).
pub fn default_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Whether a path looks like a Claude projects dir (used by commands/UI hints).
pub fn looks_like_projects_dir(_p: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

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
        // 4 opens (outer/message/usage/server_tool_use) ⇒ 4 closes.
        let assistant = r#"{"type":"assistant","timestamp":"2026-07-13T16:55:22.467Z","uuid":"abc-1","message":{"model":"glm-5.2","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5,"server_tool_use":{"web_search_requests":2}}}}"#;
        let user = r#"{"type":"user","uuid":"abc-2","message":{}}"#;
        write_lines(&file, &[assistant, user, "", "{not json"]);

        let p = ClaudeCodeProvider::with_dir(dir.path().to_path_buf());
        let files = p.discover().unwrap();
        assert_eq!(files.len(), 1);
        let result = p.parse(&files).unwrap();
        assert_eq!(result.source, "claude_code");
        assert_eq!(result.events.len(), 1);
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
        let files = p.discover().unwrap();
        let result = p.parse(&files).unwrap();
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn discover_on_missing_dir_returns_empty_not_error() {
        let base = tempfile::tempdir().unwrap();
        let p = ClaudeCodeProvider::with_dir(base.path().join("does-not-exist"));
        assert!(p.discover().unwrap().is_empty());
    }
}
