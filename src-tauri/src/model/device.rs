//! Device concepts: the published identity artifact and the read-side device /
//! run-mode types. Device membership and naming *logic* lives in `crate::devices`;
//! this file holds only the cross-boundary data shapes shared by db, sync, and
//! the frontend.

// ---- Device-name sync artifact ----

/// A device's published identity, materialized one-per-file at
/// `config/devices_<device_id>.json` (flattened — no `devices/` subdir). Each device writes ONLY its own file, so
/// concurrent edits by different devices never collide (zero Git merge
/// conflict). Only the authoritative self-name syncs; per-device aliases stay
/// local (`config.json`, never in the repo).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceArtifact {
    /// 12-hex device id (matches the filename and `usage_records.device_id`).
    pub device_id: String,
    /// This device's self-chosen display name — the authoritative name other
    /// devices learn by pulling this file.
    pub display_name: String,
    /// ISO8601 UTC of first publish; preserved across rewrites so it stays the
    /// device's stable "first seen" timestamp.
    pub first_seen: String,
}

// ---- Device & mode ----

/// A known device. `is_self` marks the device running this instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub is_self: bool,
    pub first_seen: String,
}

/// Run mode: default Standalone; Synced once a repo is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Standalone,
    Synced,
}
