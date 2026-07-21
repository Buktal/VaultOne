//! VaultOne Tauri backend library.
//!
//! Module tree: config / db / providers / ingest / pricing /
//! commands, behind a tauri-specta typed contract (ADR-0008). First start
//! bootstraps the local data dir + deviceId and defaults to Standalone
//! (ADR-0006).

use std::sync::Arc;

use specta_typescript::Typescript;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_specta::Builder;

mod commands;
mod config;
mod db;
mod error;
mod ingest;
mod model;
mod pricing;
mod providers;
mod sync;

use commands::AppState;
use config::ConfigStore;
use db::Store;

/// Assemble the tauri-specta builder with all typed commands (ADR-0008).
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        commands::get_app_info,
        commands::set_sync_repo,
        commands::clear_sync_repo,
        commands::set_display_name,
        commands::set_device_display_name,
        commands::collect_now,
        commands::sync_now,
        commands::sync_config,
        commands::resolve_config_conflict,
        commands::rebill_zero_cost,
        commands::query_usage_stats,
        commands::query_usage_trend,
        commands::query_usage_logs,
        commands::count_usage_logs,
        commands::query_models,
        commands::query_distinct_sources,
        commands::query_distinct_models,
        commands::list_devices,
        commands::list_pricing,
        commands::save_pricing_entry,
        commands::delete_pricing_entry,
        commands::reload_pricing_from_file,
        commands::save_pricing_to_file,
        commands::fetch_litellm_pricing,
        commands::get_preferences,
        commands::set_close_behavior,
        commands::set_collect_interval,
        commands::confirm_close,
    ])
}

/// Export TypeScript bindings to the frontend `src/types/generated/`
/// (ADR-0008). Dev builds only; skipped for release binaries. Path is resolved
/// from `CARGO_MANIFEST_DIR` so it is correct regardless of the runtime CWD.
fn export_bindings(builder: &Builder<tauri::Wry>) {
    #[cfg(debug_assertions)]
    {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("src")
            .join("types")
            .join("generated")
            .join("bindings.ts");
        builder
            .export(
                Typescript::default().header("// tauri-specta generated. Do not edit manually."),
                path,
            )
            .expect("Failed to export tauri-specta TypeScript bindings");
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = builder;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    export_bindings(&builder);

    // Headless binding-generation mode: regenerate `bindings.ts` then exit
    // without launching a window (CI / `VAULTONE_GEN_BINDINGS=1 cargo run`).
    // The real app exe carries the Common-Controls v6 manifest from
    // tauri-build, so generation must run through this bin, not `cargo test`.
    if std::env::var("VAULTONE_GEN_BINDINGS").is_ok() {
        return;
    }

    // Boot (ADR-0004 / 0002 / 0006): load config (bootstraps dir + deviceId),
    // open the Local Store (seeds pricing), register this device.
    let config = ConfigStore::load().expect("vaultone: failed to load local config");
    let store = Store::open(&config.paths().db).expect("vaultone: failed to open Local Store");
    {
        let cfg = config.get();
        let _ = store.upsert_device(&cfg.device_id, &cfg.display_name, true);
        // Best-effort zero-cost top-up on boot (ADR-0007): newly-seeded pricing
        // may price rows that were imported while the model was missing.
        let book = store.load_pricing_book().unwrap_or_else(|e| {
            eprintln!("[vaultone] boot rebill skipped: {e}");
            pricing::seed_book()
        });
        if let Err(e) = store.rebill_zero_cost(&book) {
            eprintln!("[vaultone] boot rebill failed: {e}");
        }
    }

    let state = AppState {
        store: Arc::new(store),
        config: Arc::new(config),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .on_window_event(|window, event| {
            // Close→tray routing (ADR-0012). Only the main window exists.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                match state.config.get().close_behavior {
                    crate::config::CloseBehavior::Quit => {
                        // Let it close — triggers the exit flush (RunEvent).
                    }
                    crate::config::CloseBehavior::Minimize => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    crate::config::CloseBehavior::Ask => {
                        api.prevent_close();
                        let _ = window.app_handle().emit("close-requested", ());
                    }
                }
            }
        })
        .setup(|app| {
            let state: tauri::State<AppState> = app.state::<AppState>();

            // Startup pull (ADR-0005, Synced only): covers the device-switch case.
            let store = state.store.clone();
            let config = state.config.clone();
            std::thread::spawn(move || {
                let cfg = config.get();
                if !cfg.is_synced() {
                    return;
                }
                let paths = config.paths();
                match crate::sync::pull_and_import(&store, &paths, &cfg) {
                    Ok(n) => eprintln!("[vaultone] startup pull imported {n} row(s)"),
                    Err(e) => eprintln!("[vaultone] startup pull failed: {e}"),
                }
            });

            // System tray (ADR-0012): left-click shows the window; the menu is
            // Quit only — collect / sync live inside the window, not the tray.
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_item])?;
            let _tray = TrayIconBuilder::with_id("main")
                .tooltip("VaultOne")
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Background scheduler (ADR-0012): periodic collect in both modes,
            // chained push when Synced. Replaces the old push-only loop. The
            // first tick is collect-only so it does not race the startup pull's
            // git-worktree ops; later ticks collect + push. Interval is re-read
            // every tick so Settings changes apply without a restart.
            let store = state.store.clone();
            let config = state.config.clone();
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut first = true;
                loop {
                    let interval = config.get().collect_interval_secs.clamp(60, 3600);
                    if let Err(e) = commands::collect_into(&store, &config) {
                        eprintln!("[vaultone] scheduled collect failed: {e}");
                    }
                    let _ = app_handle.emit("usage_changed", ());
                    if !first {
                        commands::push_if_synced(&config);
                    }
                    first = false;
                    std::thread::sleep(std::time::Duration::from_secs(interval));
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // Exit flush (ADR-0005): push any unpushed Artifact before quitting,
        // covering the close-A / open-B device switch. Synced only, best-effort.
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let state: tauri::State<AppState> = app_handle.state::<AppState>();
            let cfg = state.config.get();
            if cfg.is_synced() {
                let paths = state.config.paths();
                if let Err(e) = crate::sync::commit_and_push(&paths, &cfg) {
                    eprintln!("[vaultone] exit flush failed: {e}");
                }
            }
        }
    });
}
