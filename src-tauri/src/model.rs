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
/// (ADR-0004, cross-device deterministic); `Hour` groups on local-time hour,
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
/// is a UTC whole-day bucket (ADR-0004 cross-device determinism), so a local
/// "today" in a non-UTC zone (e.g. UTC+8) straddles two UTC days; filtering on
/// `day` would drop early-morning rows. The frontend converts its local-day
/// range to UTC timestamps, and we filter on `timestamp` (ADR-0004 amendment:
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
}
