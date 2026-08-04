//! Source-log providers (parse local session logs).
//!
//! Plugin trait + shared incremental driver. Concrete providers live in
//! submodules (`claude`, `codex`, `gemini`, `grok`, `opencode`). A provider
//! discovers Source files and parses them into two raw streams:
//!   - per-call [`RawUsage`] (one per `assistant` event = one API request), and
//!   - per-turn [`RawTurnDuration`] (from `system/turn_duration` events).
//!
//! Both are pre-device / pre-cost — the provider does NOT know about deviceId
//! or pricing. That is applied by the ingest layer, so the same provider output
//! can land in the Local Store (Standalone) and the JSONL Artifact.

use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::model::{ServerToolUse, TokenCounts};

mod claude;
mod codex;
mod gemini;
mod grok;
mod opencode;

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

    /// Full-scan parse of the given files into usage events + turn durations.
    /// Diagnostic/test surface and the semantic reference for a "parse
    /// everything" run; the production collect path is
    /// [`Provider::collect_incremental`] (which, with empty progress, also
    /// yields a full scan). Each provider's `parse` delegates to the same
    /// parsing helpers its `collect_incremental` closure uses, so testing via
    /// `parse` exercises production logic — not a divergent path.
    #[allow(dead_code)] // off the production path by design; kept as the test/diagnostic full-scan surface
    fn parse(&self, files: &[PathBuf]) -> AppResult<CollectResult>;

    /// Incremental collect: parse only what each file's recorded cursor says is
    /// new, returning the advanced cursors to persist. This is the ONLY collect
    /// entry point the production path (`collect_into`) calls. Each provider
    /// implements its own — the JSONL providers delegate to the shared
    /// [`collect_jsonl_incremental`] driver; OpenCode (SQLite, two-level
    /// watermark) keeps its own. There is intentionally no default impl: a
    /// provider that skipped this would have no production collect path.
    fn collect_incremental(
        &self,
        progress: &ScanProgress,
    ) -> AppResult<(CollectResult, ScanProgressDelta)>;
}

/// All enabled Source-log providers, in collection order. A provider whose
/// source dir is absent simply discovers no files (not an error), so every
/// provider is always instantiated; the shared `scan_progress` table keys by
/// file path, which is naturally disjoint across providers.
pub fn all_providers() -> AppResult<Vec<Box<dyn Provider>>> {
    Ok(vec![
        Box::new(claude::ClaudeCodeProvider::new()?),
        Box::new(codex::CodexProvider::new()?),
        Box::new(gemini::GeminiCliProvider::new()?),
        Box::new(opencode::OpenCodeProvider::new()?),
        Box::new(grok::GrokProvider::new()?),
    ])
}

/// One JSONL file's parse result. The provider's per-file parser returns this;
/// the shared incremental driver below handles everything else (mtime gate,
/// truncation self-heal, partial-last-line guard, cursor advance, ordering).
pub(super) struct FileParseOutcome {
    pub(super) events: Vec<RawUsage>,
    pub(super) turn_durations: Vec<RawTurnDuration>,
    pub(super) skipped: u32,
}

/// Shared incremental collect for line-oriented JSONL sources (Claude Code,
/// Codex, Grok) plus Gemini (single JSON object per file, no line cursor — its
/// `parse_file` ignores `start_line` and re-parses the whole text on each gate
/// pass). Walks every discovered file: mtime-gates unchanged ones, re-reads
/// changed ones past their line cursor, and hands the file text + start line to
/// `parse_file` — the only thing that differs across these providers is "how a
/// file's text becomes events". `parse_file` receives the 1-based start line
/// (already self-healed on truncation) and must skip lines at or before it.
/// OpenCode (SQLite, two-level watermark) keeps its own `collect_incremental` —
/// its source shape does not fit this driver.
pub(super) fn collect_jsonl_incremental(
    provider: &dyn Provider,
    progress: &ScanProgress,
    parse_file: impl Fn(&Path, &str, i64) -> FileParseOutcome,
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
        let outcome = parse_file(file, &text, start_line);
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
pub(super) fn normalize_cache_inclusive(input: u32, cache_read: u32) -> (u32, u32) {
    let clamped = cache_read.min(input);
    let fresh = input.saturating_sub(clamped);
    (fresh, clamped)
}

/// File mtime in nanos since UNIX_EPOCH, for the incremental mtime gate. Clamped
/// to `i64::MAX` (the SQLite column is INTEGER). Returns 0 if mtime is
/// unavailable — then the gate never skips (safe, just re-parses).
pub(super) fn metadata_modified_nanos(metadata: &std::fs::Metadata) -> i64 {
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
}
