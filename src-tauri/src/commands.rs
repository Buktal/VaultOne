//! Tauri command layer (ADR-0008 typed contract — the query boundary).
//!
//! Every command is `#[specta::specta]` with typed args/return/error; tauri-specta
//! generates the matching typed JS function. `tauri::State` args are injected by
//! the runtime and excluded from the JS signature. JS never sees SQL (ADR-0008).
//!
//! The state holds `Arc`s so blocking work can be moved onto `spawn_blocking`
//! without borrowing the request-scoped `State` (which is not `'static`).

use std::sync::Arc;

use tauri::{Emitter, Manager, State};

use crate::config::{CloseBehavior, ConfigStore, Language, LightweightExpand, Skin};
use crate::db::Store;
use crate::error::{AppError, AppResult};
use crate::ingest::{self, IngestReport};
use crate::model::{
    DeviceInfo, LogsQuery, ModelStatsRow, PricingEntry, RunMode, TrendBucket, TrendPoint,
    UsageFilter, UsageLogRow, UsageStats,
};
use crate::pricing;
use crate::providers::{ClaudeCodeProvider, Provider};
use crate::sync::{ConfigConflictResolution, ConfigSyncOutcome, SyncReport, VerifyReport};

/// App-wide managed state: the Local Store + local config (ADR-0004), wrapped
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

/// App status: device, mode (Standalone/Synced), paths, version (ADR-0006).
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

/// Configure the sync repo + PAT, upgrading Standalone → Synced (ADR-0006).
#[tauri::command]
#[specta::specta]
pub fn set_sync_repo(
    state: State<'_, AppState>,
    repo_url: String,
    github_token: String,
) -> AppResult<RunMode> {
    let cfg = state.config.update(|c| {
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
    Ok(cfg.mode())
}

/// Unbind the repo, downgrading to Standalone (ADR-0006). Clears the local
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

/// Probe a sync repo + PAT for reachability (ADR-0005「测试连接」). Pass explicit
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

/// Rename *this* device (display name only — not a uniqueness key, ADR-0002).
#[tauri::command]
#[specta::specta]
pub fn set_display_name(state: State<'_, AppState>, display_name: String) -> AppResult<()> {
    let cfg = state.config.update(|c| {
        c.display_name = display_name;
    })?;
    state
        .store
        .upsert_device(&cfg.device_id, &cfg.display_name, true)?;
    Ok(())
}

/// Set a friendly name for *another* device seen in the repo (ADR-0002 map).
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

// ---------------- Collect / ingest ----------------

/// Parse Source → Local Store (+ JSONL Artifact). No network (ADR-0012).
/// Shared by the manual `collect_now` command and the background scheduler so
/// both follow the exact same ingest path.
pub fn collect_into(store: &Store, config: &ConfigStore) -> AppResult<IngestReport> {
    let provider = ClaudeCodeProvider::new()?;
    // Incremental collect (ADR-0013): load per-file cursors, parse only new
    // lines, then persist the advanced cursors AFTER ingest — so a failed
    // ingest leaves the cursor untouched (next collect re-parses the same
    // lines; the ledger dedups). First run / empty table ⇒ full scan.
    let progress = store.load_scan_progress()?;
    let (result, delta) = provider.collect_incremental(&progress)?;
    let cfg = config.get();
    store.upsert_device(&cfg.device_id, &cfg.display_name, true)?;
    let book = store.load_pricing_book()?;
    let paths = config.paths();
    let report = ingest::ingest_collected(store, &paths, &cfg.device_id, &book, result)?;
    store.save_scan_progress(&delta)?;
    Ok(report)
}

/// Best-effort push of the current Artifact to the sync repo (Synced only,
/// ADR-0012). Errors are logged, never propagated — push is a backstop.
pub fn push_if_synced(config: &ConfigStore) {
    let cfg = config.get();
    if !cfg.is_synced() {
        return;
    }
    let paths = config.paths();
    if let Err(e) = crate::sync::commit_and_push(&paths, &cfg) {
        eprintln!("[vaultone] push failed: {e}");
    }
}

/// Manual「立即采集」: collect now, best-effort push if Synced, refresh the UI.
/// Heavy disk/git work → offloaded to a thread.
#[tauri::command]
#[specta::specta]
pub async fn collect_now(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> AppResult<IngestReport> {
    let store = state.store.clone();
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<IngestReport> {
        let report = collect_into(&store, &config)?;
        push_if_synced(&config);
        // Notify the UI that usage data changed (event-driven refresh).
        let _ = app_handle.emit("usage_changed", ());
        Ok(report)
    })
    .await
    .map_err(|e| AppError::Internal(format!("collect task failed: {e}")))?
}

/// Manual「立即同步」(ADR-0005, Synced only): pull + import + commit + push.
/// Standalone ⇒ no-op returning a zero report.
#[tauri::command]
#[specta::specta]
pub async fn sync_now(state: State<'_, AppState>) -> AppResult<SyncReport> {
    let store = state.store.clone();
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<SyncReport> {
        let cfg = config.get();
        if !cfg.is_synced() {
            return Ok(SyncReport::default());
        }
        let paths = config.paths();
        crate::sync::sync_now(&store, &paths, &cfg)
    })
    .await
    .map_err(|e| AppError::Internal(format!("sync task failed: {e}")))?
}

/// Manual cloud-config sync (ADR-0005 / #6, Synced only): detect conflicts on
/// shared `config/{app,user,pricing}.json`; if clean, pull + commit + push and
/// reload pricing. Returns a conflict report for the UI to resolve when local
/// and remote both edited the same file. Standalone ⇒ error (UI hides the entry).
#[tauri::command]
#[specta::specta]
pub async fn sync_config(state: State<'_, AppState>) -> AppResult<ConfigSyncOutcome> {
    let store = state.store.clone();
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<ConfigSyncOutcome> {
        let cfg = config.get();
        if !cfg.is_synced() {
            return Err(AppError::Sync(
                "not in Synced mode (ADR-0006): cloud config sync unavailable".into(),
            ));
        }
        let paths = config.paths();
        crate::sync::sync_config(&store, &paths, &cfg)
    })
    .await
    .map_err(|e| AppError::Internal(format!("config sync task failed: {e}")))?
}

/// Apply the user's per-file conflict verdicts, then pull + commit + push
/// (ADR-0005, Synced only). `choices` should cover every file `sync_config`
/// reported as conflicting.
#[tauri::command]
#[specta::specta]
pub async fn resolve_config_conflict(
    state: State<'_, AppState>,
    choices: Vec<ConfigConflictResolution>,
) -> AppResult<ConfigSyncOutcome> {
    let store = state.store.clone();
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<ConfigSyncOutcome> {
        let cfg = config.get();
        if !cfg.is_synced() {
            return Err(AppError::Sync(
                "not in Synced mode (ADR-0006): conflict resolve unavailable".into(),
            ));
        }
        let paths = config.paths();
        crate::sync::resolve_config_conflict(&store, &paths, &cfg, &choices)
    })
    .await
    .map_err(|e| AppError::Internal(format!("config resolve task failed: {e}")))?
}

/// Rebill zero-cost rows whose model now has a price (ADR-0007 top-up).
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
    state.store.list_devices()
}

// ---------------- Pricing (ADR-0007) ----------------

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

/// Re-load pricing from the cloud `pricing.json` into the DB (ADR-0007).
/// In Standalone this is the local `repo/config/pricing.json`; no push.
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
    let text = std::fs::read_to_string(&path)?;
    let entries = pricing::parse_pricing_doc(&text)?;
    for e in &entries {
        state.store.upsert_pricing(&e.to_entry())?;
    }
    Ok(entries.len() as u32)
}

/// Persist current DB pricing to the cloud `pricing.json` (ADR-0007).
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

/// Fetch LiteLLM upstream pricing and merge into the DB (ADR-0007 seed).
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

// ---------------- Preferences (ADR-0012: tray + background) ----------------

/// User-tunable preferences surfaced in the Settings「通用」card (ADR-0012).
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

/// Persist the window-close behavior (ADR-0012).
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
/// ADR-0014). Pure-local cadence — does not touch the network.
#[tauri::command]
#[specta::specta]
pub fn set_collect_interval(state: State<'_, AppState>, seconds: u32) -> AppResult<Preferences> {
    let clamped = seconds.clamp(10, 3600);
    let cfg = state.config.update(|c| c.collect_interval_secs = clamped)?;
    Ok(to_preferences(&cfg))
}

/// Persist the push-to-sync interval (seconds, clamped to [60, 7200]; Synced
/// only; ADR-0014). Decoupled from collect so the Git history grows at this
/// rate, not the (shorter) collect rate.
#[tauri::command]
#[specta::specta]
pub fn set_push_interval(state: State<'_, AppState>, seconds: u32) -> AppResult<Preferences> {
    let clamped = seconds.clamp(60, 7200);
    let cfg = state.config.update(|c| c.push_interval_secs = clamped)?;
    Ok(to_preferences(&cfg))
}

/// Persist the display language (ADR-0016) and rebuild the tray menu so the
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

/// Persist the lightweight half-icon expand trigger (ADR-0015). Pure frontend
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

/// Resolve the one-time close dialog (ADR-0012). `remember` pins `choice` as
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
