//! Device registry: the single home for the "device" concept — membership,
//! naming, and the per-device name artifact.
//!
//! Three concerns that used to be scattered across `config` / `db` / `ingest` /
//! `sync` / `commands` are collected here:
//! - **Device-name artifact** (`config/devices_<id>.json`, one file per device):
//!   the cloud registry a device publishes its identity to and reads its peers'
//!   identities from. Carried by the normal Git sync.
//! - **Membership**: "which devices exist" is computed from three sources —
//!   this device's own id, the published name artifacts, and the
//!   `repo/data/<id>/` directories — and reconciled against the Local Store's
//!   `device` table (stale local-only rows pruned).
//! - **Naming**: local aliases (set via `set_device_display_name`) are layered
//!   over the synced names at read time.
//!
//! The `device` table CRUD itself (`upsert_device` / `list_devices` /
//! `list_device_ids` / `forget_device_local` / `discover_devices_from_usage`)
//! stays in `db::Store`; this module is the registry orchestrator that calls
//! into it. `is_valid_device_id` / `generate_device_id` stay in `config`
//! (bootstrap coupling); this module calls `crate::config::is_valid_device_id`.

use std::collections::HashSet;

use crate::config::{ConfigData, Paths};
use crate::db::Store;
use crate::error::AppResult;
use crate::model::{DeviceArtifact, DeviceInfo};

// ---------------- Device-name artifact (one file per device) ----------------

/// Idempotently publish THIS device's identity to `config/devices/<id>.json`
/// (device-name sync ADR). Writes only when the file is missing or its
/// `display_name` is stale, so repeated calls (boot, every sync) don't churn
/// the worktree. `first_seen` is preserved across rewrites. Returns whether a
/// write actually happened.
///
/// No network: the file is merely staged in the worktree — the normal Git sync
/// (`commit_all` + `push`) carries the whole repo, so this file rides along.
pub fn ensure_own_device_artifact(
    paths: &Paths,
    device_id: &str,
    display_name: &str,
) -> AppResult<bool> {
    // Flat layout: repo/config/devices_<id>.json (no devices/ subdir).
    let path = paths.devices_file_path(device_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    // Preserve first_seen across rewrites; seed on first publish.
    let first_seen = existing
        .as_deref()
        .and_then(|t| serde_json::from_str::<DeviceArtifact>(t).ok())
        .map(|a| a.first_seen)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let artifact = DeviceArtifact {
        device_id: device_id.to_string(),
        display_name: display_name.to_string(),
        first_seen,
    };
    let desired = serde_json::to_string_pretty(&artifact)?;
    if existing.as_deref().map(str::trim_end) == Some(desired.as_str()) {
        return Ok(false);
    }
    std::fs::write(&path, format!("{desired}\n"))?;
    Ok(true)
}

/// Read every device's identity artifact under `config/devices/`. Skips entries
/// whose stem isn't a valid 12-hex device id and files that fail to parse, so a
/// stray/broken file never blocks the rest from loading.
pub fn read_all_device_artifacts(paths: &Paths) -> Vec<DeviceArtifact> {
    let mut out: Vec<DeviceArtifact> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // New flat layout: config/devices_<id>.json. Strip the `devices_` prefix
    // and `.json` suffix; the remainder must be a valid device id.
    if let Ok(entries) = std::fs::read_dir(&paths.repo_config) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(id) = name
                .strip_prefix("devices_")
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if !crate::config::is_valid_device_id(id) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(a) = serde_json::from_str::<DeviceArtifact>(&text) {
                    if seen.insert(a.device_id.clone()) {
                        out.push(a);
                    }
                }
            }
        }
    }

    // Legacy layout: config/devices/<id>.json (read-only fallback; new wins).
    if let Ok(entries) = std::fs::read_dir(paths.legacy_devices_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !crate::config::is_valid_device_id(stem) || seen.contains(stem) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(a) = serde_json::from_str::<DeviceArtifact>(&text) {
                    if seen.insert(a.device_id.clone()) {
                        out.push(a);
                    }
                }
            }
        }
    }

    out
}

// ---------------- Membership ----------------

/// The set of device ids the local repo currently backs: this device ∪ devices
/// with a published name artifact (`config/devices_<id>.json`) ∪ devices with a
/// data dir under `repo/data/<id>/`. The local repo filesystem is always
/// available (even Standalone), so the caller can run this on both the sync
/// and collect paths. Self is always present.
fn present_device_ids(paths: &Paths, cfg: &ConfigData) -> HashSet<String> {
    let mut present: HashSet<String> = HashSet::new();
    present.insert(cfg.device_id.clone());
    for a in read_all_device_artifacts(paths) {
        present.insert(a.device_id.clone());
    }
    if let Ok(entries) = std::fs::read_dir(&paths.repo_data) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if crate::config::is_valid_device_id(name) {
                    present.insert(name.to_string());
                }
            }
        }
    }
    present
}

// ---------------- Registry reconciliation ----------------

/// Purge local device rows Git no longer backs. Git is the source of truth for
/// which devices exist, so a device with NO git presence is residue and is
/// forgotten locally (row + usage + rollups). "Present" = this device ∪ devices
/// with a registry file (`config/devices_<id>.json`) ∪ devices with a data dir
/// under `repo/data/<id>/`. The local repo filesystem is always available (even
/// Standalone), so this runs on both the sync and collect paths — a stale
/// device is cleaned on the next collect (~30 s via the background scheduler),
/// not only on a pull. `is_self` is always kept. A failure on one id is logged,
/// not fatal.
pub fn reconcile_devices(store: &Store, paths: &Paths, cfg: &ConfigData) -> AppResult<()> {
    // Build the set of devices Git still backs.
    let present = present_device_ids(paths, cfg);

    // Purge dirty rows: local-only devices Git no longer backs. Self is always
    // kept (it's in `present`). A failure on one id is logged, not fatal.
    for id in store.list_device_ids()? {
        if id == cfg.device_id || present.contains(&id) {
            continue;
        }
        match store.forget_device_local(&id) {
            Ok(n) => eprintln!("[vaultone] reconciled stale device {id} ({n} rows dropped)"),
            Err(e) => eprintln!("[vaultone] failed to reconcile device {id}: {e}"),
        }
    }
    Ok(())
}

/// Reload the (just-pulled) cloud device registry into the Store, then
/// reconcile dirty devices. Each registry file upsert is best-effort so one bad
/// row can't abort the rest. Aliases stay local and are layered on at
/// `list_devices`. Used by the usage-sync pull path; reconcile itself also
/// runs on the collect path.
pub(crate) fn reload_devices_into_store(
    store: &Store,
    paths: &Paths,
    cfg: &ConfigData,
) -> AppResult<()> {
    for a in read_all_device_artifacts(paths) {
        let is_self = a.device_id == cfg.device_id;
        if let Err(e) = store.upsert_device(&a.device_id, &a.display_name, is_self) {
            eprintln!("[vaultone] device reload skipped {}: {e}", a.device_id);
        }
    }
    reconcile_devices(store, paths, cfg)
}

// ---------------- Naming layer ----------------

/// Layer local aliases over the synced device names, and re-derive `is_self`
/// from the live config. An alias (set via `set_device_display_name`) wins
/// where present; the device table's synced name (learned from the cloud
/// registry) is kept otherwise. `is_self` is re-derived because the stored
/// column can go stale (e.g. this device's id was regenerated) and a peer must
/// never be mislabeled "this device". Mutates `devices` in place.
pub fn apply_aliases(devices: &mut [DeviceInfo], cfg: &ConfigData) {
    for d in devices {
        // Re-derive is_self from the live config — the stored column can go
        // stale (e.g. this device's id was regenerated) and a peer must never
        // be mislabeled "this device".
        d.is_self = d.device_id == cfg.device_id;
        // Layer local aliases (set_device_display_name) over the synced names:
        // an alias wins where present, the device table's synced name otherwise.
        if let Some(alias) = cfg.device_names.get(&d.device_id) {
            d.display_name = alias.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    /// Git is the source of truth for which devices exist. After a pull,
    /// `reload_devices_into_store` must keep devices Git still backs (this
    /// device, a peer with a registry file, a peer with a data dir) and purge
    /// local-only residue (a device with no git presence at all).
    #[test]
    fn reload_devices_reconciles_stale_local_only_devices() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        std::fs::create_dir_all(&paths.repo_config).unwrap();
        std::fs::create_dir_all(&paths.repo_data).unwrap();
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();

        let self_id = "0123456789ab";
        let live_peer = "aaaaaaaaaaaa"; // backed by a pulled registry file
        let data_peer = "bbbbbbbbbbbb"; // backed by a repo/data/<id>/ dir
        let ghost = "cccccccccccc"; // local-only: no git presence

        let cfg = crate::config::ConfigData {
            device_id: self_id.into(),
            ..Default::default()
        };

        // Seed all four into the local registry.
        for id in [self_id, live_peer, data_peer, ghost] {
            store.upsert_device(id, "name", id == self_id).unwrap();
        }
        assert_eq!(store.list_device_ids().unwrap().len(), 4);

        // Git presence after the (simulated) pull.
        ensure_own_device_artifact(&paths, live_peer, "name").unwrap();
        std::fs::create_dir_all(paths.device_data_dir(data_peer)).unwrap();
        // ghost: intentionally nothing in git.

        reload_devices_into_store(&store, &paths, &cfg).unwrap();

        let ids = store.list_device_ids().unwrap();
        assert!(ids.iter().any(|i| i == self_id), "self always kept");
        assert!(ids.iter().any(|i| i == live_peer), "registry peer kept");
        assert!(ids.iter().any(|i| i == data_peer), "data-dir peer kept");
        assert!(
            !ids.iter().any(|i| i == ghost),
            "local-only ghost must be pruned"
        );
    }

    #[test]
    fn device_artifact_flat_layout_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Writes to the new flat path (config/devices_<id>.json).
        assert!(ensure_own_device_artifact(&paths, "0123456789ab", "Laptop").unwrap());
        // Idempotent: identical content ⇒ no rewrite.
        assert!(!ensure_own_device_artifact(&paths, "0123456789ab", "Laptop").unwrap());
        // Reads back from the flat path.
        let read = read_all_device_artifacts(&paths);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].device_id, "0123456789ab");
        assert_eq!(read[0].display_name, "Laptop");
        // Path is flat — no legacy devices/ subdir was created.
        assert!(paths.devices_file_path("0123456789ab").exists());
        assert!(!paths
            .legacy_devices_dir()
            .join("0123456789ab.json")
            .exists());
    }

    #[test]
    fn read_all_device_artifacts_reads_legacy_layout_too() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        // Seed a legacy file under config/devices/<id>.json (old layout peer).
        let legacy = paths.legacy_devices_dir().join("abcdef012345.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(
            &legacy,
            r#"{"device_id":"abcdef012345","display_name":"OldPeer","first_seen":"2026-01-01T00:00:00.000Z"}"#,
        )
        .unwrap();
        // And a flat file for a different device (new layout).
        ensure_own_device_artifact(&paths, "0123456789ab", "NewPeer").unwrap();

        let mut ids: Vec<String> = read_all_device_artifacts(&paths)
            .into_iter()
            .map(|a| a.device_id)
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["0123456789ab".to_string(), "abcdef012345".to_string()],
            "both layouts are read"
        );
    }

    /// `apply_aliases` re-derives `is_self` from the live config and overlays
    /// local aliases on top of the synced names, leaving un-aliased devices'
    /// names untouched.
    #[test]
    fn apply_aliases_layers_local_names_and_rederives_self() {
        let mut devices = vec![
            DeviceInfo {
                device_id: "0123456789ab".into(),
                display_name: "Synced Self".into(),
                // Stale stored value — must be corrected.
                is_self: false,
                first_seen: String::new(),
            },
            DeviceInfo {
                device_id: "aaaaaaaaaaaa".into(),
                display_name: "Synced Peer".into(),
                is_self: true, // Stale — a peer mislabeled as self.
                first_seen: String::new(),
            },
            DeviceInfo {
                device_id: "bbbbbbbbbbbb".into(),
                display_name: "Other Peer".into(),
                is_self: false,
                first_seen: String::new(),
            },
        ];
        let mut cfg = ConfigData {
            device_id: "0123456789ab".into(),
            ..Default::default()
        };
        cfg.device_names
            .insert("aaaaaaaaaaaa".to_string(), "Aliased Peer".to_string());

        apply_aliases(&mut devices, &cfg);

        assert!(devices[0].is_self, "self re-derived from live cfg");
        assert_eq!(devices[0].display_name, "Synced Self", "self name kept");
        assert!(!devices[1].is_self, "peer no longer mislabeled as self");
        assert_eq!(
            devices[1].display_name, "Aliased Peer",
            "alias wins over synced name"
        );
        assert!(!devices[2].is_self);
        assert_eq!(
            devices[2].display_name, "Other Peer",
            "un-aliased device keeps its synced name"
        );
    }
}
