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

use crate::error::{AppError, AppResult};

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

/// Secret env-var keys stripped from `settingsConfig` before it leaves this
/// device (the synced `providers.json`): API keys live in the `env` block and
/// must never enter the repo. `AWS_REGION` is deliberately NOT here — it is a
/// non-secret region code (or a `${VAR}` template-variable placeholder), not a
/// credential. This list is the single source of truth: `Provider::redacted`,
/// the sync merge, and the export path (`provider::export_import`) all route
/// through it.
pub const SECRET_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_ACCESS_KEY_ID",
];

impl Provider {
    /// The sync-safe projection: `settingsConfig` with [`SECRET_ENV_KEYS`]
    /// removed from its `env` object. Blank config passes through unchanged
    /// (nothing to strip); config with a secret key is re-serialized
    /// deterministically (serde_json's default `Value` map sorts keys), so the
    /// written file is byte-stable across pushes. Returns `Err` when the
    /// config is not valid JSON / not an object / has a non-object `env` — a
    /// provider whose secrets cannot be proven absent must not be published
    /// (the sync writer skips it).
    pub fn redacted(&self) -> AppResult<Provider> {
        let trimmed = self.settings_config.trim();
        if trimmed.is_empty() {
            return Ok(self.clone());
        }
        let mut v: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
            AppError::Config(format!("provider settingsConfig is not valid JSON: {e}"))
        })?;
        let obj = v.as_object_mut().ok_or_else(|| {
            AppError::Config("provider settingsConfig is not a JSON object".into())
        })?;
        let Some(env) = obj.get_mut("env") else {
            // No env block ⇒ no key location ⇒ nothing to strip.
            return Ok(self.clone());
        };
        let env = env.as_object_mut().ok_or_else(|| {
            AppError::Config("provider settingsConfig env is not a JSON object".into())
        })?;
        let mut stripped = false;
        for key in SECRET_ENV_KEYS {
            if env.remove(*key).is_some() {
                stripped = true;
            }
        }
        if !stripped {
            return Ok(self.clone());
        }
        let mut p = self.clone();
        p.settings_config = serde_json::to_string_pretty(&v)?;
        Ok(p)
    }

    /// True iff two rows carry identical syncable structure: every field that
    /// syncs — including the key-stripped `settingsConfig` — except
    /// `sort_index` (never set through save) and `updated_at` (the computed
    /// freshness). Secret keys don't count (stripped before compare), so a
    /// key-only edit compares equal. A provider whose config cannot be parsed
    /// never compares equal — treat that as a structural change, never assume.
    pub fn structure_equals(&self, other: &Provider) -> bool {
        if self.id != other.id
            || self.name != other.name
            || self.website_url != other.website_url
            || self.category != other.category
            || self.icon != other.icon
            || self.icon_color != other.icon_color
            || self.notes != other.notes
            || self.meta != other.meta
        {
            return false;
        }
        // The fields above are already compared; the key-stripped config is
        // all that remains. Compare the config strings only — the redacted
        // clones still carry each row's `updated_at`, which must not count.
        match (self.redacted(), other.redacted()) {
            (Ok(a), Ok(b)) => a.settings_config == b.settings_config,
            _ => false,
        }
    }
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

    /// A provider whose env carries all four secret keys plus a region, a
    /// base URL and a model.
    fn keyed_provider() -> Provider {
        Provider {
            id: "p1".into(),
            name: "Bedrock".into(),
            website_url: "https://bedrock.aws".into(),
            category: ProviderCategory::CloudProvider,
            icon: "bedrock".into(),
            icon_color: "#ff0".into(),
            sort_index: 2,
            notes: "n".into(),
            settings_config: r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.bedrock","ANTHROPIC_AUTH_TOKEN":"sk-token","ANTHROPIC_API_KEY":"sk-key","AWS_ACCESS_KEY_ID":"AKIA123","AWS_SECRET_ACCESS_KEY":"top-secret","AWS_REGION":"us-east-1","ANTHROPIC_MODEL":"claude-sonnet"},"includeCoAuthoredBy":false}"#.into(),
            meta: r#"{"auth_field":"aws"}"#.into(),
            updated_at: "2026-08-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn redacted_strips_secret_keys_and_keeps_region_and_structure() {
        let p = keyed_provider();
        let r = p.redacted().unwrap();
        let v: serde_json::Value = serde_json::from_str(&r.settings_config).unwrap();
        let env = &v["env"];
        for key in SECRET_ENV_KEYS {
            assert!(env.get(*key).is_none(), "{key} must be stripped");
        }
        // Non-secret env entries survive; AWS_REGION is a region/template
        // placeholder, not a credential.
        assert_eq!(env["AWS_REGION"], serde_json::json!("us-east-1"));
        assert_eq!(
            env["ANTHROPIC_BASE_URL"],
            serde_json::json!("https://api.bedrock")
        );
        assert_eq!(env["ANTHROPIC_MODEL"], serde_json::json!("claude-sonnet"));
        assert_eq!(v["includeCoAuthoredBy"], serde_json::json!(false));
        // The rest of the row is untouched.
        assert_eq!(r.id, p.id);
        assert_eq!(r.name, p.name);
        assert_eq!(r.sort_index, p.sort_index);
        assert_eq!(r.updated_at, p.updated_at);
        assert_eq!(r.meta, p.meta);
        // Redaction is idempotent and byte-stable.
        assert_eq!(
            r.settings_config,
            r.redacted().unwrap().settings_config,
            "redacting twice must not churn the bytes"
        );
        // The secret key names never appear anywhere in the projection.
        for key in SECRET_ENV_KEYS {
            assert!(!r.settings_config.contains(key));
        }
    }

    #[test]
    fn redacted_passes_through_blank_config_and_config_without_secrets() {
        let mut blank = keyed_provider();
        blank.settings_config = "  ".into();
        assert_eq!(blank.redacted().unwrap().settings_config, "  ");

        let mut plain = keyed_provider();
        plain.settings_config =
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://x.dev"},"includeCoAuthoredBy":false}"#.into();
        let r = plain.redacted().unwrap();
        // Nothing was stripped ⇒ the authored text is kept verbatim.
        assert_eq!(r.settings_config, plain.settings_config);

        // No env block at all ⇒ nothing to strip.
        let mut no_env = keyed_provider();
        no_env.settings_config = r#"{"includeCoAuthoredBy":false}"#.into();
        assert_eq!(
            no_env.redacted().unwrap().settings_config,
            no_env.settings_config
        );
    }

    #[test]
    fn redacted_rejects_unparseable_or_non_object_config() {
        let mut bad = keyed_provider();
        bad.settings_config = "{oops".into();
        assert!(bad.redacted().is_err(), "invalid JSON must error");
        bad.settings_config = r#"[1,2]"#.into();
        assert!(bad.redacted().is_err(), "non-object must error");
        bad.settings_config = r#"{"env":"nope"}"#.into();
        assert!(bad.redacted().is_err(), "non-object env must error");
    }

    #[test]
    fn structure_equals_ignores_keys_and_freshness_but_not_other_fields() {
        let base = keyed_provider();

        // A key-only edit compares equal (structure unchanged).
        let mut keyed = base.clone();
        keyed.settings_config = r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.bedrock","ANTHROPIC_AUTH_TOKEN":"sk-NEW-token","AWS_REGION":"us-east-1","ANTHROPIC_MODEL":"claude-sonnet"},"includeCoAuthoredBy":false}"#.into();
        keyed.updated_at = "2026-08-02T00:00:00.000Z".into();
        assert!(base.structure_equals(&keyed), "key edit is not structural");

        // A structural edit (name, endpoint, model…) differs.
        let mut renamed = base.clone();
        renamed.name = "Bedrock Pro".into();
        assert!(!base.structure_equals(&renamed));
        let mut moved = base.clone();
        moved.settings_config = r#"{"env":{"ANTHROPIC_BASE_URL":"https://other.dev","ANTHROPIC_AUTH_TOKEN":"sk-token"}}"#.into();
        assert!(!base.structure_equals(&moved));

        // An unparseable config never compares equal.
        let mut broken = base.clone();
        broken.settings_config = "{oops".into();
        assert!(!base.structure_equals(&broken));
    }
}
