//! Pricing table + cost calculator (ADR-0007).
//!
//! Cost is computed at ingest time by a pure, unit-testable function
//! (`CostCalculator`), looking up per-million-token rates with model-key
//! normalization + prefix fallback, and cache heuristics when cache rates are
//! unspecified. A whole model with no rate ⇒ cost 0 (recorded for later top-up,
//! ADR-0007: freeze + top-up zero-cost only). Rates are `rust_decimal::Decimal`
//! for precision; the DB column is TEXT, the DTO exposes `f64`.

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::error::{AppError, AppResult};
use crate::model::{CostBreakdown, PricingEntry, TokenCounts};

/// Per-million-token USD rates for one model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    pub model_key: String,
    pub display_name: String,
    pub input: Decimal,
    pub output: Decimal,
    pub cache_read: Decimal,
    pub cache_creation: Decimal,
    pub is_builtin: bool,
}

impl ModelPricing {
    /// Convert to the boundary DTO (f64 rates, ADR-0008 / 0007).
    pub fn to_entry(&self) -> PricingEntry {
        PricingEntry {
            model_key: self.model_key.clone(),
            display_name: self.display_name.clone(),
            input_per_million: self.input.to_f64().unwrap_or(0.0),
            output_per_million: self.output.to_f64().unwrap_or(0.0),
            cache_read_per_million: self.cache_read.to_f64().unwrap_or(0.0),
            cache_creation_per_million: self.cache_creation.to_f64().unwrap_or(0.0),
            is_builtin: self.is_builtin,
        }
    }

    /// Parse from a DTO (UI writes back f64 → Decimal).
    pub fn from_entry(e: &PricingEntry) -> AppResult<Self> {
        let d = |f: f64| AppResult::Ok(Decimal::try_from(f)?);
        Ok(Self {
            model_key: e.model_key.clone(),
            display_name: e.display_name.clone(),
            input: d(e.input_per_million)?,
            output: d(e.output_per_million)?,
            cache_read: d(e.cache_read_per_million)?,
            cache_creation: d(e.cache_creation_per_million)?,
            is_builtin: e.is_builtin,
        })
    }
}

/// In-memory pricing book keyed by normalized model key (used at ingest).
#[derive(Debug, Clone, Default)]
pub struct PricingBook {
    by_key: HashMap<String, ModelPricing>,
}

impl PricingBook {
    pub fn from_iter<I: IntoIterator<Item = ModelPricing>>(iter: I) -> Self {
        let mut by_key = HashMap::new();
        for p in iter {
            by_key.insert(normalize_key(&p.model_key), p);
        }
        Self { by_key }
    }

    /// Resolve a runtime model string to usable rates (ADR-0007):
    /// normalization candidates → exact → prefix fallback. Cache rates left at
    /// zero are filled with the documented heuristics (cache write ≈ 1.25×
    /// input, cache read ≈ 0.1× input). Returns `None` if no model matches at
    /// all (⇒ zero-cost placeholder).
    pub fn resolve(&self, model: &str) -> Option<ResolvedRate> {
        for candidate in normalization_candidates(model) {
            if let Some(p) = self.by_key.get(&candidate) {
                return Some(ResolvedRate::from_pricing(p));
            }
        }
        None
    }
}

/// Resolved, ingest-ready rates with cache heuristics applied.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedRate {
    pub input: Decimal,
    pub output: Decimal,
    pub cache_read: Decimal,
    pub cache_creation: Decimal,
}

impl ResolvedRate {
    fn from_pricing(p: &ModelPricing) -> Self {
        // Cache heuristics (ADR-0007) only kick in when the rate is unset (0).
        let zero = Decimal::ZERO;
        let cache_read = if p.cache_read == zero {
            p.input * dec("0.1")
        } else {
            p.cache_read
        };
        let cache_creation = if p.cache_creation == zero {
            p.input * dec("1.25")
        } else {
            p.cache_creation
        };
        Self {
            input: p.input,
            output: p.output,
            cache_read,
            cache_creation,
        }
    }
}

/// Pure cost calculator (ADR-0007: unit-testable). `rate` is per 1M tokens.
pub struct CostCalculator;

impl CostCalculator {
    /// Compute the cost breakdown for one request. `None` rate ⇒ all-zero
    /// (model missing from book) — the row is stored as a zero-cost placeholder.
    pub fn calc(tokens: TokenCounts, rate: Option<ResolvedRate>) -> CostBreakdown {
        let Some(rate) = rate else {
            return CostBreakdown::default();
        };
        let per_token = |rate: Decimal, tokens: u32| {
            // rate is USD per 1M tokens → cost = rate * tokens / 1_000_000.
            rate * Decimal::from(tokens) / Decimal::from(1_000_000)
        };
        CostBreakdown::from_buckets(
            per_token(rate.input, tokens.input),
            per_token(rate.output, tokens.output),
            per_token(rate.cache_read, tokens.cache_read),
            per_token(rate.cache_creation, tokens.cache_creation),
        )
    }
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap_or(Decimal::ZERO)
}

/// Normalize a model key for matching: lowercase, strip `[...]` brackets and
/// trailing `-yyyymmdd` dates. e.g. `glm-5.2[1m]` → `glm-5.2`,
/// `claude-3-5-haiku-20241022` → `claude-3-5-haiku`.
pub fn normalize_key(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    // drop bracketed suffixes like [1m]
    let no_brackets = lower.split('[').next().unwrap_or(&lower).trim_end();
    // drop trailing 8-digit date
    let bytes = no_brackets.as_bytes();
    let cut = if bytes.len() >= 9 {
        let tail = &no_brackets[no_brackets.len() - 9..];
        if tail.starts_with('-') && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            no_brackets.len() - 9
        } else {
            no_brackets.len()
        }
    } else {
        no_brackets.len()
    };
    no_brackets[..cut].to_string()
}

/// Ordered candidates to try: full normalized key, then progressively shorter
/// `-`-delimited prefixes (prefix fallback, ADR-0007).
fn normalization_candidates(model: &str) -> Vec<String> {
    let norm = normalize_key(model);
    let mut out = vec![norm.clone()];
    let parts: Vec<&str> = norm.split('-').collect();
    // build prefixes by dropping trailing segments (keep ≥1 segment)
    for end in (1..parts.len()).rev() {
        let prefix = parts[..end].join("-");
        if !out.contains(&prefix) {
            out.push(prefix);
        }
    }
    out
}

/// A small built-in seed so cost calc works offline before the LiteLLM fetch
/// (ADR-0007). These are **bootstrap placeholders** — refresh via LiteLLM or
/// edit in the Pricing UI; treat values as approximate.
///
/// `glm-5.2` is included because it is the transit model used via CC-Switch in
/// the session-log sample (`docs/research/claude-code-session-fields.md`).
pub fn builtin_seed() -> Vec<ModelPricing> {
    let row = |key: &str, name: &str, i: &str, o: &str, cr: &str, cc: &str| ModelPricing {
        model_key: key.to_string(),
        display_name: name.to_string(),
        input: dec(i),
        output: dec(o),
        cache_read: dec(cr),
        cache_creation: dec(cc),
        is_builtin: true,
    };
    // (key, display, input, output, cache_read/1M, cache_creation/1M) — USD.
    vec![
        // The user's transit model via CC-Switch. Placeholder price, verify/override.
        row("glm-5.2", "GLM 5.2", "0.60", "2.20", "0.06", "0.75"),
        // Anthropic Claude family (approx, public list prices).
        row(
            "claude-fable-5",
            "Claude Fable 5",
            "5.00",
            "25.00",
            "0.50",
            "6.25",
        ),
        row(
            "claude-opus-4-8",
            "Claude Opus 4.8",
            "15.00",
            "75.00",
            "1.50",
            "18.75",
        ),
        row(
            "claude-sonnet-5",
            "Claude Sonnet 5",
            "3.00",
            "15.00",
            "0.30",
            "3.75",
        ),
        row(
            "claude-haiku-4-5",
            "Claude Haiku 4.5",
            "1.00",
            "5.00",
            "0.10",
            "1.25",
        ),
        row(
            "claude-3-5-haiku",
            "Claude 3.5 Haiku",
            "0.80",
            "4.00",
            "0.08",
            "1.00",
        ),
        row(
            "claude-3-5-sonnet",
            "Claude 3.5 Sonnet",
            "3.00",
            "15.00",
            "0.30",
            "3.75",
        ),
        // OpenAI (approx).
        row("gpt-5.6", "GPT-5.6", "2.50", "10.00", "0.25", "2.50"),
        row("gpt-4o", "GPT-4o", "2.50", "10.00", "1.25", "2.50"),
        // Google (approx).
        row(
            "gemini-2.5-pro",
            "Gemini 2.5 Pro",
            "1.25",
            "5.00",
            "0.125",
            "1.25",
        ),
    ]
}

/// Convenience: a book preloaded with the built-in seed.
pub fn seed_book() -> PricingBook {
    PricingBook::from_iter(builtin_seed())
}

/// Parse a JSON pricing document (the cloud `pricing.json` shape) into entries.
/// Accepts either an array of `PricingEntry` or `{ "models": [...] }`.
pub fn parse_pricing_doc(json: &str) -> AppResult<Vec<ModelPricing>> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        models: Vec<PricingEntry>,
    }
    let trimmed = json.trim_start();
    let entries: Vec<PricingEntry> = if trimmed.starts_with('{') {
        serde_json::from_str::<Wrapper>(json)?.models
    } else {
        serde_json::from_str(json)?
    };
    entries.iter().map(ModelPricing::from_entry).collect()
}

/// Serialize pricing entries back to the cloud doc shape (`{ "models": [...] }`).
pub fn write_pricing_doc(entries: &[ModelPricing]) -> AppResult<String> {
    let dto: Vec<PricingEntry> = entries.iter().map(ModelPricing::to_entry).collect();
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({ "models": dto }),
    )?)
}

/// Fetch the LiteLLM upstream price sheet and convert it to per-million-token
/// pricing entries (ADR-0007 seed). LiteLLM lists costs **per token**, so each
/// is multiplied by 1e6. Best-effort: the caller treats network failure as
/// "keep using the existing book" (offline-first, ADR-0007 fallback).
pub fn fetch_litellm() -> AppResult<Vec<ModelPricing>> {
    const URL: &str =
        "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
    let resp = ureq::get(URL)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| AppError::Pricing(format!("litellm fetch failed: {e}")))?;
    let body = resp
        .into_string()
        .map_err(|e| AppError::Pricing(format!("litellm read failed: {e}")))?;
    parse_litellm_body(&body)
}

/// Parse a LiteLLM price sheet body into per-million pricing entries.
fn parse_litellm_body(body: &str) -> AppResult<Vec<ModelPricing>> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    let obj = v
        .as_object()
        .ok_or_else(|| AppError::Pricing("litellm body is not a JSON object".into()))?;
    let mut out = Vec::new();
    for (key, val) in obj {
        // Skip the doc's template pseudo-entry and anything without costs.
        if key.eq_ignore_ascii_case("sample_provider/claude-3-5-sonnet") || key.contains("sample") {
            continue;
        }
        let o = match val.as_object() {
            Some(o) => o,
            None => continue,
        };
        let get_f = |k: &str| o.get(k).and_then(|x| x.as_f64());
        let Some(input) = get_f("input_cost_per_token") else {
            continue;
        };
        // Drop free / placeholder entries (0 input cost ⇒ not a real price).
        if input <= 0.0 {
            continue;
        }
        let output = get_f("output_cost_per_token").unwrap_or(0.0);
        let cache_creation = get_f("cache_creation_input_token_cost").unwrap_or(0.0);
        let cache_read = get_f("cache_read_input_token_cost").unwrap_or(0.0);
        // Key form is often `provider/model`; keep the model tail.
        let model_key = key.rsplit('/').next().unwrap_or(key).to_string();
        out.push(ModelPricing {
            display_name: model_key.clone(),
            model_key,
            input: per_million(input),
            output: per_million(output),
            cache_read: per_million(cache_read),
            cache_creation: per_million(cache_creation),
            is_builtin: true,
        });
    }
    Ok(out)
}

/// Convert a per-token USD cost to a per-1M-token `Decimal`.
fn per_million(per_token: f64) -> Decimal {
    let per_million_f = per_token * 1_000_000.0;
    Decimal::try_from(per_million_f).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_strips_brackets_lowercases_and_drops_date() {
        assert_eq!(normalize_key("glm-5.2[1m]"), "glm-5.2");
        assert_eq!(
            normalize_key("Claude-3-5-Haiku-20241022"),
            "claude-3-5-haiku"
        );
        assert_eq!(normalize_key("GPT-4o"), "gpt-4o");
        // No bracket suffix, no trailing 8-digit date → only lowercased.
        assert_eq!(normalize_key("claude-3-5-sonnet"), "claude-3-5-sonnet");
    }

    #[test]
    fn calc_is_zero_when_rate_missing() {
        let tokens = TokenCounts {
            input: 1000,
            output: 500,
            cache_creation: 0,
            cache_read: 0,
        };
        let cost = CostCalculator::calc(tokens, None);
        assert_eq!(cost.total_f64(), 0.0);
    }

    #[test]
    fn calc_applies_per_million_rates() {
        // input @ 3 USD/1M × 1000 tokens = 0.003 USD.
        let tokens = TokenCounts {
            input: 1000,
            output: 0,
            cache_creation: 0,
            cache_read: 0,
        };
        let rate = ResolvedRate {
            input: dec("3"),
            output: dec("0"),
            cache_read: dec("0"),
            cache_creation: dec("0"),
        };
        let cost = CostCalculator::calc(tokens, Some(rate));
        assert!((cost.total_f64() - 0.003).abs() < 1e-9);
    }

    #[test]
    fn resolve_fills_unset_cache_rates_via_heuristics() {
        let book = PricingBook::from_iter([ModelPricing {
            model_key: "m".into(),
            display_name: "M".into(),
            input: dec("10"),
            output: dec("30"),
            cache_read: Decimal::ZERO,
            cache_creation: Decimal::ZERO,
            is_builtin: true,
        }]);
        let rate = book.resolve("m").expect("seeded model resolves");
        // cache_read ≈ 0.1×input, cache_creation ≈ 1.25×input (ADR-0007).
        assert_eq!(rate.cache_read, dec("1.0"));
        assert_eq!(rate.cache_creation, dec("12.5"));
        assert_eq!(rate.input, dec("10"));
        assert_eq!(rate.output, dec("30"));
    }

    #[test]
    fn resolve_uses_prefix_fallback_and_none_for_unknown() {
        let book = PricingBook::from_iter([ModelPricing {
            model_key: "gpt-4o".into(),
            display_name: "GPT-4o".into(),
            input: dec("2.5"),
            output: dec("10"),
            cache_read: dec("1.25"),
            cache_creation: dec("2.5"),
            is_builtin: true,
        }]);
        assert!(
            book.resolve("gpt-4o-mini").is_some(),
            "longer string falls back to gpt-4o"
        );
        assert!(book.resolve("gpt-4o").is_some());
        assert!(book.resolve("totally-unknown").is_none());
    }

    #[test]
    fn seed_book_resolves_transit_model() {
        let book = seed_book();
        // The CC-Switch transit model carries a [1m] bracket that normalizes away.
        assert!(book.resolve("glm-5.2[1m]").is_some());
    }

    #[test]
    fn pricing_doc_round_trip() {
        let entries: Vec<_> = builtin_seed().into_iter().take(2).collect();
        let doc = write_pricing_doc(&entries).unwrap();
        let parsed = parse_pricing_doc(&doc).unwrap();
        assert_eq!(parsed.len(), entries.len());
        assert_eq!(parsed[0].model_key, entries[0].model_key);
        assert_eq!(parsed[0].is_builtin, entries[0].is_builtin);
    }

    #[test]
    fn parse_litellm_body_skips_sample_and_free_entries() {
        let body = r#"{
            "sample_provider/claude-3-5-sonnet": {"input_cost_per_token": 0.000003},
            "claude-3-5-haiku": {"input_cost_per_token": 0.0000008, "output_cost_per_token": 0.000004},
            "free-model": {"input_cost_per_token": 0}
        }"#;
        let entries = parse_litellm_body(body).unwrap();
        assert_eq!(entries.len(), 1, "sample + free entries must be skipped");
        assert_eq!(entries[0].model_key, "claude-3-5-haiku");
        let per_million_input = entries[0].input.to_f64().unwrap();
        assert!(
            (per_million_input - 0.8).abs() < 1e-6,
            "per-token 0.0000008 → per-million ≈0.8"
        );
    }
}
