//! Local data layout + config (ADR-0002 / 0004 / 0006).
//!
//! Everything lives under `~/.config/vaultone/` (even on Windows:
//! `C:\Users\<user>\.config\vaultone\`, CodeBurn-style, ADR-0004). The local
//! `config.json` (token / deviceId / repo URL / display-name map) never enters
//! the repo. First start defaults to Standalone (ADR-0006).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::Rng;

use crate::error::{AppError, AppResult};
use crate::model::RunMode;

/// Root of all VaultOne local data: `~/.config/vaultone`.
pub fn root_dir() -> AppResult<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| AppError::Config("cannot resolve home dir".into()))?;
    Ok(home.join(".config").join("vaultone"))
}

/// All well-known paths under the root (ADR-0004 layout).
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub config_json: PathBuf,
    pub db: PathBuf,
    pub repo: PathBuf,
    pub repo_config: PathBuf,
    pub repo_data: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    /// Resolve all paths from the root (does not create anything).
    pub fn resolve(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            config_json: root.join("config.json"),
            db: root.join("vaultone.db"),
            repo: root.join("repo"),
            repo_config: root.join("repo").join("config"),
            repo_data: root.join("repo").join("data"),
            logs: root.join("logs"),
        }
    }

    /// Per-device Artifact directory: `repo/data/<deviceId>/`.
    pub fn device_data_dir(&self, device_id: &str) -> PathBuf {
        self.repo_data.join(device_id)
    }

    /// JSONL Artifact path for a given day: `repo/data/<deviceId>/usage-<day>.jsonl`.
    pub fn artifact_path(&self, device_id: &str, day: &str) -> PathBuf {
        self.device_data_dir(device_id)
            .join(format!("usage-{day}.jsonl"))
    }

    /// Cloud pricing config: `repo/config/pricing.json` (ADR-0007).
    pub fn pricing_json(&self) -> PathBuf {
        self.repo_config.join("pricing.json")
    }
}

/// Window-close behavior preference (ADR-0012). Crosses the Rust→JS boundary.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Show the minimize/quit dialog each time (default).
    #[default]
    Ask,
    /// Always minimize to tray — keeps the background scheduler alive.
    Minimize,
    /// Always quit.
    Quit,
}

/// How the lightweight glance card's tucked half-icon expands (ADR-0015).
/// Crosses the Rust→JS boundary; Rust itself doesn't act on it (a pure frontend
/// interaction), but it rides `ConfigData` so every Settings preference lives in
/// one place.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum LightweightExpand {
    /// Click the half-icon to expand (default — won't fire on a stray hover).
    #[default]
    Click,
    /// Hover the half-icon to expand.
    Hover,
}

/// Color skin for multi-skin theming (ADR-0013 token-first). Serialized
/// snake_case; `neutral` is the default and maps to NO `data-skin` attribute on
/// `<html>` (the :root/.dark values in src/index.css ARE the Neutral palette —
/// pure greyscale chrome over a default multi-hue chart). Per-device, not synced
/// (config.json never enters the repo). The four chromatic skins each override
/// `--brand` (+ `--brand-strong`) and the button-foreground vars in index.css;
/// everything else holds. The frontend applies it; Rust only stores it.
///
/// Back-compat: the legacy snake_case names (`pixso`/`cuiwei`/`tingwu`/
/// `yanzhi`/`zizi`) are accepted as aliases, so an older config.json lands on
/// the closest new skin instead of failing to deserialize — `pixso` (the old
/// default) → `Neutral` (the new default); the rest map by hue family.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Skin {
    #[default]
    #[serde(alias = "pixso")]
    Neutral,
    #[serde(alias = "cuiwei")]
    Sage,
    #[serde(alias = "tingwu")]
    Azure,
    #[serde(alias = "yanzhi")]
    Crimson,
    #[serde(alias = "zizi")]
    Mauve,
}

/// Default background-collect interval in seconds (ADR-0014: 30 s — decoupled
/// from the push cadence, which has its own interval).
///
/// `u32` (not `u64`): the value crosses the Rust→JS boundary via the typed
/// specta contract, and specta forbids exporting BigInt-style types (`u64`,
/// `i64`, …) to avoid JS precision loss. `u32`'s range (≈4.29e9 s) is ample
/// for an interval clamped to [10, 3600].
fn default_collect_interval_secs() -> u32 {
    30
}

/// Default push-to-sync interval in seconds (ADR-0014: 10 min). Decoupled from
/// collect so a short collect cadence does not bloat the Git history.
fn default_push_interval_secs() -> u32 {
    600
}

/// Display language (ADR-0016). Serialized lowercase (`en`/`zh`/`ja`), matching
/// the frontend locale codes. The tray "Quit" item — the only user-facing Rust
/// string — is localized from this; all other UI text is frontend i18n.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Zh,
    Ja,
}

/// The local `config.json` content (ADR-0004). Never uploaded to the repo.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigData {
    pub device_id: String,
    /// Friendly name for *this* device (ADR-0002: display name, not a key).
    pub display_name: String,
    /// Sync repo URL; `None` ⇒ Standalone (ADR-0006).
    pub repo_url: Option<String>,
    /// Fine-grained PAT (ADR-0004); kept only in local config + Rust memory.
    #[serde(default)]
    pub github_token: Option<String>,
    /// `deviceId → friendly name` for other devices seen in the repo.
    #[serde(default)]
    pub device_names: BTreeMap<String, String>,
    /// Optional: GitHub handle resolved from the token (for display only).
    #[serde(default)]
    pub github_user: Option<String>,
    /// Window-close behavior (ADR-0012). `Ask` ⇒ show the minimize/quit dialog.
    #[serde(default)]
    pub close_behavior: CloseBehavior,
    /// Background collect interval in seconds (ADR-0014). Clamped to [10, 3600]
    /// at use; serialized verbatim so the UI shows what the user typed.
    #[serde(default = "default_collect_interval_secs")]
    pub collect_interval_secs: u32,
    /// Push-to-sync interval in seconds (ADR-0014). Synced only; clamped to
    /// [60, 7200] at use. Decoupled from collect so the Git push cadence stays
    /// independent of the (shorter) collect cadence.
    #[serde(default = "default_push_interval_secs")]
    pub push_interval_secs: u32,
    /// Display language (ADR-0016). Default English; per-device, not synced
    /// (config.json never enters the repo).
    #[serde(default)]
    pub language: Language,
    /// How the lightweight half-icon expands (ADR-0015). Frontend-only behavior;
    /// Rust doesn't read it, but it lives here so all Settings prefs are unified.
    #[serde(default)]
    pub lightweight_expand: LightweightExpand,
    /// Color skin (multi-skin theming). Frontend-only effect; Rust doesn't act
    /// on it, but it rides ConfigData so every Settings preference is unified.
    #[serde(default)]
    pub skin: Skin,
}

impl Default for ConfigData {
    fn default() -> Self {
        // A real deviceId is generated on first start (see `ensure_config`);
        // this default is only a fallback if config.json lacks the field.
        Self {
            device_id: String::new(),
            display_name: "VaultOne".to_string(),
            repo_url: None,
            github_token: None,
            device_names: BTreeMap::new(),
            github_user: None,
            close_behavior: CloseBehavior::Ask,
            collect_interval_secs: default_collect_interval_secs(),
            push_interval_secs: default_push_interval_secs(),
            language: Language::En,
            lightweight_expand: LightweightExpand::Click,
            skin: Skin::Neutral,
        }
    }
}

impl ConfigData {
    /// Synced iff a repo URL *and* a token are configured (ADR-0006).
    pub fn mode(&self) -> RunMode {
        match self.repo_url.as_deref().zip(self.github_token.as_deref()) {
            Some((url, token)) if !url.trim().is_empty() && !token.trim().is_empty() => {
                RunMode::Synced
            }
            _ => RunMode::Standalone,
        }
    }

    pub fn is_synced(&self) -> bool {
        self.mode() == RunMode::Synced
    }

    /// Mask the token for any non-storage surface (logs / UI echoes).
    pub fn masked_token(&self) -> Option<String> {
        self.github_token.as_ref().map(|t| {
            let len = t.chars().count();
            if len <= 8 {
                "****".to_string()
            } else {
                let head: String = t.chars().take(4).collect();
                let tail: String = t.chars().skip(len.saturating_sub(4)).collect();
                format!("{head}…{tail}")
            }
        })
    }
}

/// Thread-safe holder for the loaded config + paths, shared via Tauri state.
#[derive(Debug)]
pub struct ConfigStore {
    paths: Paths,
    data: Mutex<ConfigData>,
}

impl ConfigStore {
    /// Load (or bootstrap on first run) config + ensure the full directory
    /// layout exists (ADR-0004). Idempotent.
    pub fn load() -> AppResult<Self> {
        let root = root_dir()?;
        let paths = Paths::resolve(&root);

        // Full directory layout up front (ADR-0004).
        for dir in [
            &paths.root,
            &paths.repo,
            &paths.repo_config,
            &paths.repo_data,
            &paths.logs,
        ] {
            fs::create_dir_all(dir)?;
        }

        let data = match fs::read(&paths.config_json) {
            Ok(bytes) => serde_json::from_slice::<ConfigData>(&bytes).unwrap_or_else(|e| {
                // Corrupt config shouldn't brick the app; log + fall back, then
                // re-bootstrap a sane deviceId below.
                eprintln!("[vaultone] config.json unreadable, re-bootstrapping: {e}");
                ConfigData::default()
            }),
            Err(_) => ConfigData::default(),
        };

        let mut data = data;
        let mut dirty = false;

        // deviceId first-generation (ADR-0002): persistent 12-hex, collision-checked.
        if data.device_id.is_empty() || !is_valid_device_id(&data.device_id) {
            data.device_id = generate_device_id(&paths);
            if data.display_name.trim().is_empty() || data.display_name == "VaultOne" {
                data.display_name = default_display_name(&data.device_id);
            }
            dirty = true;
        }

        if dirty {
            Self::write_config(&paths, &data)?;
        }

        Ok(Self {
            paths,
            data: Mutex::new(data),
        })
    }

    /// Snapshot the current config.
    pub fn get(&self) -> ConfigData {
        self.data.lock().expect("config mutex poisoned").clone()
    }

    /// Mutate the in-memory config under the lock, persist, and return a copy.
    pub fn update<F>(&self, mutate: F) -> AppResult<ConfigData>
    where
        F: FnOnce(&mut ConfigData),
    {
        let mut data = self.data.lock().expect("config mutex poisoned");
        mutate(&mut data);
        Self::write_config(&self.paths, &data)?;
        Ok(data.clone())
    }

    /// Read-only path accessors.
    pub fn paths(&self) -> Paths {
        self.paths.clone()
    }

    fn write_config(paths: &Paths, data: &ConfigData) -> AppResult<()> {
        let bytes = serde_json::to_vec_pretty(data)?;
        fs::write(&paths.config_json, bytes)?;
        Ok(())
    }
}

/// A valid deviceId is 12 lowercase hex chars (ADR-0002: 48-bit short id).
pub fn is_valid_device_id(id: &str) -> bool {
    id.len() == 12
        && id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && (!c.is_ascii_alphabetic() || c.is_ascii_lowercase()))
}

/// Generate a 12-hex deviceId (48 bits), retrying if it collides with an
/// existing device dir in `repo/data/` (ADR-0002: collision check).
fn generate_device_id(paths: &Paths) -> String {
    let existing = list_existing_device_ids(&paths.repo_data);
    let mut rng = rand::thread_rng();
    for _ in 0..8 {
        let bytes: [u8; 6] = rng.gen();
        let id = hex_encode(&bytes);
        if !existing.iter().any(|e| e == &id) {
            return id;
        }
    }
    // Astronomically unlikely (8 × 2^-48); fall through with the last candidate.
    let bytes: [u8; 6] = rng.gen();
    hex_encode(&bytes)
}

fn list_existing_device_ids(repo_data: &Path) -> Vec<String> {
    match fs::read_dir(repo_data) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|s| is_valid_device_id(s))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn default_display_name(device_id: &str) -> String {
    let prefix = &device_id[..6.min(device_id.len())];
    format!("Device-{prefix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_device_id_rules() {
        assert!(is_valid_device_id("0123456789ab"));
        assert!(is_valid_device_id("abcdef012345"));
        assert!(!is_valid_device_id("0123456789a")); // too short
        assert!(!is_valid_device_id("0123456789abc")); // too long
        assert!(!is_valid_device_id("abcdef01234g")); // non-hex letter
        assert!(!is_valid_device_id("ABCDEF012345")); // uppercase rejected
    }

    #[test]
    fn generated_device_id_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let id = generate_device_id(&paths);
        assert!(is_valid_device_id(&id));
    }

    #[test]
    fn generated_device_id_avoids_existing_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Pre-seed an existing device dir under repo/data/.
        fs::create_dir_all(paths.device_data_dir("aabbccddeeff")).unwrap();
        for _ in 0..16 {
            let id = generate_device_id(&paths);
            assert_ne!(
                id, "aabbccddeeff",
                "generator must avoid existing device dirs"
            );
            assert!(is_valid_device_id(&id));
        }
    }

    #[test]
    fn artifact_path_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let p = paths.artifact_path("0123456789ab", "2026-07-13");
        let s = p.to_string_lossy().into_owned();
        assert!(s.contains("0123456789ab"), "{s}");
        assert!(s.ends_with("usage-2026-07-13.jsonl"), "{s}");
    }

    #[test]
    fn mode_requires_both_repo_url_and_token() {
        let mut c = ConfigData::default();
        assert_eq!(c.mode(), RunMode::Standalone);
        c.repo_url = Some("https://github.com/x/y".into());
        assert_eq!(c.mode(), RunMode::Standalone, "token still missing");
        c.github_token = Some("ghp_token".into());
        assert_eq!(c.mode(), RunMode::Synced);
        c.github_token = Some("   ".into());
        assert_eq!(c.mode(), RunMode::Standalone, "blank token ⇒ standalone");
    }

    #[test]
    fn masked_token_redacts() {
        let mut c = ConfigData::default();
        assert_eq!(c.masked_token(), None);
        c.github_token = Some("short".into());
        assert_eq!(c.masked_token().as_deref(), Some("****"));
        c.github_token = Some("ghp_abcdefghijklmnop".into());
        assert_eq!(c.masked_token().as_deref(), Some("ghp_…mnop"));
    }
}
