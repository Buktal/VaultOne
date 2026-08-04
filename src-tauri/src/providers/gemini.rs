//! Gemini CLI (`~/.gemini`) session-log provider.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::model::{ServerToolUse, TokenCounts};

use super::{
    collect_jsonl_incremental, normalize_cache_inclusive, CollectResult, FileParseOutcome,
    Provider, RawUsage, ScanProgress, ScanProgressDelta,
};

/// Gemini CLI (`~/.gemini`) session-log provider.
///
/// Reads `<gemini_dir>/tmp/<project_hash>/chats/session-*.json`. Each file is a
/// single JSON object with a `messages` array; only `type:"gemini"` messages
/// carrying a `tokens` object are consumed. The CLI's `input` is
/// cache-inclusive (it contains `cached`), so it is normalized to fresh at
/// parse; `cached` is cache_read, and `thoughts` is folded into `output`
/// (thinking tokens are billed as output). `cache_creation` is always 0 —
/// Gemini uses implicit caching and does not expose a write bucket.
pub struct GeminiCliProvider {
    gemini_dir: PathBuf,
}

impl GeminiCliProvider {
    /// Default provider rooted at `~/.gemini`.
    pub fn new() -> AppResult<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| AppError::Provider("cannot resolve home dir".into()))?;
        Ok(Self {
            gemini_dir: home.join(".gemini"),
        })
    }

    /// Test/override constructor with an explicit gemini dir.
    #[cfg(test)]
    pub(crate) fn with_dir(dir: PathBuf) -> Self {
        Self { gemini_dir: dir }
    }

    fn discover_in(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let tmp = self.gemini_dir.join("tmp");
        if !tmp.is_dir() {
            return files;
        }
        let Ok(project_dirs) = std::fs::read_dir(&tmp) else {
            return files;
        };
        for entry in project_dirs.flatten() {
            let chats = entry.path().join("chats");
            if !chats.is_dir() {
                continue;
            }
            let Ok(chat_files) = std::fs::read_dir(&chats) else {
                continue;
            };
            for fe in chat_files.flatten() {
                let path = fe.path();
                let is_session = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("session-") && n.ends_with(".json"))
                    .unwrap_or(false);
                if is_session {
                    files.push(path);
                }
            }
        }
        files
    }
}

impl Provider for GeminiCliProvider {
    fn name(&self) -> &'static str {
        "gemini_cli"
    }

    fn discover(&self) -> AppResult<Vec<PathBuf>> {
        if !self.gemini_dir.exists() {
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
            events.extend(parse_gemini_text(&text));
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

    /// Incremental collect: a Gemini session file is a single JSON object, so
    /// there is no line cursor — only the mtime gate (owned by the shared JSONL
    /// driver) is meaningful, and a gated file is re-parsed in full. The line
    /// cursor the driver advances is harmless: this provider's `parse_file`
    /// ignores `start_line` and parses the whole text every gate pass. The
    /// ledger dedups already-seen message ids; a CLI rewrite that changes an
    /// existing message's tokens is NOT re-costed (freeze + top-up only), which
    /// matches the session-log contract.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        collect_jsonl_incremental(self, progress, |_file, text, _start_line| {
            // Single JSON object per file ⇒ no line cursor; `start_line` is
            // irrelevant and the whole text is parsed on every gate pass.
            FileParseOutcome {
                events: parse_gemini_text(text),
                turn_durations: Vec::new(),
                skipped: 0,
            }
        })
    }
}

/// Parsed token fields from a Gemini `tokens` object (pre-thoughts-merge).
struct GeminiTokens {
    input: u32,
    output: u32,
    cached: u32,
    thoughts: u32,
}

impl GeminiTokens {
    fn is_all_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.thoughts == 0 && self.cached == 0
    }
}

fn parse_gemini_tokens(tokens: &serde_json::Value) -> GeminiTokens {
    let n = |k: &str| tokens.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    GeminiTokens {
        input: n("input"),
        output: n("output"),
        cached: n("cached"),
        thoughts: n("thoughts"),
    }
}

/// Parse one Gemini session file's JSON text into raw events.
fn parse_gemini_text(text: &str) -> Vec<RawUsage> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let Some(messages) = value.get("messages").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }
        let Some(tokens_obj) = msg.get("tokens") else {
            continue;
        };
        if !tokens_obj.is_object() {
            continue;
        }
        let tokens = parse_gemini_tokens(tokens_obj);
        if tokens.is_all_zero() {
            continue;
        }
        // Gemini's `input` is cache-inclusive (it already contains `cached`);
        // normalize to fresh so RawUsage.input matches the fresh-input contract.
        let (fresh_input, clamped_cache_read) =
            normalize_cache_inclusive(tokens.input, tokens.cached);
        let output = tokens.output + tokens.thoughts;
        // A row that normalizes to nothing (e.g. a malformed cache-only row
        // whose `cached` exceeds its inclusive `input`, clamped down to 0)
        // carries no billable tokens — skip it rather than emit a zero event.
        if fresh_input == 0 && output == 0 && clamped_cache_read == 0 {
            continue;
        }
        let message_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let timestamp = msg
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        events.push(RawUsage {
            uuid: format!("gemini:{session_id}:{message_id}"),
            timestamp: timestamp.unwrap_or_else(crate::time::now_iso),
            model: model.to_string(),
            source: "gemini_cli".to_string(),
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
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn write_gemini_session(dir: &Path, hash: &str, filename: &str, json: &str) -> PathBuf {
        let path = dir.join("tmp").join(hash).join("chats").join(filename);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn gemini_parse_tokens_variants() {
        let full: serde_json::Value = serde_json::json!({
            "input": 8522, "output": 29, "cached": 3138, "thoughts": 405, "tool": 0, "total": 8956
        });
        let t = parse_gemini_tokens(&full);
        assert_eq!(t.input, 8522);
        assert_eq!(t.output, 29);
        assert_eq!(t.cached, 3138);
        assert_eq!(t.thoughts, 405);
        // missing fields ⇒ 0.
        let partial: serde_json::Value = serde_json::json!({ "input": 100, "output": 50 });
        let t = parse_gemini_tokens(&partial);
        assert_eq!(t.cached, 0);
        assert_eq!(t.thoughts, 0);
        // all-zero ⇒ skipped by the parse loop.
        let zero: serde_json::Value =
            serde_json::json!({ "input": 0, "output": 0, "cached": 0, "thoughts": 0 });
        assert!(parse_gemini_tokens(&zero).is_all_zero());
        // cache-only ⇒ NOT all-zero ⇒ kept.
        let cache_only: serde_json::Value =
            serde_json::json!({ "input": 0, "output": 0, "cached": 5000, "thoughts": 0 });
        assert!(!parse_gemini_tokens(&cache_only).is_all_zero());
    }

    #[test]
    fn gemini_discover_missing_dir_returns_empty() {
        let base = tempfile::tempdir().unwrap();
        let p = GeminiCliProvider::with_dir(base.path().join("nope"));
        assert!(p.discover().unwrap().is_empty());
    }

    /// Four-bucket mapping: Gemini's `input` is cache-inclusive in the source —
    /// normalized to fresh (input − cache_read, clamped) at parse — output folds
    /// in thoughts, cache_creation = 0. The fixture's `total` (8522 + 29 + 405
    /// = 8956) must round-trip to parsed.total() once input is de-cached.
    /// Cache-only / all-zero / non-gemini messages are dropped.
    #[test]
    fn gemini_parses_session_into_fresh_four_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {"type":"gemini","id":"m1","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:34:56.789Z","tokens":{"input":8522,"output":29,"cached":3138,"thoughts":405,"tool":0,"total":8956}},
                {"type":"gemini","id":"m2","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:35:00.000Z","tokens":{"input":5000,"output":0,"cached":5000,"thoughts":0}},
                {"type":"gemini","id":"m3","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:35:01.000Z","tokens":{"input":0,"output":0,"cached":0,"thoughts":0}},
                {"type":"user","id":"u1","message":"hi"}
            ]
        }"#;
        write_gemini_session(dir.path(), "hashA", "session-1.json", json);
        let p = GeminiCliProvider::with_dir(dir.path().to_path_buf());
        let result = p.parse(&p.discover().unwrap()).unwrap();
        assert_eq!(result.source, "gemini_cli");
        assert_eq!(
            result.events.len(),
            2,
            "m3 all-zero + user dropped; m1/m2 kept"
        );
        let by_id: std::collections::HashMap<&str, &RawUsage> =
            result.events.iter().map(|e| (e.uuid.as_str(), e)).collect();
        let m1 = by_id["gemini:s1:m1"];
        // Fresh input = inclusive input (8522) − cache_read (3138).
        assert_eq!(m1.tokens.input, 5384, "input de-cached: 8522 - 3138");
        assert_eq!(m1.tokens.output, 434, "output folds in thoughts (29 + 405)");
        assert_eq!(m1.tokens.cache_read, 3138);
        assert_eq!(m1.tokens.cache_creation, 0);
        assert_eq!(m1.model, "gemini-2.5-pro");
        // Invariant: de-caching preserves the fixture total (fresh input +
        // output + cache_read == inclusive input + output + thoughts).
        assert_eq!(
            m1.tokens.total(),
            8956,
            "fresh input + output + cache_read == fixture total"
        );
        let m2 = by_id["gemini:s1:m2"];
        assert_eq!(m2.tokens.cache_read, 5000, "cache-only message kept");
        assert_eq!(m2.tokens.input, 0);
        assert_eq!(m2.tokens.output, 0);
    }

    #[test]
    fn gemini_incremental_mtime_gates_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"sessionId":"s1","messages":[{"type":"gemini","id":"m1","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:34:56.789Z","tokens":{"input":10,"output":1,"cached":2,"thoughts":0}}]}"#;
        let path = write_gemini_session(dir.path(), "h", "session-1.json", json);
        let p = GeminiCliProvider::with_dir(dir.path().to_path_buf());
        let (r1, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert_eq!(r1.events.len(), 1);
        let progress: ScanProgress = delta;
        // Unchanged file ⇒ mtime gate skips it entirely.
        let (r2, delta2) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r2.events.len(), 0);
        assert!(delta2.is_empty());
        // Rewrite (new mtime) ⇒ full re-parse; the seen id is re-emitted (the
        // ledger dedups at ingest, not here).
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, json).unwrap();
        let (r3, _) = p.collect_incremental(&progress).unwrap();
        assert_eq!(r3.events.len(), 1);
    }
}
