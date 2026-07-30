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

use std::path::{Path, PathBuf};

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

/// Per-file incremental scan cursor. Persisted in `scan_progress`;
/// replaceable — a lost cursor triggers a full rescan (the ledger dedups).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileCursor {
    /// File mtime (nanos) as last seen by this cursor.
    pub last_modified: i64,
    /// Last fully-processed 1-based line number. 0 = nothing parsed yet.
    pub last_line_offset: i64,
}

/// file_path → cursor. Loaded before `collect_incremental`, saved after. A plain
/// `HashMap` alias (not a newtype) — it is a trivial wrapper.
pub type ScanProgress = std::collections::HashMap<String, FileCursor>;

/// One collect's worth of cursor advances: only entries for files actually
/// opened and read. Saved as an UPSERT. Same shape as `ScanProgress` (a subset).
pub type ScanProgressDelta = std::collections::HashMap<String, FileCursor>;

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

    /// Incremental collect: parse only lines past each file's
    /// recorded cursor, returning the advanced cursors to persist. The default
    /// impl **degrades to a full parse and returns an empty delta** (the cursor
    /// never advances), so a provider that does not override this stays correct
    /// and full-scan. Override for append-only JSONL sources (ClaudeCodeProvider).
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        let _ = progress;
        let result = self.collect()?;
        // Empty delta ⇒ nothing saved; next collect is still full. Correct for a
        // provider with no incremental logic.
        Ok((result, ScanProgressDelta::new()))
    }
}

/// All enabled Source-log providers, in collection order. A provider whose
/// source dir is absent simply discovers no files (not an error), so every
/// provider is always instantiated; the shared `scan_progress` table keys by
/// file path, which is naturally disjoint across providers.
pub fn all_providers() -> AppResult<Vec<Box<dyn Provider>>> {
    Ok(vec![
        Box::new(ClaudeCodeProvider::new()?),
        Box::new(CodexProvider::new()?),
        Box::new(GeminiCliProvider::new()?),
        Box::new(OpenCodeProvider::new()?),
    ])
}

/// One JSONL file's parse result. The provider's per-file parser returns this;
/// the shared incremental driver below handles everything else (mtime gate,
/// truncation self-heal, partial-last-line guard, cursor advance, ordering).
struct FileParseOutcome {
    events: Vec<RawUsage>,
    turn_durations: Vec<RawTurnDuration>,
    skipped: u32,
}

/// Shared incremental collect for append-only JSONL sources (Claude Code,
/// Codex). Walks every discovered file: mtime-gates unchanged ones, re-reads
/// changed ones past their line cursor, and hands the file text + start line to
/// `parse_file` — the only thing that differs across JSONL providers is "how a
/// file's lines become events". `parse_file` receives the 1-based start line
/// (already self-healed on truncation) and must skip lines at or before it.
/// Gemini (single JSON object, no line cursor) and OpenCode (SQLite, two-level
/// watermark) keep their own `collect_incremental` — their source shapes do not
/// fit this driver.
fn collect_jsonl_incremental(
    provider: &dyn Provider,
    progress: &ScanProgress,
    parse_file: impl Fn(&str, i64) -> FileParseOutcome,
) -> AppResult<(CollectResult, ScanProgressDelta)> {
    let files = provider.discover()?;
    let mut events: Vec<RawUsage> = Vec::new();
    let mut turn_durations: Vec<RawTurnDuration> = Vec::new();
    let mut skipped = 0u32;
    let mut delta = ScanProgressDelta::new();

    for file in &files {
        let path_str = file.to_string_lossy().into_owned();
        // mtime gate — one stat; unchanged files do no IO/serde.
        let metadata = match std::fs::metadata(file) {
            Ok(m) => m,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let mtime = metadata_modified_nanos(&metadata);
        let prev = progress.get(&path_str).copied().unwrap_or_default();
        // `prev.last_modified != 0` lets a never-seen file parse in full.
        if prev.last_modified != 0 && mtime <= prev.last_modified {
            continue;
        }
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let total_lines = text.lines().count() as i64;
        // Truncation self-heal: if the file shrank below the last known offset,
        // re-read from the start (would otherwise silently drop post-truncation
        // appends).
        let start_line = if total_lines < prev.last_line_offset {
            0
        } else {
            prev.last_line_offset
        };
        let outcome = parse_file(&text, start_line);
        events.extend(outcome.events);
        turn_durations.extend(outcome.turn_durations);
        skipped += outcome.skipped;
        // Partial-last-line guard: no trailing newline ⇒ the last line may be
        // mid-write; don't advance past it or the next collect skips it.
        let ends_clean = text.ends_with('\n') || text.ends_with('\r');
        let new_offset = if ends_clean {
            total_lines
        } else if total_lines > start_line {
            total_lines - 1
        } else {
            start_line
        };
        delta.insert(
            path_str,
            FileCursor {
                last_modified: mtime,
                last_line_offset: new_offset,
            },
        );
    }

    // Deterministic order (timestamp, then uuid).
    events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
    // files_scanned stays "discovered count" — do not redefine to "parsed count".
    let result = CollectResult {
        source: provider.name().to_string(),
        events,
        turn_durations,
        files_scanned: files.len() as u32,
        lines_skipped: skipped,
    };
    Ok((result, delta))
}

/// Normalize a cache-inclusive `input` — one whose value already contains its
/// `cache_read` portion — into VaultOne's fresh-input representation: subtract
/// `cache_read` (floored at 0) and clamp `cache_read` so it can never exceed
/// `input`. Returns `(fresh_input, clamped_cache_read)`.
///
/// Cache-inclusive sources (Codex, Gemini, Grok) call this at parse time so the
/// `RawUsage.input` they emit is always fresh — the one hard contract every
/// provider must satisfy. Fresh sources (Claude, OpenCode) carry fresh input
/// natively and skip this step.
fn normalize_cache_inclusive(input: u32, cache_read: u32) -> (u32, u32) {
    let clamped = cache_read.min(input);
    let fresh = input.saturating_sub(clamped);
    (fresh, clamped)
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
        let mut events_by_mid: std::collections::HashMap<String, RawUsage> =
            std::collections::HashMap::new();
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
                if Self::fold_line(line, &mut events_by_mid, &mut turn_durations) {
                    skipped += 1;
                }
            }
        }
        // Deterministic order (timestamp, then uuid) so repeated parses of the
        // same sources yield identical artifact lines.
        let mut events: Vec<RawUsage> = events_by_mid.into_values().collect();
        events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        Ok(CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations,
            files_scanned: files.len() as u32,
            lines_skipped: skipped,
        })
    }

    /// Incremental collect: parse only lines past each file's
    /// recorded cursor and return the advanced cursors to persist. The mtime
    /// gate skips unchanged files (no IO/serde); a never-seen file ({0,0})
    /// falls through to a full parse on first sight.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        collect_jsonl_incremental(self, progress, |text, start_line| {
            // Same message-id dedup as `parse` — one assistant response may span
            // several content-block events that all repeat the full usage. The
            // stored uuid stays the event uuid (not the message id); re-keying
            // would cause a mass migration duplicate on first run.
            let mut events_by_mid: std::collections::HashMap<String, RawUsage> =
                std::collections::HashMap::new();
            let mut turn_durations = Vec::new();
            let mut skipped = 0u32;
            for (idx, line) in text.lines().enumerate() {
                let line_no = idx as i64 + 1; // 1-based
                if line_no <= start_line {
                    continue;
                }
                if Self::fold_line(line, &mut events_by_mid, &mut turn_durations) {
                    skipped += 1;
                }
            }
            FileParseOutcome {
                events: events_by_mid.into_values().collect(),
                turn_durations,
                skipped,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Codex provider
// ---------------------------------------------------------------------------

/// Codex (`~/.codex`) session-log provider.
///
/// Reads `<codex_dir>/sessions/**/*.jsonl` (depth ≤ 3, i.e. `YYYY/MM/DD`) and
/// `<codex_dir>/archived_sessions/*.jsonl` (flat). Only `session_meta`,
/// `turn_context`, and `event_msg` (subtype `token_count`) events are consumed.
///
/// Codex's `total_token_usage` is **cumulative** and its `input_tokens` is
/// cache-inclusive, so the provider computes per-call deltas and subtracts
/// `cache_read` to yield a fresh `input` — Codex is the one cache-inclusive
/// source (parse-time fresh-input normalization). Sub-agent / fork logs replay
/// the parent thread's history before their own usage; that replay only
/// re-establishes the cumulative baseline and is never emitted.
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
        collect_jsonl_incremental(self, progress, |text, start_line| {
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
                        state.current_model = normalize_codex_model(model);
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
                    state.current_model = normalize_codex_model(model);
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

/// Normalize a Codex model name: lowercase → strip `provider/` prefix → strip
/// `-YYYY-MM-DD` / `-YYYYMMDD` date suffix. Required for pricing-table hits.
fn normalize_codex_model(raw: &str) -> String {
    let mut name = raw.to_lowercase();
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }
    // Strip ISO date suffix -YYYY-MM-DD (exactly 11 chars).
    if name.len() > 11 && name.is_char_boundary(name.len() - 11) {
        let suffix = &name[name.len() - 11..];
        if suffix.is_ascii()
            && suffix.as_bytes()[0] == b'-'
            && suffix[1..5].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[5] == b'-'
            && suffix[6..8].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[8] == b'-'
            && suffix[9..11].chars().all(|c| c.is_ascii_digit())
        {
            name.truncate(name.len() - 11);
        }
    }
    // Strip compact date suffix -YYYYMMDD (exactly 8 chars after last '-').
    if name.len() > 9 {
        let parts: Vec<&str> = name.rsplitn(2, '-').collect();
        if parts.len() == 2 {
            if let Some(suffix) = parts.first() {
                if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
                    name = parts[1].to_string();
                }
            }
        }
    }
    name
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

// ---------------------------------------------------------------------------
// Gemini CLI provider
// ---------------------------------------------------------------------------

/// Gemini CLI (`~/.gemini`) session-log provider.
///
/// Reads `<gemini_dir>/tmp/<project_hash>/chats/session-*.json`. Each file is a
/// single JSON object with a `messages` array; only `type:"gemini"` messages
/// carrying a `tokens` object are consumed. The CLI pre-normalizes tokens, so
/// `input` is already fresh, `cached` is cache_read, and `thoughts` is folded
/// into `output` (thinking tokens are billed as output). `cache_creation` is
/// always 0 — Gemini uses implicit caching and does not expose a write bucket.
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
    /// there is no line cursor — mtime-gate unchanged files and full re-parse
    /// the rest. The ledger dedups already-seen message ids; a CLI rewrite that
    /// changes an existing message's tokens is NOT re-costed (freeze + top-up
    /// only), which matches the session-log contract.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)> {
        let files = self.discover()?;
        let mut events = Vec::new();
        let mut skipped = 0u32;
        let mut delta = ScanProgressDelta::new();
        for file in &files {
            let path_str = file.to_string_lossy().into_owned();
            let metadata = match std::fs::metadata(file) {
                Ok(m) => m,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let mtime = metadata_modified_nanos(&metadata);
            let prev = progress.get(&path_str).copied().unwrap_or_default();
            if prev.last_modified != 0 && mtime <= prev.last_modified {
                continue;
            }
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            events.extend(parse_gemini_text(&text));
            // No line cursor for a single-JSON file; offset stays 0.
            delta.insert(
                path_str,
                FileCursor {
                    last_modified: mtime,
                    last_line_offset: 0,
                },
            );
        }
        events.sort_by(|a, b| (&a.timestamp, &a.uuid).cmp(&(&b.timestamp, &b.uuid)));
        let result = CollectResult {
            source: self.name().to_string(),
            events,
            turn_durations: Vec::new(),
            files_scanned: files.len() as u32,
            lines_skipped: skipped,
        };
        Ok((result, delta))
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
                input: tokens.input,
                output: tokens.output + tokens.thoughts,
                cache_creation: 0,
                cache_read: tokens.cached,
            },
            server_tool_use: ServerToolUse::default(),
            stop_reason: String::new(),
            service_tier: String::new(),
            iterations: 0,
        });
    }
    events
}

// ---------------------------------------------------------------------------
// OpenCode provider (SQLite)
// ---------------------------------------------------------------------------

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

/// File mtime in nanos since UNIX_EPOCH, for the incremental mtime gate. Clamped
/// to `i64::MAX` (the SQLite column is INTEGER). Returns 0 if mtime is
/// unavailable — then the gate never skips (safe, just re-parses).
fn metadata_modified_nanos(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
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

    fn write_lines(path: &Path, lines: &[impl AsRef<str>]) {
        let mut f = fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{}", l.as_ref()).unwrap();
        }
    }

    // ===================== shared normalizer =====================

    #[test]
    fn normalize_cache_inclusive_subtracts_and_clamps() {
        // Typical inclusive source: input already contains cache_read.
        let (fresh, cached) = normalize_cache_inclusive(8522, 3138);
        assert_eq!(fresh, 5384);
        assert_eq!(cached, 3138);
        // cache_read within input ⇒ both unchanged.
        let (fresh, cached) = normalize_cache_inclusive(100, 30);
        assert_eq!(fresh, 70);
        assert_eq!(cached, 30);
        // Abnormal: cache_read exceeds input (delta arithmetic) ⇒ clamped down,
        // fresh input floored at 0.
        let (fresh, cached) = normalize_cache_inclusive(10, 80);
        assert_eq!(fresh, 0);
        assert_eq!(cached, 10);
        // Both zero ⇒ both zero.
        let (fresh, cached) = normalize_cache_inclusive(0, 0);
        assert_eq!((fresh, cached), (0, 0));
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

    #[test]
    fn incremental_default_impl_returns_empty_delta() {
        // A provider that does NOT override collect_incremental must still work:
        // full parse, empty delta (cursor never advances).
        struct StubProvider;
        impl Provider for StubProvider {
            fn name(&self) -> &'static str {
                "stub"
            }
            fn discover(&self) -> AppResult<Vec<PathBuf>> {
                Ok(Vec::new())
            }
            fn parse(&self, _files: &[PathBuf]) -> AppResult<CollectResult> {
                Ok(CollectResult::default())
            }
        }
        let p = StubProvider;
        let (result, delta) = p.collect_incremental(&ScanProgress::new()).unwrap();
        assert!(delta.is_empty(), "default impl advances no cursor");
        assert!(
            result.events.is_empty(),
            "default impl still yields a full-parse result"
        );
    }

    // ===================== Codex provider =====================

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

    #[test]
    fn codex_normalize_model_lowercase_prefix_and_dates() {
        assert_eq!(normalize_codex_model("GLM-4.6"), "glm-4.6");
        assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("OPENAI/GPT-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("gpt-5.4-pro-2026-03-05"),
            "gpt-5.4-pro"
        );
        assert_eq!(normalize_codex_model("gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("claude-opus-4-6-20260206"),
            "claude-opus-4-6"
        );
        assert_eq!(
            normalize_codex_model("openai/GPT-5.4-2026-03-05"),
            "gpt-5.4"
        );
        assert_eq!(normalize_codex_model("openai/gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(normalize_codex_model("o3"), "o3");
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

    // ===================== Gemini CLI provider =====================

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

    /// Four-bucket mapping vs CC-Switch (field-for-field equal): input is fresh
    /// as-is, output folds in thoughts, cache_read = cached, cache_creation = 0.
    /// Cache-only messages are kept; all-zero and non-gemini messages dropped.
    #[test]
    fn gemini_parses_session_into_fresh_four_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {"type":"gemini","id":"m1","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:34:56.789Z","tokens":{"input":8522,"output":29,"cached":3138,"thoughts":405,"tool":0,"total":8956}},
                {"type":"gemini","id":"m2","model":"gemini-2.5-pro","timestamp":"2026-07-15T12:35:00.000Z","tokens":{"input":0,"output":0,"cached":5000,"thoughts":0}},
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
        assert_eq!(m1.tokens.input, 8522);
        assert_eq!(m1.tokens.output, 434, "output folds in thoughts (29 + 405)");
        assert_eq!(m1.tokens.cache_read, 3138);
        assert_eq!(m1.tokens.cache_creation, 0);
        assert_eq!(m1.model, "gemini-2.5-pro");
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

    // ===================== OpenCode provider (SQLite) =====================

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
