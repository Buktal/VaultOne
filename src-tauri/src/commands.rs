//! Tauri command layer (typed contract — the query boundary).
//!
//! Every command is `#[specta::specta]` with typed args/return/error; tauri-specta
//! generates the matching typed JS function. `tauri::State` args are injected by
//! the runtime and excluded from the JS signature. JS never sees SQL.
//!
//! The state holds `Arc`s so blocking work can be moved onto `spawn_blocking`
//! without borrowing the request-scoped `State` (which is not `'static`).

use std::sync::Arc;

use tauri::{Emitter, Manager, State};

use crate::collect::AlignReport;
use crate::config::{CloseBehavior, ConfigStore, Language, LightweightExpand, Skin};
use crate::db::Store;
use crate::error::{AppError, AppResult};
use crate::model::{
    DeviceInfo, LogsQuery, ModelStatsRow, PricingEntry, RunMode, TrendBucket, TrendPoint,
    UsageFilter, UsageLogRow, UsageStats,
};
use crate::pricing;
use crate::sync::VerifyReport;

/// App-wide managed state: the Local Store + local config, wrapped
/// in `Arc` so blocking tasks can take owned clones.
pub struct AppState {
    pub store: Arc<Store>,
    pub config: Arc<ConfigStore>,
}

/// Snapshot of app/status info for the UI on startup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppInfo {
    pub device_id: String,
    pub display_name: String,
    pub mode: RunMode,
    pub repo_url: Option<String>,
    pub masked_token: Option<String>,
    pub github_user: Option<String>,
    pub claude_projects_dir: Option<String>,
    pub version: String,
}

// ---------------- App info / config ----------------

/// App status: device, mode (Standalone/Synced), paths, version.
#[tauri::command]
#[specta::specta]
pub fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let cfg = state.config.get();
    let claude_dir = crate::providers::default_projects_dir().map(|p| p.display().to_string());
    Ok(AppInfo {
        device_id: cfg.device_id.clone(),
        display_name: cfg.display_name.clone(),
        mode: cfg.mode(),
        repo_url: cfg.repo_url.clone(),
        masked_token: cfg.masked_token(),
        github_user: cfg.github_user.clone(),
        claude_projects_dir: claude_dir,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Configure the sync repo + PAT, upgrading Standalone → Synced, then
/// immediately pull the remote so peer devices show up without a restart (the
/// startup pull only fires on next launch). Best-effort: a pull failure doesn't
/// undo the bind — the next startup pull retries.
#[tauri::command]
#[specta::specta]
pub async fn set_sync_repo(
    state: State<'_, AppState>,
    repo_url: String,
    github_token: String,
) -> AppResult<RunMode> {
    let config = state.config.clone();
    let store = state.store.clone();
    let mode = tauri::async_runtime::spawn_blocking(move || -> AppResult<RunMode> {
        let cfg = config.update(|c| {
            c.repo_url = if repo_url.trim().is_empty() {
                None
            } else {
                Some(repo_url.trim().to_string())
            };
            c.github_token = if github_token.trim().is_empty() {
                None
            } else {
                Some(github_token.trim().to_string())
            };
        })?;
        if cfg.is_synced() {
            let paths = config.paths();
            match crate::sync::pull_and_import(&store, &paths, &cfg) {
                Ok(n) => eprintln!("[vaultone] set_sync_repo imported {n} row(s)"),
                Err(e) => eprintln!("[vaultone] set_sync_repo pull failed: {e}"),
            }
        }
        Ok(cfg.mode())
    })
    .await
    .map_err(|e| AppError::Internal(format!("set_sync_repo task failed: {e}")))??;
    Ok(mode)
}

/// Unbind the repo, downgrading to Standalone. Clears the local
/// `.git` so a re-bind (often to a different repo) starts clean instead of
/// reusing the old remote/branch. Usage rows (DB) and `data/` are retained.
#[tauri::command]
#[specta::specta]
pub fn clear_sync_repo(state: State<'_, AppState>) -> AppResult<RunMode> {
    let cfg = state.config.update(|c| {
        c.repo_url = None;
        c.github_token = None;
    })?;
    let paths = state.config.paths();
    crate::sync::reset_local_git(&paths.repo);
    Ok(cfg.mode())
}

/// Probe a sync repo + PAT for reachability (「测试连接」). Pass explicit
/// values to validate BEFORE binding, or `None`/`None` to re-check the already-
/// configured repo. Pure ls-remote — never mutates config or the real sync repo.
/// Always returns `Ok(report)`; the probe's own outcome (auth ok / bad token /
/// not found) lives in `report.ok`, so the frontend never throws on a failed
/// probe (only a `spawn_blocking` join failure surfaces as an `AppError`).
#[tauri::command]
#[specta::specta]
pub async fn verify_sync_repo(
    state: State<'_, AppState>,
    repo_url: Option<String>,
    github_token: Option<String>,
) -> AppResult<VerifyReport> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<VerifyReport> {
        let cfg = config.get();
        let report = match (repo_url, github_token) {
            // Validate an as-yet-unbound pair straight from the Settings inputs.
            (Some(url), Some(tok)) => crate::sync::verify_remote(&url, &tok),
            // Re-check the configured repo: the raw PAT never crosses to JS, so
            // the masked_token the UI shows can't drive a re-probe — read the
            // real token server-side from config.
            (None, None) => match (cfg.repo_url.as_deref(), cfg.github_token.as_deref()) {
                (Some(url), Some(tok)) => crate::sync::verify_remote(url, tok),
                _ => crate::sync::verify_remote("", ""),
            },
            // One field present, the other absent: surface as an input error.
            _ => crate::sync::verify_remote("", ""),
        };
        Ok(report)
    })
    .await
    .map_err(|e| AppError::Internal(format!("verify task failed: {e}")))?
}

/// Rename *this* device (display name only — not a uniqueness key).
#[tauri::command]
#[specta::specta]
pub fn set_display_name(state: State<'_, AppState>, display_name: String) -> AppResult<()> {
    let cfg = state.config.update(|c| {
        c.display_name = display_name;
    })?;
    state
        .store
        .upsert_device(&cfg.device_id, &cfg.display_name, true)?;
    // Publish the new name to the cloud registry (config/devices/<id>.json);
    // the normal Git sync carries it. Best-effort — a write failure doesn't
    // undo the local rename. No write if the file is already current.
    let _ = crate::devices::ensure_own_device_artifact(
        &state.config.paths(),
        &cfg.device_id,
        &cfg.display_name,
    );
    Ok(())
}

/// Set a friendly name for *another* device seen in the repo (map).
#[tauri::command]
#[specta::specta]
pub fn set_device_display_name(
    state: State<'_, AppState>,
    device_id: String,
    display_name: String,
) -> AppResult<()> {
    let is_self = state.config.get().device_id == device_id;
    state
        .store
        .upsert_device(&device_id, &display_name, is_self)?;
    state.config.update(|c| {
        c.device_names.insert(device_id, display_name);
    })?;
    Ok(())
}

/// Locally forget a peer device: drop its registry row + all its local usage
/// data (records, rollups, turn durations, ledger) + its local artifact dir,
/// and clear any local alias. `library_action` decides the fate of the peer's
/// library subtree (`repo/library/<id>/`): migrated into this device's library
/// or deleted. Nothing is pushed to Git — a peer still in the repo reappears on
/// the next sync (registry + data artifacts are re-imported). This device
/// (`is_self`) is not forgettable; rename it instead.
#[tauri::command]
#[specta::specta]
pub fn forget_device(
    state: State<'_, AppState>,
    device_id: String,
    library_action: crate::library::LibraryForgetAction,
) -> AppResult<()> {
    let cfg = state.config.get();
    if cfg.device_id == device_id {
        return Err(AppError::Config(
            "this device cannot be removed (rename it instead)".into(),
        ));
    }
    // Capture the peer's alias BEFORE the registry row + alias map are dropped —
    // the migrate target folder is named after it (`from-<name>`).
    let peer_name = cfg
        .device_names
        .get(&device_id)
        .cloned()
        .unwrap_or_default();
    state.store.forget_device_local(&device_id)?;
    state.config.update(|c| {
        c.device_names.remove(&device_id);
    })?;
    let paths = state.config.paths();
    // Per-device JSONL artifact dir.
    let data_dir = paths.device_data_dir(&device_id);
    if data_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&data_dir) {
            eprintln!(
                "[vaultone] forget_device: failed to remove {}: {e}",
                data_dir.display()
            );
        }
    }
    // Per-device library subtree (migrate or delete), mirroring the data-dir
    // cleanup above: local-only, no Git push.
    if let Err(e) =
        crate::library::forget_device_library(&paths, &cfg, &device_id, library_action, &peer_name)
    {
        eprintln!(
            "[vaultone] forget_device: library {:?} failed: {e}",
            library_action
        );
    }
    // Cloud device-name registry file this peer published.
    let devices_file = paths.devices_file_path(&device_id);
    if devices_file.exists() {
        if let Err(e) = std::fs::remove_file(&devices_file) {
            eprintln!(
                "[vaultone] forget_device: failed to remove {}: {e}",
                devices_file.display()
            );
        }
    }
    Ok(())
}

// ---------------- Collect / ingest ----------------
// The ingest path (`collect_into`) and the manual orchestrators (`align`,
// `sync_round`) live in `collect`; the items here are the typed Tauri commands
// that drive them.

/// Manual「采集 / 同步」: collect now, then (Synced only) pull + push with a
/// bounded retry. The dashboard button's single action — Standalone ⇒ collect;
/// Synced ⇒ collect + sync. The run mode decides what it means, not the UI.
/// Heavy disk/git work → offloaded to a thread.
#[tauri::command]
#[specta::specta]
pub async fn collect_now(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> AppResult<AlignReport> {
    let store = state.store.clone();
    let config = state.config.clone();
    let report =
        tauri::async_runtime::spawn_blocking(move || crate::collect::align(&store, &config))
            .await
            .map_err(|e| AppError::Internal(format!("collect task failed: {e}")))?;
    // Notify the UI that usage data changed (event-driven refresh).
    let _ = app_handle.emit("usage_changed", ());
    Ok(report)
}

/// Manual「立即同步」: the Settings entry — same `align` as the dashboard button
/// (collect + sync). Kept as a distinct command so the Settings card has its
/// own trigger next to the repo binding, but the work is identical. Standalone
/// ⇒ collect only (sync degrades to a local refresh).
#[tauri::command]
#[specta::specta]
pub async fn sync_now(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> AppResult<AlignReport> {
    let store = state.store.clone();
    let config = state.config.clone();
    let report =
        tauri::async_runtime::spawn_blocking(move || crate::collect::align(&store, &config))
            .await
            .map_err(|e| AppError::Internal(format!("sync task failed: {e}")))?;
    let _ = app_handle.emit("usage_changed", ());
    Ok(report)
}

/// Rebill zero-cost rows whose model now has a price (top-up).
#[tauri::command]
#[specta::specta]
pub fn rebill_zero_cost(state: State<'_, AppState>) -> AppResult<u32> {
    let book = state.store.load_pricing_book()?;
    Ok(state.store.rebill_zero_cost(&book)? as u32)
}

// ---------------- Dashboard reads ----------------

#[tauri::command]
#[specta::specta]
pub fn query_usage_stats(state: State<'_, AppState>, filter: UsageFilter) -> AppResult<UsageStats> {
    state.store.query_stats(&filter)
}

#[tauri::command]
#[specta::specta]
pub fn query_usage_trend(
    state: State<'_, AppState>,
    filter: UsageFilter,
    bucket: TrendBucket,
) -> AppResult<Vec<TrendPoint>> {
    state.store.query_trend(&filter, bucket)
}

#[tauri::command]
#[specta::specta]
pub fn query_usage_logs(
    state: State<'_, AppState>,
    query: LogsQuery,
) -> AppResult<Vec<UsageLogRow>> {
    state.store.query_logs(&query)
}

#[tauri::command]
#[specta::specta]
pub fn count_usage_logs(state: State<'_, AppState>, filter: UsageFilter) -> AppResult<u32> {
    state.store.count_logs(&filter)
}

#[tauri::command]
#[specta::specta]
pub fn query_models(
    state: State<'_, AppState>,
    filter: UsageFilter,
) -> AppResult<Vec<ModelStatsRow>> {
    state.store.query_models(&filter)
}

#[tauri::command]
#[specta::specta]
pub fn query_distinct_sources(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.store.query_distinct("source")
}

#[tauri::command]
#[specta::specta]
pub fn query_distinct_models(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.store.query_distinct("model")
}

#[tauri::command]
#[specta::specta]
pub fn list_devices(state: State<'_, AppState>) -> AppResult<Vec<DeviceInfo>> {
    let mut devices = state.store.list_devices()?;
    let cfg = state.config.get();
    crate::devices::apply_aliases(&mut devices, &cfg);
    // NOTE: duplicate display names are no longer disambiguated with an id
    // prefix — the picker shows the raw name and truncates with an ellipsis if
    // it overflows. Users tell peers apart by renaming them in Settings.
    Ok(devices)
}

// ---------------- Pricing ----------------

#[tauri::command]
#[specta::specta]
pub fn list_pricing(state: State<'_, AppState>) -> AppResult<Vec<PricingEntry>> {
    state.store.list_pricing()
}

/// Add or update a pricing entry from the UI (user edits ⇒ `is_builtin=false`).
#[tauri::command]
#[specta::specta]
pub fn save_pricing_entry(
    state: State<'_, AppState>,
    entry: PricingEntry,
    is_builtin: Option<bool>,
) -> AppResult<()> {
    let mut entry = entry;
    entry.is_builtin = is_builtin.unwrap_or(false);
    state.store.upsert_pricing(&entry)
}

#[tauri::command]
#[specta::specta]
pub fn delete_pricing_entry(state: State<'_, AppState>, model_key: String) -> AppResult<()> {
    state.store.delete_pricing(&model_key)
}

/// Re-load pricing from the local `pricing.json` into the DB. Pricing is
/// per-device local (never synced); this is an import surface, not a sync path.
#[tauri::command]
#[specta::specta]
pub fn reload_pricing_from_file(state: State<'_, AppState>) -> AppResult<u32> {
    let path = state.config.paths().pricing_json();
    if !path.exists() {
        return Err(AppError::Pricing(format!(
            "pricing.json not found at {}",
            path.display()
        )));
    }
    state.store.reload_pricing_from_path(&path)
}

/// Persist current DB pricing to the local `pricing.json` (never synced).
#[tauri::command]
#[specta::specta]
pub fn save_pricing_to_file(state: State<'_, AppState>) -> AppResult<()> {
    let entries = state.store.load_pricing_models()?;
    let doc = pricing::write_pricing_doc(&entries)?;
    let path = state.config.paths().pricing_json();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc)?;
    Ok(())
}

/// Fetch LiteLLM upstream pricing and merge into the DB (seed).
/// Network → async + offloaded. Best-effort: returns count merged (0 offline).
#[tauri::command]
#[specta::specta]
pub async fn fetch_litellm_pricing(state: State<'_, AppState>) -> AppResult<u32> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<u32> {
        let entries = crate::pricing::fetch_litellm()?;
        let mut merged = 0u32;
        for e in &entries {
            store.upsert_pricing(&e.to_entry())?;
            merged += 1;
        }
        Ok(merged)
    })
    .await
    .map_err(|e| AppError::Pricing(format!("litellm task failed: {e}")))?
}

// ---------------- Preferences (tray + background) ----------------

/// User-tunable preferences surfaced in the Settings「通用」card.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Preferences {
    pub close_behavior: CloseBehavior,
    pub collect_interval_secs: u32,
    pub push_interval_secs: u32,
    pub language: Language,
    pub lightweight_expand: LightweightExpand,
    pub skin: Skin,
}

fn to_preferences(cfg: &crate::config::ConfigData) -> Preferences {
    Preferences {
        close_behavior: cfg.close_behavior,
        collect_interval_secs: cfg.collect_interval_secs,
        push_interval_secs: cfg.push_interval_secs,
        language: cfg.language,
        lightweight_expand: cfg.lightweight_expand,
        skin: cfg.skin,
    }
}

/// Read the current preferences for the Settings card.
#[tauri::command]
#[specta::specta]
pub fn get_preferences(state: State<'_, AppState>) -> AppResult<Preferences> {
    Ok(to_preferences(&state.config.get()))
}

/// Persist the window-close behavior.
#[tauri::command]
#[specta::specta]
pub fn set_close_behavior(
    state: State<'_, AppState>,
    close_behavior: CloseBehavior,
) -> AppResult<Preferences> {
    let cfg = state.config.update(|c| c.close_behavior = close_behavior)?;
    Ok(to_preferences(&cfg))
}

/// Persist the background-collect interval (seconds, clamped to [10, 3600];
///). Pure-local cadence — does not touch the network.
#[tauri::command]
#[specta::specta]
pub fn set_collect_interval(state: State<'_, AppState>, seconds: u32) -> AppResult<Preferences> {
    let clamped = seconds.clamp(10, 3600);
    let cfg = state.config.update(|c| c.collect_interval_secs = clamped)?;
    Ok(to_preferences(&cfg))
}

/// Persist the push-to-sync interval (seconds, clamped to [60, 7200]; Synced
/// only). Decoupled from collect so the Git history grows at this
/// rate, not the (shorter) collect rate.
#[tauri::command]
#[specta::specta]
pub fn set_push_interval(state: State<'_, AppState>, seconds: u32) -> AppResult<Preferences> {
    let clamped = seconds.clamp(60, 7200);
    let cfg = state.config.update(|c| c.push_interval_secs = clamped)?;
    Ok(to_preferences(&cfg))
}

/// Persist the display language and rebuild the tray menu so the
/// "Quit" item follows the new language immediately. The tray item is the only
/// user-facing Rust string; all other UI text is frontend i18n driven by this
/// same preference.
#[tauri::command]
#[specta::specta]
pub fn set_language(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    language: Language,
) -> AppResult<Preferences> {
    let cfg = state.config.update(|c| c.language = language)?;
    if let Some(tray) = app_handle.tray_by_id("main") {
        if let Ok(menu) = crate::tray_menu_for(&app_handle, language) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    Ok(to_preferences(&cfg))
}

/// Persist the lightweight half-icon expand trigger. Pure frontend
/// behavior; Rust doesn't read it back, but it rides ConfigData for unity.
#[tauri::command]
#[specta::specta]
pub fn set_lightweight_expand(
    state: State<'_, AppState>,
    lightweight_expand: LightweightExpand,
) -> AppResult<Preferences> {
    let cfg = state
        .config
        .update(|c| c.lightweight_expand = lightweight_expand)?;
    Ok(to_preferences(&cfg))
}

/// Persist the color skin (multi-skin theming). Pure frontend effect — Rust
/// never reads it back; it rides ConfigData for unity with the other prefs.
#[tauri::command]
#[specta::specta]
pub fn set_skin(state: State<'_, AppState>, skin: Skin) -> AppResult<Preferences> {
    let cfg = state.config.update(|c| c.skin = skin)?;
    Ok(to_preferences(&cfg))
}

/// Resolve the one-time close dialog. `remember` pins `choice` as
/// the persisted behavior; the chosen action is then executed immediately.
/// `Minimize`/`Ask` hide the window (scheduler keeps running); `Quit` exits.
#[tauri::command]
#[specta::specta]
pub fn confirm_close(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    choice: CloseBehavior,
    remember: bool,
) -> AppResult<()> {
    if remember {
        let _ = state.config.update(|c| c.close_behavior = choice);
    }
    match choice {
        CloseBehavior::Quit => app_handle.exit(0),
        CloseBehavior::Minimize | CloseBehavior::Ask => {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    }
    Ok(())
}
