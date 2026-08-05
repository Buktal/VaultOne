//! Session management — synced groups I/O + the Tauri command surface.
//!
//! Two group tracks:
//! - **Local** (`local_groups` SQLite table): device-private, CRUD immediate,
//!   never in git. Owned by `db::Store`.
//! - **Synced** (`data/<deviceId>/groups.json`): cross-device via git. Each
//!   device writes ONLY its own file; reading merges every device's file by id
//!   (the device-registry pattern). Ids carry a device prefix
//!   (`<deviceId>-<8hex>`) so they are globally unique without coordination.
//!
//! Session CRUD (favorited / custom_title / group membership / list / transcript
//! read) is here too, layered over `db::Store` (sessions table) + `ingest`
//! (transcript I/O). Write commands emit `"sessions_changed"` so the frontend
//! refreshes its session queries.

use std::path::PathBuf;

use tauri::{Emitter, State};

use crate::commands::AppState;
use crate::config::{ConfigData, Paths};
use crate::error::{AppError, AppResult};
use crate::model::{
    LocalGroup, SessionFilter, SessionGroup, SessionMessage, SessionRow, SyncedGroup,
};

/// Per-device synced-groups file: `repo/data/<deviceId>/groups.json`.
fn groups_json_path(paths: &Paths, device_id: &str) -> PathBuf {
    paths.device_data_dir(device_id).join("groups.json")
}

/// Wrapper so the file is a stable JSON object with one array (extensible
/// without a wire break later). Missing file ⇒ empty doc.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct SyncedGroupsDoc {
    #[serde(default)]
    groups: Vec<SyncedGroup>,
}

/// Read one device's synced-groups file. Missing/unreadable ⇒ empty.
fn read_device_synced_groups(paths: &Paths, device_id: &str) -> Vec<SyncedGroup> {
    let path = groups_json_path(paths, device_id);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<SyncedGroupsDoc>(&text)
        .unwrap_or_default()
        .groups
}

/// Every device's synced groups merged by id (latest `updated_at` wins; ties →
/// first-seen). Iterates only valid device dirs so a stray folder never shows
/// up as a groups source. This is the read-side of the per-device-write pattern
/// (mirrors `devices::read_all_device_artifacts`).
pub fn read_all_synced_groups(paths: &Paths) -> Vec<SyncedGroup> {
    let root = &paths.repo_data;
    let mut by_id: std::collections::HashMap<String, SyncedGroup> =
        std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name_owned = entry.file_name();
        let Some(name) = name_owned.to_str() else {
            continue;
        };
        if !crate::config::is_valid_device_id(name) {
            continue;
        }
        for g in read_device_synced_groups(paths, name) {
            let existing = by_id.get(&g.id);
            let take = existing
                .map(|e| e.updated_at < g.updated_at)
                .unwrap_or(true);
            if take {
                by_id.insert(g.id.clone(), g);
            }
        }
    }
    let mut out: Vec<SyncedGroup> = by_id.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Write THIS device's synced-groups file (the device only writes its own —
/// never a peer's). Creates the parent dir.
fn write_own_synced_groups(
    paths: &Paths,
    device_id: &str,
    groups: &[SyncedGroup],
) -> AppResult<()> {
    let path = groups_json_path(paths, device_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let doc = SyncedGroupsDoc {
        groups: groups.to_vec(),
    };
    let json = serde_json::to_string_pretty(&doc)?;
    std::fs::write(&path, format!("{json}\n"))?;
    Ok(())
}

/// Generate a globally-unique synced-group id: `<deviceId>-<8hex>`. The device
/// prefix is the ownership marker (only this device edits the group), so a peer
/// never collides.
fn generate_synced_group_id(device_id: &str) -> String {
    use rand::Rng;
    let bytes: [u8; 4] = rand::thread_rng().gen();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{device_id}-{hex}")
}

/// The subset of synced groups THIS device owns (id prefix matches device_id).
fn own_synced_groups(paths: &Paths, device_id: &str) -> Vec<SyncedGroup> {
    read_device_synced_groups(paths, device_id)
        .into_iter()
        .filter(|g| is_owned_by(g, device_id))
        .collect()
}

/// True iff `group` was created by `device_id` (its id carries the prefix).
fn is_owned_by(group: &SyncedGroup, device_id: &str) -> bool {
    group.id.strip_prefix(&format!("{device_id}-")).is_some()
}

/// Create a synced group owned by this device and commit + push (Synced only).
pub fn create_synced_group_owned(
    paths: &Paths,
    cfg: &ConfigData,
    name: &str,
) -> AppResult<SyncedGroup> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("group name must not be empty".into()));
    }
    let id = generate_synced_group_id(&cfg.device_id);
    let group = SyncedGroup {
        id,
        name: name.to_string(),
        device_id: cfg.device_id.clone(),
        updated_at: crate::time::now_iso(),
    };
    let mut groups = own_synced_groups(paths, &cfg.device_id);
    groups.push(group.clone());
    write_own_synced_groups(paths, &cfg.device_id, &groups)?;
    crate::sync::commit_and_push_best_effort(paths, cfg, "vaultone: groups sync");
    Ok(group)
}

/// Rename a synced group OWNED by this device. A peer's group is read-only here
/// (its owning device will publish the rename on its own round).
pub fn rename_synced_group_owned(
    paths: &Paths,
    cfg: &ConfigData,
    id: &str,
    name: &str,
) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("group name must not be empty".into()));
    }
    let mut groups = own_synced_groups(paths, &cfg.device_id);
    let g = groups.iter_mut().find(|g| g.id == id).ok_or_else(|| {
        AppError::Config(format!("synced group not found (or not owned here): {id}"))
    })?;
    g.name = name.to_string();
    g.updated_at = crate::time::now_iso();
    write_own_synced_groups(paths, &cfg.device_id, &groups)?;
    crate::sync::commit_and_push_best_effort(paths, cfg, "vaultone: groups sync");
    Ok(())
}

/// Delete a synced group OWNED by this device.
pub fn delete_synced_group_owned(paths: &Paths, cfg: &ConfigData, id: &str) -> AppResult<()> {
    let mut groups = own_synced_groups(paths, &cfg.device_id);
    let before = groups.len();
    groups.retain(|g| g.id != id);
    if groups.len() == before {
        return Err(AppError::Config(format!(
            "synced group not found (or not owned here): {id}"
        )));
    }
    write_own_synced_groups(paths, &cfg.device_id, &groups)?;
    crate::sync::commit_and_push_best_effort(paths, cfg, "vaultone: groups sync");
    Ok(())
}

/// Build the unified `SessionGroup` DTO list (local + synced tracks).
fn list_groups_dto(store: &crate::db::Store, paths: &Paths) -> AppResult<Vec<SessionGroup>> {
    let mut out = Vec::new();
    for lg in store.list_local_groups()? {
        out.push(SessionGroup {
            id: lg.id,
            name: lg.name,
            kind: "local".to_string(),
            device_id: String::new(),
        });
    }
    for sg in read_all_synced_groups(paths) {
        out.push(SessionGroup {
            id: sg.id,
            name: sg.name,
            kind: "synced".to_string(),
            device_id: sg.device_id,
        });
    }
    Ok(out)
}

/// Emit `sessions_changed` so the frontend's session queries invalidate.
fn emit_sessions_changed(app_handle: &tauri::AppHandle) {
    let _ = app_handle.emit("sessions_changed", ());
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn query_sessions_cmd(
    state: State<'_, AppState>,
    filter: Option<SessionFilter>,
) -> AppResult<Vec<SessionRow>> {
    state.store.query_sessions(filter.as_ref())
}

#[tauri::command]
#[specta::specta]
pub fn get_session_transcript_cmd(
    state: State<'_, AppState>,
    id: String,
    device_id: String,
) -> AppResult<Vec<SessionMessage>> {
    // The transcript lives in the db (`session_messages`) for every session —
    // favorited or not — so this read no longer depends on the favorites-only
    // jsonl snapshot. `device_id` is the own device; its rows win on uuid
    // conflict (it is the source of truth for a session it collected), then
    // peers' pulled-in rows fill the gaps.
    state.store.query_session_transcript(&id, &device_id)
}

#[tauri::command]
#[specta::specta]
pub fn set_session_favorited_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    device_id: String,
    favorited: bool,
) -> AppResult<()> {
    state
        .store
        .set_session_favorited(&device_id, &id, favorited)?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_session_custom_title_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    device_id: String,
    title: Option<String>,
) -> AppResult<()> {
    state
        .store
        .set_session_custom_title(&device_id, &id, title.as_deref())?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_session_local_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    device_id: String,
    group_id: Option<String>,
) -> AppResult<()> {
    state
        .store
        .set_session_local_group(&device_id, &id, group_id.as_deref())?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_session_synced_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    device_id: String,
    group_id: Option<String>,
) -> AppResult<()> {
    state
        .store
        .set_session_synced_group(&device_id, &id, group_id.as_deref())?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

// ---- local groups ----

#[tauri::command]
#[specta::specta]
pub fn list_local_groups_cmd(state: State<'_, AppState>) -> AppResult<Vec<LocalGroup>> {
    state.store.list_local_groups()
}

#[tauri::command]
#[specta::specta]
pub fn create_local_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> AppResult<LocalGroup> {
    let id = generate_local_group_id();
    let created_at = crate::time::now_iso();
    state
        .store
        .create_local_group(&id, name.trim(), &created_at)?;
    emit_sessions_changed(&app_handle);
    Ok(LocalGroup {
        id,
        name: name.trim().to_string(),
        created_at,
    })
}

#[tauri::command]
#[specta::specta]
pub fn rename_local_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    name: String,
) -> AppResult<()> {
    state.store.rename_local_group(&id, name.trim())?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn delete_local_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> AppResult<()> {
    state.store.delete_local_group(&id)?;
    emit_sessions_changed(&app_handle);
    Ok(())
}

// ---- synced groups ----

#[tauri::command]
#[specta::specta]
pub fn list_synced_groups_cmd(state: State<'_, AppState>) -> AppResult<Vec<SyncedGroup>> {
    Ok(read_all_synced_groups(&state.config.paths()))
}

#[tauri::command]
#[specta::specta]
pub async fn create_synced_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> AppResult<SyncedGroup> {
    let config = state.config.clone();
    let group = tauri::async_runtime::spawn_blocking(move || -> AppResult<SyncedGroup> {
        let cfg = config.get();
        let paths = config.paths();
        create_synced_group_owned(&paths, &cfg, &name)
    })
    .await
    .map_err(|e| AppError::Internal(format!("create_synced_group task failed: {e}")))??;
    emit_sessions_changed(&app_handle);
    Ok(group)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_synced_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    name: String,
) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let cfg = config.get();
        let paths = config.paths();
        rename_synced_group_owned(&paths, &cfg, &id, &name)
    })
    .await
    .map_err(|e| AppError::Internal(format!("rename_synced_group task failed: {e}")))??;
    emit_sessions_changed(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_synced_group_cmd(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let cfg = config.get();
        let paths = config.paths();
        delete_synced_group_owned(&paths, &cfg, &id)
    })
    .await
    .map_err(|e| AppError::Internal(format!("delete_synced_group task failed: {e}")))??;
    emit_sessions_changed(&app_handle);
    Ok(())
}

/// Unified groups list (local + synced) for one-shot UI fetch.
#[tauri::command]
#[specta::specta]
pub fn list_groups_cmd(state: State<'_, AppState>) -> AppResult<Vec<SessionGroup>> {
    list_groups_dto(&state.store, &state.config.paths())
}

/// Local group id: 8 hex chars. Device-private, so no prefix is needed (unlike
/// synced groups, which carry a device prefix for cross-device uniqueness).
fn generate_local_group_id() -> String {
    use rand::Rng;
    let bytes: [u8; 4] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    fn cfg(device_id: &str) -> ConfigData {
        ConfigData {
            device_id: device_id.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn synced_group_id_carries_device_prefix() {
        let id = generate_synced_group_id("aabbccddeeff");
        assert!(id.starts_with("aabbccddeeff-"));
        assert_eq!(id.len(), "aabbccddeeff-".len() + 8);
    }

    #[test]
    fn read_all_synced_groups_merges_by_id_latest_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Device A owns one group.
        let a = SyncedGroup {
            id: "aabbccddeeff-11111111".into(),
            name: "A-group".into(),
            device_id: "aabbccddeeff".into(),
            updated_at: "2026-08-01T10:00:00.000Z".into(),
        };
        write_own_synced_groups(&paths, "aabbccddeeff", std::slice::from_ref(&a)).unwrap();
        // Device B owns another.
        let b = SyncedGroup {
            id: "112233445566-22222222".into(),
            name: "B-group".into(),
            device_id: "112233445566".into(),
            updated_at: "2026-08-02T10:00:00.000Z".into(),
        };
        write_own_synced_groups(&paths, "112233445566", std::slice::from_ref(&b)).unwrap();

        let all = read_all_synced_groups(&paths);
        assert_eq!(all.len(), 2, "both devices' groups merge");
        assert!(all.iter().any(|g| g.id == a.id));
        assert!(all.iter().any(|g| g.id == b.id));
    }

    #[test]
    fn create_rename_delete_synced_group_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let cfg = cfg("aabbccddeeff");
        let g = create_synced_group_owned(&paths, &cfg, "Work").unwrap();
        assert!(is_owned_by(&g, "aabbccddeeff"));
        assert_eq!(g.name, "Work");

        rename_synced_group_owned(&paths, &cfg, &g.id, "Work Important").unwrap();
        let all = read_all_synced_groups(&paths);
        assert_eq!(all[0].name, "Work Important");

        delete_synced_group_owned(&paths, &cfg, &g.id).unwrap();
        assert!(read_all_synced_groups(&paths).is_empty());
    }

    #[test]
    fn rename_peer_owned_group_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Seed a peer-owned group under the peer's dir.
        write_own_synced_groups(
            &paths,
            "112233445566",
            &[SyncedGroup {
                id: "112233445566-99999999".into(),
                name: "Peer".into(),
                device_id: "112233445566".into(),
                updated_at: "2026-08-01T00:00:00.000Z".into(),
            }],
        )
        .unwrap();
        let cfg = cfg("aabbccddeeff");
        // This device does NOT own the group ⇒ reject.
        let err = rename_synced_group_owned(&paths, &cfg, "112233445566-99999999", "x");
        assert!(err.is_err(), "cannot rename a peer's group from here");
    }

    #[test]
    fn read_all_synced_groups_ignores_non_device_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        std::fs::create_dir_all(paths.repo_data.join("not-a-device")).unwrap();
        std::fs::write(
            paths.repo_data.join("not-a-device").join("groups.json"),
            "{\"groups\":[]}",
        )
        .unwrap();
        assert!(read_all_synced_groups(&paths).is_empty());
    }
}
