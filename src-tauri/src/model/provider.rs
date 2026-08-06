//! Provider (供应商) entity types.
//!
//! One provider is a vendor VaultOne can switch Claude Code to: a
//! `settings.json` snapshot (`settingsConfig`) plus app-side extras (`meta`).
//! The snapshot is the single authority — every form field, preset and snippet
//! reads/writes it — and API keys live inside its `env` block
//! (`ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`). Both `settingsConfig` and
//! `meta` cross the boundary as raw JSON *text*: the store persists them as
//! TEXT as-is, and the future CodeMirror editor edits that text directly, so
//! nothing here parses or prettifies it (that is the frontend `derive.ts`'s
//! job).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Provider category. `Custom` is the value for user-created providers; the
/// rest describe the built-in presets (added by the preset ticket) so the
/// list view can label and theme them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    Official,
    CnOfficial,
    Aggregator,
    CloudProvider,
    Custom,
}

impl ProviderCategory {
    /// The SQLite-stored spelling (also the JSON spelling via `rename_all`).
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProviderCategory::Official => "official",
            ProviderCategory::CnOfficial => "cn_official",
            ProviderCategory::Aggregator => "aggregator",
            ProviderCategory::CloudProvider => "cloud_provider",
            ProviderCategory::Custom => "custom",
        }
    }

    /// Parse the SQLite-stored spelling; anything unrecognised falls back to
    /// `Custom` so an unknown value (a typo, a future category) never fails
    /// the whole list read.
    pub(crate) fn from_db_str(s: &str) -> ProviderCategory {
        match s {
            "official" => ProviderCategory::Official,
            "cn_official" => ProviderCategory::CnOfficial,
            "aggregator" => ProviderCategory::Aggregator,
            "cloud_provider" => ProviderCategory::CloudProvider,
            _ => ProviderCategory::Custom,
        }
    }
}

/// A provider (供应商): `settingsConfig` is a Claude Code `settings.json`
/// snapshot (raw JSON text); `meta` carries app-side info the live file never
/// sees. `sortIndex` is the user-ordered display rank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub website_url: String,
    pub category: ProviderCategory,
    pub icon: String,
    pub icon_color: String,
    pub sort_index: u32,
    pub notes: String,
    /// Claude Code `settings.json` snapshot, raw JSON text.
    pub settings_config: String,
    /// App-side extras, raw JSON text. Never written to the live file.
    pub meta: String,
    pub updated_at: String,
}

/// A short random id for a user-created provider (8 lowercase hex chars, the
/// same shape as `sessions::generate_local_group_id` — each module owns its own
/// id space so a prefix is unnecessary).
pub(crate) fn generate_provider_id() -> String {
    use rand::Rng;
    let bytes: [u8; 4] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_db_str_roundtrips() {
        for c in [
            ProviderCategory::Official,
            ProviderCategory::CnOfficial,
            ProviderCategory::Aggregator,
            ProviderCategory::CloudProvider,
            ProviderCategory::Custom,
        ] {
            assert_eq!(ProviderCategory::from_db_str(c.as_str()), c);
        }
    }

    #[test]
    fn category_unknown_db_str_falls_back_to_custom() {
        assert_eq!(
            ProviderCategory::from_db_str("bogus"),
            ProviderCategory::Custom
        );
    }

    #[test]
    fn provider_serializes_camel_case() {
        let p = Provider {
            id: "p1".into(),
            name: "Kimi".into(),
            website_url: "https://platform.kimi.com".into(),
            category: ProviderCategory::CnOfficial,
            icon: "kimi".into(),
            icon_color: "#6366F1".into(),
            sort_index: 0,
            notes: String::new(),
            settings_config: r#"{"env":{}}"#.into(),
            meta: r#"{}"#.into(),
            updated_at: "2026-08-07T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"websiteUrl\""));
        assert!(json.contains("\"sortIndex\""));
        assert!(json.contains("\"settingsConfig\""));
        assert!(json.contains("\"cn_official\""));
        // The raw JSON text fields stay raw — the value is escaped (inner
        // quotes → `\"`) but never re-parsed or prettified.
        assert!(json.contains(r#""settingsConfig":"{\"env\":{}}""#));
    }

    #[test]
    fn provider_id_is_eight_hex_chars() {
        let id = generate_provider_id();
        assert_eq!(id.len(), 8);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
