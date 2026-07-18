//! Core domain model: the per-request Usage Record and the DTOs that cross the
//! Rust→JS boundary (ADR-0003 / 0004 / 0008 / 0009).
//!
//! Naming is snake_case end-to-end (Rust ↔ SQLite ↔ JSONL ↔ TS) — one uniform
//! convention, no rename transforms that could drift across the specta/serde
//! boundary (ADR-0012: reliability first).
//!
//! Boundary type rules (ADR-0008): no BigInt-style ints (usize/i64/...). Token
//! counts are `u32` (max ~4.29e9, far above any context size); timestamps cross
//! as ISO8601 strings; cost crosses as `f64` (display-only on the JS side — the
//! JS layer never recomputes cost, ADR-0009), while internally cost is kept as
//! `rust_decimal::Decimal` for precision and stored as TEXT in SQLite.

use std::str::FromStr;

use rust_decimal::Decimal;

// ---- Token / tool sub-structures (shared by internal record + DTOs) ----

/// Token four-pack (ADR-0003). `u32` across the boundary (ADR-0008).
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
    /// Sum of all four buckets — "真实消耗 Tokens" in the dashboard (BLUEPRINT).
    pub fn total(self) -> u32 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
    }

    /// Cache-hit rate as a ratio in [0,1] for display (0 when nothing cacheable).
    /// Denominator = fresh input + cache reads (the "could have been cached" pool).
    pub fn cache_hit_rate(self) -> f64 {
        let denom = self.input as f64 + self.cache_read as f64;
        if denom <= 0.0 {
            0.0
        } else {
            self.cache_read as f64 / denom
        }
    }
}

/// Server-side tool usage reported by some providers (BLUEPRINT bonus column).
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

/// Cost split by token bucket, in USD (ADR-0009: computed at ingest, stored).
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

    pub fn total_f64(self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.total_usd.to_f64().unwrap_or(0.0)
    }
}

// ---- Internal Usage Record (provider output → SQLite + JSONL) ----

/// One model API call (ADR-0003: per-request granularity). This is the unit a
/// provider emits, the Local Store stores, and one JSONL line serializes.
///
/// `uuid` is the dedup key (ADR-0005 ledger). `pricing_model` records the model
/// key used to look up the price, so zero-cost rows can be rebilled precisely
/// (ADR-0009: freeze + top-up zero-cost only).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRecord {
    pub uuid: String,
    /// ISO8601 UTC, e.g. `2026-07-13T16:55:22.467Z`.
    pub timestamp: String,
    /// Derived `yyyy-mm-dd` (UTC) for daily bucketing.
    pub day: String,
    /// Billed / mapped model, e.g. `glm-5.2`.
    pub model: String,
    /// Normalized model key used for pricing lookup (ADR-0009 rebill key).
    pub pricing_model: String,
    /// Provider tag, e.g. `claude_code` (ADR-0001).
    pub source: String,
    /// Owning device's 12-hex id (ADR-0002).
    pub device_id: String,
    pub tokens: TokenCounts,
    pub server_tool_use: ServerToolUse,
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

// ---- DTOs crossing the boundary (specta-typed, f64 cost) ----

/// One row of the request-log table (BLUEPRINT 请求日志; ADR-0003 columns).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UsageLogRow {
    pub uuid: String,
    pub timestamp: String,
    pub model: String,
    pub source: String,
    pub device_id: String,
    pub tokens: TokenCounts,
    pub total_cost_usd: f64,
}

/// Aggregate totals over a filtered range (BLUEPRINT 使用统计).
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
}

/// Per-model aggregate row (for breakdown tables / model filter).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelStatsRow {
    pub model: String,
    pub request_count: u32,
    pub total_tokens: u32,
    pub total_cost_usd: f64,
}

/// One point on the trend chart (per day).
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

/// Filter args shared by stats / trend / logs queries (ADR-0007 query boundary).
///
/// All fields optional; empty/None means "no constraint". `device_scope` is the
/// semantic cache-key axis (ADR-0008): `None` = all devices.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UsageFilter {
    /// Inclusive lower ISO8601 day (`yyyy-mm-dd`).
    pub from_day: Option<String>,
    /// Inclusive upper ISO8601 day.
    pub to_day: Option<String>,
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

// ---- Pricing (ADR-0006 / 0009) ----

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

/// A known device (ADR-0002). `is_self` marks the device running this instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub is_self: bool,
    pub first_seen: String,
}

/// Run mode (ADR-0011): default Standalone; Synced once a repo is configured.
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
        assert!((t.cache_hit_rate() - 90.0 / 190.0).abs() < 1e-9);
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
}
