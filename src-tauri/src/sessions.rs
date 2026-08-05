//! Session management — the domain logic behind the sessions Tauri commands
//! (the command layer itself lives in `commands`).
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
//! read) is layered over `db::Store` (sessions table) + `ingest` (transcript
//! I/O). The `commands` module's write commands call the `*_owned` operations
//! here and emit `"sessions_changed"` so the frontend refreshes its queries.

use std::path::PathBuf;

use crate::config::{ConfigData, Paths};
use crate::error::{AppError, AppResult};
use crate::model::{SessionGroup, SyncedGroup};

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
    let mut by_id: std::collections::HashMap<String, SyncedGroup> =
        std::collections::HashMap::new();
    for name in crate::devices::iter_data_device_ids(paths).unwrap_or_default() {
        for g in read_device_synced_groups(paths, &name) {
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
pub(crate) fn list_groups_dto(
    store: &crate::db::Store,
    paths: &Paths,
) -> AppResult<Vec<SessionGroup>> {
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

/// Local group id: 8 hex chars. Device-private, so no prefix is needed (unlike
/// synced groups, which carry a device prefix for cross-device uniqueness).
pub(crate) fn generate_local_group_id() -> String {
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
