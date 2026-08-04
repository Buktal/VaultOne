//! Core domain model for the rebuilt VaultOne.
//!
//! Two grains (re-derivation 2026-07-21):
//!   - [`UsageRecord`]: one model API call (per-call). The unit a provider
//!     emits, the Local Store stores, and one JSONL line serializes.
//!   - [`TurnDuration`]: one turn's wall-clock (per-turn), sourced from the
//!     `system/turn_duration` event. Separate from per-call records because a
//!     turn spans multiple API calls.
//!
//! Boundary type rules: no pointer-sized ints cross the Rust→JS boundary.
//! Token counts are `u32`; timestamps cross as ISO8601 strings; cost crosses as
//! `f64` (display-only on the JS side — JS never recomputes cost), while cost
//! is kept internally as `rust_decimal::Decimal` and stored as TEXT in SQLite.

use std::str::FromStr;

use rust_decimal::Decimal;

// ---- Token / tool sub-structures (shared by internal record + DTOs) ----

/// Token four-pack (per-call). `u32` across the boundary.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct TokenCounts {
    pub input: u32,
    pub output: u32,
    pub cache_creation: u32,
    pub cache_read: u32,
}

impl TokenCounts {
    /// Sum of all four buckets — "真实消耗 Tokens" in the dashboard.
    pub fn total(self) -> u32 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
    }

    /// Cache-hit rate as a ratio in [0,1] for display (0 when nothing cacheable).
    /// Denominator = fresh input + cache creation + cache reads — the full
    /// "could have been cached" pool. Matches CC-Switch's cache_hit_rate.
    pub fn cache_hit_rate(self) -> f64 {
        let denom = self.input as f64 + self.cache_creation as f64 + self.cache_read as f64;
        if denom <= 0.0 {
            0.0
        } else {
            self.cache_read as f64 / denom
        }
    }
}

/// Server-side tool usage reported by Claude Code's usage block.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct ServerToolUse {
    pub web_search: u32,
    pub web_fetch: u32,
}

// ---- Decimal <-> string serde (JSONL stores cost as precision-safe TEXT) ----

/// Serialize `Decimal` as a string (precision-safe for JSONL / SQLite TEXT).
pub fn ser_decimal<S: serde::Serializer>(d: &Decimal, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.to_string())
}

/// Deserialize `Decimal` from a string (JSONL reader).
pub fn de_decimal<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    Decimal::from_str(&s).map_err(serde::de::Error::custom)
}

/// Cost split by token bucket, in USD. Computed at ingest, then frozen.
///
/// Internal-only (Decimal precision); DTOs below expose `f64` to the frontend.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CostBreakdown {
    #[serde(serialize_with = "ser_decimal", deserialize_with = "de_decimal")]
    pub input_usd: Decimal,
    #[serde(serialize_with = "ser_decimal", deserialize_with = "de_decimal")]
    pub output_usd: Decimal,
    #[serde(serialize_with = "ser_decimal", deserialize_with = "de_decimal")]
    pub cache_read_usd: Decimal,
    #[serde(serialize_with = "ser_decimal", deserialize_with = "de_decimal")]
    pub cache_creation_usd: Decimal,
    #[serde(serialize_with = "ser_decimal", deserialize_with = "de_decimal")]
    pub total_usd: Decimal,
}

impl CostBreakdown {
    /// Build a breakdown from the four bucket costs; `total` = their sum.
    pub fn from_buckets(
        input: Decimal,
        output: Decimal,
        cache_read: Decimal,
        cache_creation: Decimal,
    ) -> Self {
        let total = input + output + cache_read + cache_creation;
        Self {
            input_usd: input,
            output_usd: output,
            cache_read_usd: cache_read,
            cache_creation_usd: cache_creation,
            total_usd: total,
        }
    }

    /// Decimal total as `f64` for test assertions.
    #[cfg(test)]
    pub fn total_f64(self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.total_usd.to_f64().unwrap_or(0.0)
    }
}

// ---- Per-call Usage Record (provider output → SQLite + JSONL) ----

/// One model API call (per-call granularity). This is the unit a provider
/// emits, the Local Store stores, and one JSONL line serializes.
///
/// `uuid` is the dedup key. `pricing_model` records the normalized model key
/// used to look up the price, so zero-cost rows can be rebilled precisely
/// (freeze + top-up zero-cost only).
///
/// `turn_duration` is intentionally NOT here — a turn spans multiple calls, so
/// it lives in the separate per-turn [`TurnDuration`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRecord {
    pub uuid: String,
    /// ISO8601 UTC, e.g. `2026-07-13T16:55:22.467Z`.
    pub timestamp: String,
    /// Derived `yyyy-mm-dd` (UTC) for daily bucketing.
    pub day: String,
    /// Billed / mapped model, e.g. `glm-5.2`.
    pub model: String,
    /// Normalized model key used for pricing lookup (rebill key).
    pub pricing_model: String,
    /// Provider tag, e.g. `claude_code`.
    pub source: String,
    /// Session this call belongs to: the source log's session identifier
    /// (Claude = the jsonl file stem). Attached grouping info only — the dedup
    /// key stays `(uuid, device_id)`; `session_id` is NOT part of it. Empty when
    /// a provider has not been wired for sessions yet (every source but Claude
    /// in this phase). See `docs/design/session-management.md` §3.1.
    #[serde(default)]
    pub session_id: String,
    /// Owning device's 12-hex id.
    pub device_id: String,
    pub tokens: TokenCounts,
    pub server_tool_use: ServerToolUse,
    /// How the assistant turn terminated: `tool_use` / `end_turn` / ...
    /// Semantic termination reason (NOT an HTTP status). Per-call.
    pub stop_reason: String,
    /// Service tier label, e.g. `standard`. Per-call.
    pub service_tier: String,
    /// Reasoning/thinking iteration count (source array length). 0 when the
    /// model/version records no iterations.
    pub iterations: u32,
    pub cost: CostBreakdown,
}

impl UsageRecord {
    /// Derive the `yyyy-mm-dd` day bucket from an ISO8601 timestamp (UTC).
    /// Falls back to the first 10 chars if parsing fails, so bad input never
    /// drops a record — it just lands in a best-effort bucket.
    pub fn day_from_timestamp(ts: &str) -> String {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
            return t.with_timezone(&chrono::Utc).format("%Y-%m-%d").to_string();
        }
        ts.get(..10).unwrap_or("0000-00-00").to_string()
    }
}

// ---- Per-turn TurnDuration (separate grain from per-call records) ----

/// One turn's wall-clock duration. Sourced from the `system/turn_duration`
/// event's `durationMs`. Kept separate from per-call [`UsageRecord`] because a
/// turn spans multiple API calls — the duration is a turn-level fact, not a
/// per-call one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TurnDuration {
    /// Dedup key (the source `system/turn_duration` event's uuid).
    pub uuid: String,
    pub timestamp: String,
    /// Derived `yyyy-mm-dd` (UTC).
    pub day: String,
    /// Owning device's 12-hex id.
    pub device_id: String,
    /// Turn wall-clock in milliseconds.
    pub duration_ms: u32,
}

// ---- Device-name sync artifact ----

/// A device's published identity, materialized one-per-file at
/// `config/devices_<device_id>.json` (flattened — no `devices/` subdir). Each device writes ONLY its own file, so
/// concurrent edits by different devices never collide (zero Git merge
/// conflict). Only the authoritative self-name syncs; per-device aliases stay
/// local (`config.json`, never in the repo).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceArtifact {
    /// 12-hex device id (matches the filename and `usage_records.device_id`).
    pub device_id: String,
    /// This device's self-chosen display name — the authoritative name other
    /// devices learn by pulling this file.
    pub display_name: String,
    /// ISO8601 UTC of first publish; preserved across rewrites so it stays the
    /// device's stable "first seen" timestamp.
    pub first_seen: String,
}

// ---- Session management (session-as-usage-grouping-key) ----
//
// Session is a grouping key on `usage_records` (ADR 0001), NOT a parallel
// entity. These types model the two layers a session carries:
//   - system data (collect can re-extract and refresh freely), and
//   - user data (custom_title / favorited / group_id — re-extract MUST NOT
//     overwrite; the UPSERT policy + the merge pure-functions in `ingest`
//     enforce this invariant in code, per architecture.md).
// `local_group_id` is local-only (never enters git); only the system data +
// `custom_title` / `favorited` / `synced_group_id` ride the `session-meta` grain.

/// Session system data: the layer collect re-extracts from the source log on
/// every pass. Refreshable — re-collecting a session updates these fields in
/// place. This is a strict subset of [`SessionMetaSync`] (which adds the
/// syncable user data) and of the SQLite `sessions` row (which also adds the
/// local-only `local_group_id`).
///
/// Also serves as the provider output type alias [`RawSession`] — there is no
/// device/cost attaching step for sessions (unlike `RawUsage` → `UsageRecord`),
/// so the provider-output shape and the system-data layer are identical. One
/// struct, one source of truth (architecture.md).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct SessionSystemData {
    /// Session id (Claude = the jsonl file stem).
    pub id: String,
    /// Provider tag, e.g. `claude_code`.
    pub source: String,
    /// Working directory the session ran in (Claude `cwd`).
    pub project_dir: String,
    /// Best-effort original title (Claude `summary` / first user message).
    pub title_orig: String,
    /// ISO8601 of the first event observed in the source log.
    pub started_at: String,
    /// ISO8601 of the most recent event observed. Drives cross-day snapshot
    /// merge (latest wins) and session-list ordering.
    pub last_active_at: String,
}

/// Provider-output alias for a parsed session (pre-device). Identical to
/// [`SessionSystemData`] — no device/cost step exists for sessions, so the two
/// concepts share one struct (single source of truth).
pub type RawSession = SessionSystemData;

/// `session-meta-<day>.jsonl` grain row = system data + the SYNCABLE user data
/// (`custom_title` / `favorited` / `synced_group_id`). Deliberately does NOT
/// carry `local_group_id` — that field is device-private and never enters git
/// (ADR 0002). A session spanning multiple days writes one snapshot per day's
/// file; readers merge by `id` (see `ingest::merge_session_snapshots`).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct SessionMetaSync {
    // ---- system data (refreshable) ----
    pub id: String,
    pub source: String,
    pub project_dir: String,
    pub title_orig: String,
    pub started_at: String,
    pub last_active_at: String,
    // ---- syncable user data (re-extract never overwrites; COALESCE on merge) ----
    /// User rename; display-priority over `title_orig`.
    #[serde(default)]
    pub custom_title: String,
    /// Favorited star; true ⇒ this device collects and syncs the transcript.
    #[serde(default)]
    pub favorited: bool,
    /// Synced-group membership (cross-device, in-git). Empty = ungrouped.
    #[serde(default)]
    pub synced_group_id: String,
}

/// Role of a transcript line. Matches Claude Code's event types, collapsed to
/// the four values the UI reasons about.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum SessionMessageRole {
    #[default]
    User,
    Assistant,
    Tool,
    System,
}

/// One transcript line. Single source of truth across three roles: provider
/// output (`RawSessionMessage` concept), the per-session JSONL Artifact
/// (`sessions/<id>.jsonl`), and the DTO crossing to the frontend. The shape is
/// identical for all three, so one struct (single source of truth) — the
/// `RawSessionMessage` name the design doc uses is a role, not a separate type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SessionMessage {
    /// Source event uuid (dedup key within one session's transcript file).
    pub uuid: String,
    /// Session this message belongs to.
    pub session_id: String,
    pub role: SessionMessageRole,
    /// ISO8601 timestamp of the source event.
    pub ts: String,
    /// Model on assistant messages (None for user/tool/system).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Tool name on tool_use messages (None otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Trimmed text content: text blocks for user/assistant, the tool_use `name`
    /// summary for tool calls; thinking blocks' full text, base64 images, and
    /// >32 KB tool_results are filtered/truncated at collect time.
    pub content: String,
}

/// One session row for the frontend list. Aggregates (request_count /
/// total_tokens / total_cost_usd) are computed live by `GROUP BY session_id`
/// over `usage_records` at query time — they are NOT stored on the session, so
/// there is no second source of token/cost truth to drift (ADR 0001).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SessionRow {
    pub id: String,
    pub device_id: String,
    pub source: String,
    pub project_dir: String,
    /// Display title: `custom_title` when set, else `title_orig`.
    pub title: String,
    pub favorited: bool,
    pub local_group_id: String,
    pub synced_group_id: String,
    pub started_at: String,
    pub last_active_at: String,
    /// Live aggregate over `usage_records` for this session.
    pub request_count: u32,
    /// Live aggregate: sum of all four token buckets.
    pub total_tokens: u32,
    /// Live aggregate: sum of cost.
    pub total_cost_usd: f64,
}

/// Optional filter for `query_sessions`. Every field optional; `None` = no
/// constraint. Mirrors the shape of `UsageFilter` for the session list.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SessionFilter {
    /// Scope to one device (`None` = all devices).
    pub device_scope: Option<String>,
    /// Scope to one source, e.g. `claude_code`.
    pub source: Option<String>,
    /// `Some(true)` = only favorited; `Some(false)` = only non-favorited.
    pub favorited: Option<bool>,
    /// Scope to a local group (empty string matches ungrouped).
    pub local_group_id: Option<String>,
    /// Scope to a synced group (empty string matches ungrouped).
    pub synced_group_id: Option<String>,
    /// Inclusive lower bound on `last_active_at` (ISO8601).
    pub from_ts: Option<String>,
    /// Inclusive upper bound on `last_active_at` (ISO8601).
    pub to_ts: Option<String>,
}

/// One group entry for the frontend, unified across the two tracks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SessionGroup {
    pub id: String,
    pub name: String,
    /// `"local"` (device-private SQLite) or `"synced"` (per-device groups.json).
    pub kind: String,
    /// Owning device id. Only meaningful for `kind == "synced"`; empty for local.
    pub device_id: String,
}

/// A local group row (SQLite `local_groups`; device-private, never enters git).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct LocalGroup {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// A synced-group row (`data/<deviceId>/groups.json`; cross-device via git).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct SyncedGroup {
    pub id: String,
    pub name: String,
    /// Owning device (the one that created the group). Encoded in the id prefix
    /// too, but kept here for read-without-parse convenience.
    pub device_id: String,
    pub updated_at: String,
}

// ---- DTOs crossing the boundary (specta-typed, f64 cost) ----

/// One row of the request-log table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UsageLogRow {
    pub uuid: String,
    pub timestamp: String,
    pub model: String,
    pub source: String,
    pub device_id: String,
    pub tokens: TokenCounts,
    pub stop_reason: String,
    pub total_cost_usd: f64,
}

/// Aggregate totals over a filtered range.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UsageStats {
    pub request_count: u32,
    pub total_tokens: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_tokens: u32,
    pub cache_read_tokens: u32,
    /// Cache-hit ratio in [0,1].
    pub cache_hit_rate: f64,
    pub total_cost_usd: f64,
    /// Aggregate over TurnDuration rows in range (per-turn grain).
    pub turn_count: u32,
    pub avg_turn_duration_ms: f64,
}

/// Per-model aggregate row (for breakdown tables / model filter).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelStatsRow {
    pub model: String,
    pub request_count: u32,
    pub total_tokens: u32,
    pub total_cost_usd: f64,
}

/// One point on the trend chart. `day` carries the bucket key: a `YYYY-MM-DD`
/// UTC day (`TrendBucket::Day`) or a `YYYY-MM-DDTHH` local hour
/// (`TrendBucket::Hour`). The field keeps the `day` name for wire stability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TrendPoint {
    pub day: String,
    pub total_tokens: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_tokens: u32,
    pub cache_read_tokens: u32,
    pub total_cost_usd: f64,
}

/// Trend aggregation granularity. `Day` groups on the UTC `day` column
/// (cross-device deterministic); `Hour` groups on local-time hour,
/// used for the single-day zoom where per-day resolution collapses to one bar.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum TrendBucket {
    Day,
    Hour,
}

/// Filter args shared by stats / trend / logs queries.
///
/// All fields optional; `None` means "no constraint". `device_scope` is the
/// semantic cache-key axis: `None` = all devices.
///
/// Range bounds are ISO8601 **timestamps**, not `day` strings. The `day` column
/// is a UTC whole-day bucket (cross-device determinism), so a local
/// "today" in a non-UTC zone (e.g. UTC+8) straddles two UTC days; filtering on
/// `day` would drop early-morning rows. The frontend converts its local-day
/// range to UTC timestamps, and we filter on `timestamp` (amendment:
/// `day` stays the UTC bucket for grouping/trend only).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UsageFilter {
    /// Inclusive lower ISO8601 UTC timestamp, e.g. `2026-07-21T16:00:00Z`.
    pub from_ts: Option<String>,
    /// Inclusive upper ISO8601 UTC timestamp.
    pub to_ts: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
    pub device_scope: Option<String>,
}

/// Query params for the request-log endpoint (adds paging to `UsageFilter`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LogsQuery {
    pub filter: UsageFilter,
    pub limit: u32,
    pub offset: u32,
}

// ---- Pricing ----

/// A pricing entry: USD per 1M tokens for each bucket.
///
/// Cost crosses as `f64` for the UI; internally stored as Decimal TEXT.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PricingEntry {
    /// Normalized model key (primary key).
    pub model_key: String,
    pub display_name: String,
    /// USD per 1M input tokens.
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_creation_per_million: f64,
    /// True when seeded from LiteLLM upstream, false when user-defined/edited.
    pub is_builtin: bool,
}

// ---- Device & mode ----

/// A known device. `is_self` marks the device running this instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub is_self: bool,
    pub first_seen: String,
}

/// Run mode: default Standalone; Synced once a repo is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Standalone,
    Synced,
}

// ---- Model-key normalization (single source of truth) ----
//
// One neutral home for the "normalized model key" concept, built from
// orthogonal sub-steps that callers compose to match their semantics. This
// replaces the two former divergent implementations (`pricing::normalize_key`
// and `codex::normalize_codex_model`) so the rules can no longer silently
// drift apart — architecture review #11.
//
// Two ready-made combinations are exposed:
//   - [`normalize_model_key`]: the superset (prefix + brackets + ISO date +
//     compact date). Used by providers whose raw model names carry provider
//     prefixes and date stamps (e.g. Codex).
//   - [`normalize_pricing_key`]: the pricing "basic set" (brackets + compact
//     date). Used by the pricing book and the ingest rebill key, preserving
//     the former `pricing::normalize_key` lookup behavior exactly.

/// Strip a `provider/` prefix: keep the tail after the last `/`. No-op when the
/// name has no `/`. e.g. `openai/gpt-5.4` → `gpt-5.4`.
pub(crate) fn strip_provider_prefix(name: &str) -> &str {
    match name.rfind('/') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

/// Strip a `[...]` bracketed suffix such as the `[1m]` context-window tag.
/// Returns the part before the first `[`, trailing whitespace trimmed. No-op
/// when the name has no `[`. e.g. `glm-5.2[1m]` → `glm-5.2`.
pub(crate) fn strip_brackets(name: &str) -> &str {
    match name.find('[') {
        Some(pos) => name[..pos].trim_end(),
        None => name,
    }
}

/// Strip a trailing ISO date `-YYYY-MM-DD` (11 chars). No-op when absent.
pub(crate) fn strip_iso_date_suffix(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() > 11 && name.is_char_boundary(bytes.len() - 11) {
        let tail = &name[bytes.len() - 11..];
        if tail.is_ascii()
            && tail.as_bytes()[0] == b'-'
            && tail[1..5].bytes().all(|b| b.is_ascii_digit())
            && tail.as_bytes()[5] == b'-'
            && tail[6..8].bytes().all(|b| b.is_ascii_digit())
            && tail.as_bytes()[8] == b'-'
            && tail[9..11].bytes().all(|b| b.is_ascii_digit())
        {
            return &name[..bytes.len() - 11];
        }
    }
    name
}

/// Strip a trailing compact date `-YYYYMMDD` (a `-` followed by exactly 8
/// digits, 9 chars total). No-op when absent.
pub(crate) fn strip_compact_date_suffix(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 9 && name.is_char_boundary(bytes.len() - 9) {
        let tail = &name[bytes.len() - 9..];
        if tail.starts_with('-') && tail[1..].bytes().all(|b| b.is_ascii_digit()) {
            return &name[..bytes.len() - 9];
        }
    }
    name
}

/// Canonical model-key normalization — the superset: ASCII-lowercase, strip a
/// `provider/` prefix, strip `[...]` brackets, then strip trailing ISO
/// (`-YYYY-MM-DD`) and compact (`-YYYYMMDD`) date suffixes. Used by providers
/// whose raw model names carry prefixes/date stamps. Every sub-step is a no-op
/// when its pattern is absent, so this never changes a name that did not carry
/// that pattern.
pub(crate) fn normalize_model_key(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    let after_prefix = strip_provider_prefix(&lower);
    let after_brackets = strip_brackets(after_prefix);
    let after_iso = strip_iso_date_suffix(after_brackets);
    strip_compact_date_suffix(after_iso).to_string()
}

/// Pricing-table key normalization — the "basic set": ASCII-lowercase, strip
/// `[...]` brackets and a trailing compact (`-YYYYMMDD`) date. Preserves the
/// former `pricing::normalize_key` verbatim, so pricing keys stay stable and
/// lookups + rebill keys are unchanged. Deliberately omits prefix and ISO-date
/// stripping; see [`normalize_model_key`] for the superset.
pub(crate) fn normalize_pricing_key(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    let after_brackets = strip_brackets(&lower);
    strip_compact_date_suffix(after_brackets).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_from_timestamp_utc_bucket() {
        assert_eq!(
            UsageRecord::day_from_timestamp("2026-07-13T16:55:22.467Z"),
            "2026-07-13"
        );
    }

    #[test]
    fn day_from_timestamp_garbage_falls_back_to_prefix() {
        // Unparseable but ≥10 chars ⇒ first 10 chars as the day bucket.
        assert_eq!(
            UsageRecord::day_from_timestamp("garbage-input-here"),
            "garbage-in"
        );
        // <10 chars ⇒ the explicit fallback sentinel.
        assert_eq!(UsageRecord::day_from_timestamp("short"), "0000-00-00");
    }

    #[test]
    fn token_total_sums_four_buckets() {
        let t = TokenCounts {
            input: 100,
            output: 50,
            cache_creation: 10,
            cache_read: 90,
        };
        assert_eq!(t.total(), 250);
    }

    #[test]
    fn token_cache_hit_rate() {
        let t = TokenCounts {
            input: 100,
            output: 50,
            cache_creation: 10,
            cache_read: 90,
        };
        assert!((t.cache_hit_rate() - 90.0 / 200.0).abs() < 1e-9);
        // Nothing cacheable ⇒ 0.
        let z = TokenCounts {
            input: 0,
            output: 5,
            cache_creation: 0,
            cache_read: 0,
        };
        assert_eq!(z.cache_hit_rate(), 0.0);
    }

    #[test]
    fn cost_breakdown_total_is_bucket_sum() {
        let cb = CostBreakdown::from_buckets(
            Decimal::from_str("1.0").unwrap(),
            Decimal::from_str("2.0").unwrap(),
            Decimal::from_str("0.5").unwrap(),
            Decimal::from_str("0.5").unwrap(),
        );
        assert_eq!(cb.total_usd, Decimal::from_str("4.0").unwrap());
    }

    #[test]
    fn usage_record_carries_new_per_call_fields() {
        let r = UsageRecord {
            uuid: "u1".into(),
            timestamp: "2026-07-21T10:00:00Z".into(),
            day: "2026-07-21".into(),
            model: "glm-5.2".into(),
            pricing_model: "glm-5.2".into(),
            source: "claude_code".into(),
            session_id: "session-abc".into(),
            device_id: "abc123def456".into(),
            tokens: TokenCounts::default(),
            server_tool_use: ServerToolUse::default(),
            stop_reason: "tool_use".into(),
            service_tier: "standard".into(),
            iterations: 3,
            cost: CostBreakdown::default(),
        };
        assert_eq!(r.stop_reason, "tool_use");
        assert_eq!(r.service_tier, "standard");
        assert_eq!(r.iterations, 3);
        assert_eq!(r.session_id, "session-abc");
    }

    #[test]
    fn usage_record_session_id_defaults_empty_when_absent_in_jsonl() {
        // An older Artifact line (pre-session) lacks `session_id`. It must
        // deserialize with an empty default rather than fail — the column was
        // added after the initial schema, and peers may still carry old lines.
        let json = r#"{"uuid":"u1","timestamp":"2026-07-21T10:00:00Z","day":"2026-07-21","model":"glm-5.2","pricing_model":"glm-5.2","source":"claude_code","device_id":"abc123def456","tokens":{"input":0,"output":0,"cache_creation":0,"cache_read":0},"server_tool_use":{"web_search":0,"web_fetch":0},"stop_reason":"","service_tier":"","iterations":0,"cost":{"input_usd":"0","output_usd":"0","cache_read_usd":"0","cache_creation_usd":"0","total_usd":"0"}}"#;
        let r: UsageRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.session_id, "", "absent session_id ⇒ empty default");
    }

    #[test]
    fn session_types_roundtrip() {
        let sys = SessionSystemData {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/proj".into(),
            title_orig: "Hello".into(),
            started_at: "2026-08-01T10:00:00Z".into(),
            last_active_at: "2026-08-01T11:00:00Z".into(),
        };
        let s: SessionSystemData =
            serde_json::from_str(&serde_json::to_string(&sys).unwrap()).unwrap();
        assert_eq!(s, sys);

        // SessionMetaSync round-trips its three user-data fields.
        let meta = SessionMetaSync {
            id: "s1".into(),
            source: "claude_code".into(),
            project_dir: "/p".into(),
            title_orig: "orig".into(),
            started_at: "2026-08-01T10:00:00Z".into(),
            last_active_at: "2026-08-01T11:00:00Z".into(),
            custom_title: "Renamed".into(),
            favorited: true,
            synced_group_id: "dev-abcd1234".into(),
        };
        let m: SessionMetaSync =
            serde_json::from_str(&serde_json::to_string(&meta).unwrap()).unwrap();
        assert_eq!(m, meta);

        // A meta row missing the user-data keys (older grain line) deserializes
        // to the defaults — `#[serde(default)]` on each user-data column.
        let partial = r#"{"id":"s1","source":"claude_code","project_dir":"","title_orig":"","started_at":"","last_active_at":""}"#;
        let pm: SessionMetaSync = serde_json::from_str(partial).unwrap();
        assert_eq!(pm.custom_title, "");
        assert!(!pm.favorited);
        assert_eq!(pm.synced_group_id, "");
    }

    #[test]
    fn session_message_roundtrips_and_skips_none_extras() {
        let m = SessionMessage {
            uuid: "e1".into(),
            session_id: "s1".into(),
            role: SessionMessageRole::Assistant,
            ts: "2026-08-01T10:00:00Z".into(),
            model: Some("glm-5.2".into()),
            name: None,
            content: "hi".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: SessionMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        // `name: None` is skipped on serialize (skip_serializing_if).
        assert!(!json.contains("\"name\""));
    }

    #[test]
    fn turn_duration_roundtrips() {
        let td = TurnDuration {
            uuid: "t1".into(),
            timestamp: "2026-07-21T10:00:00Z".into(),
            day: "2026-07-21".into(),
            device_id: "abc123def456".into(),
            duration_ms: 209_499,
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: TurnDuration = serde_json::from_str(&json).unwrap();
        assert_eq!(back, td);
    }

    // ---- Model-key normalization sub-steps ----

    #[test]
    fn strip_provider_prefix_keeps_tail_after_last_slash() {
        assert_eq!(strip_provider_prefix("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(strip_provider_prefix("a/b/c"), "c");
        // No slash → unchanged.
        assert_eq!(strip_provider_prefix("gpt-5.4"), "gpt-5.4");
    }

    #[test]
    fn strip_brackets_drops_context_window_tag() {
        assert_eq!(strip_brackets("glm-5.2[1m]"), "glm-5.2");
        // Trailing whitespace before the bracket is trimmed.
        assert_eq!(strip_brackets("glm-5.2 [1m]"), "glm-5.2");
        // No bracket → unchanged.
        assert_eq!(strip_brackets("gpt-5.4"), "gpt-5.4");
    }

    #[test]
    fn strip_iso_date_suffix_matches_only_dashed_iso_form() {
        assert_eq!(strip_iso_date_suffix("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            strip_iso_date_suffix("gpt-5.4-pro-2026-03-05"),
            "gpt-5.4-pro"
        );
        // Compact 8-digit form is NOT the ISO step's concern.
        assert_eq!(
            strip_iso_date_suffix("gpt-5.4-20260305"),
            "gpt-5.4-20260305"
        );
        // Non-date tail → unchanged.
        assert_eq!(strip_iso_date_suffix("gpt-5.2-codex"), "gpt-5.2-codex");
    }

    #[test]
    fn strip_compact_date_suffix_matches_only_eight_digit_form() {
        assert_eq!(
            strip_compact_date_suffix("claude-3-5-haiku-20241022"),
            "claude-3-5-haiku"
        );
        assert_eq!(strip_compact_date_suffix("gpt-5.4-20260305"), "gpt-5.4");
        // ISO form (dashes inside) is NOT the compact step's concern.
        assert_eq!(
            strip_compact_date_suffix("gpt-5.4-2026-03-05"),
            "gpt-5.4-2026-03-05"
        );
        // Non-date tail → unchanged.
        assert_eq!(strip_compact_date_suffix("gpt-5.2-codex"), "gpt-5.2-codex");
    }

    // ---- Model-key normalization entry points ----

    #[test]
    fn normalize_model_key_applies_the_full_superset() {
        // Lowercase + prefix + ISO date.
        assert_eq!(normalize_model_key("OPENAI/GPT-5.4-2026-03-05"), "gpt-5.4");
        // Lowercase + prefix + compact date.
        assert_eq!(normalize_model_key("openai/gpt-5.4-20260305"), "gpt-5.4");
        // Lowercase only.
        assert_eq!(normalize_model_key("GLM-4.6"), "glm-4.6");
        // ISO date with a version token before it.
        assert_eq!(normalize_model_key("gpt-5.4-pro-2026-03-05"), "gpt-5.4-pro");
        // Compact date after a versioned name.
        assert_eq!(
            normalize_model_key("claude-opus-4-6-20260206"),
            "claude-opus-4-6"
        );
        // No prefix/date/brackets → only lowercased.
        assert_eq!(normalize_model_key("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(normalize_model_key("o3"), "o3");
        // Brackets are stripped too: a no-op for Codex today, but the superset
        // keeps the rule so a future bracketed Codex name still matches.
        assert_eq!(normalize_model_key("openai/gpt-5.4[1m]"), "gpt-5.4");
    }

    #[test]
    fn normalize_pricing_key_preserves_the_basic_set() {
        // Bracket strip + lowercase (the [1m] transit-model case).
        assert_eq!(normalize_pricing_key("glm-5.2[1m]"), "glm-5.2");
        // Lowercase + compact date strip (Anthropic-style date stamp).
        assert_eq!(
            normalize_pricing_key("Claude-3-5-Haiku-20241022"),
            "claude-3-5-haiku"
        );
        assert_eq!(normalize_pricing_key("GPT-4o"), "gpt-4o");
        // No bracket/date → only lowercased.
        assert_eq!(
            normalize_pricing_key("claude-3-5-sonnet"),
            "claude-3-5-sonnet"
        );
        // The basic set deliberately does NOT strip prefixes or ISO dates —
        // pinning the former pricing::normalize_key behavior (the divergence
        // is intentional and currently harmless; see architecture review #11).
        assert_eq!(
            normalize_pricing_key("openai/gpt-5.4-2026-03-05"),
            "openai/gpt-5.4-2026-03-05"
        );
    }
}
