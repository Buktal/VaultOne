//! Library — a per-device, git-mediated cloud-storage relay.
//!
//! Users drop arbitrary files / dirs in; they land under
//! `repo/library/<deviceId>/` and ride the normal Git sync. Upload is the only
//! automatic direction (drag ⇒ write + push). Download is manual — the user
//! exports an item to a path they choose; VaultOne never writes into an AI
//! tool's own config dir. Same-name same-kind overwrites (Git history is the
//! safety net); same-name different-kind is rejected (a path cannot be both a
//! file and a directory, and the delete-then-create it would need is
//! destructive). Per-device subtrees never collide across devices.

use std::path::{Path, PathBuf};

use tauri::State;

use crate::commands::AppState;
use crate::config::{ConfigData, ConfigStore};
use crate::error::{AppError, AppResult};

/// A Library entry is either a single file or a directory tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    File,
    Dir,
}

/// One entry under a device's Library subtree, as shown in the list.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LibraryEntry {
    /// Display name (file or dir basename).
    pub name: String,
    pub kind: LibraryKind,
    /// Bytes (files only; 0 for dirs — size is not recursed).
    pub size: f64,
    /// Epoch millis (f64 — specta-safe, dayjs-friendly).
    pub modified_ms: f64,
    /// Owning device id.
    pub device_id: String,
    /// Owning device display name (self name or a known alias).
    pub device_name: String,
    pub is_self: bool,
    /// Path relative to the library root: `<deviceId>/<sub...>/<name>`. Used to
    /// target delete / rename / export.
    pub rel_path: String,
    /// Absolute filesystem path, for the frontend's `convertFileSrc` preview.
    pub abs_path: String,
}

/// One item the user is uploading (from the pending-upload dialog).
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct UploadItem {
    /// Absolute source path on this machine (from the drag-drop event).
    pub source_path: String,
    /// Final name in the library (the user may have renamed it).
    pub target_name: String,
}

/// What `forget_device` does with a peer's library subtree
/// (`repo/library/<peerId>/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryForgetAction {
    /// Move the subtree into THIS device's library under `from-<peer>/`.
    Migrate,
    /// Delete the subtree outright.
    Delete,
}

/// File/folder counts for a device's library subtree, shown in the
/// forget-device dialog so the user sees what they are migrating or deleting.
/// f64 to stay specta-safe across the TS boundary (counts, never fractional).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DeviceLibrarySummary {
    pub files: f64,
    pub dirs: f64,
}

/// Special device-scope value meaning "every device".
const SCOPE_ALL: &str = "all";

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

/// List the direct children of `(device_scope, subpath)` under the library
/// root. `device_scope = "all"` aggregates every device dir; a specific id
/// scopes to one. `subpath` is relative to each device's own root (used when
/// drilling into a directory). `is_self` and the device's display name are
/// layered on from the config.
pub fn scan(
    config: &ConfigStore,
    device_scope: &str,
    subpath: &str,
) -> AppResult<Vec<LibraryEntry>> {
    let paths = config.paths();
    let cfg = config.get();
    let lib_root = paths.library.clone();

    let device_ids = match device_scope {
        SCOPE_ALL | "" => device_dirs(&lib_root, &cfg),
        id => vec![id.to_string()],
    };

    let mut out = Vec::new();
    for did in device_ids {
        let dir = lib_root.join(&did).join(subpath_rel(subpath));
        if !dir.is_dir() {
            continue;
        }
        let is_self = did == cfg.device_id;
        let device_name = if is_self {
            cfg.display_name.clone()
        } else {
            cfg.device_names
                .get(&did)
                .cloned()
                .unwrap_or_else(|| did.clone())
        };
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".gitkeep" {
                continue;
            }
            let meta = entry.metadata()?;
            let kind = if meta.is_dir() {
                LibraryKind::Dir
            } else {
                LibraryKind::File
            };
            let size = if meta.is_file() {
                meta.len() as f64
            } else {
                0.0
            };
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            let rel_path = join_rel(&did, subpath, &name);
            let abs_path = entry.path().to_string_lossy().to_string();
            out.push(LibraryEntry {
                name,
                kind,
                size,
                modified_ms,
                device_id: did.clone(),
                device_name: device_name.clone(),
                is_self,
                rel_path,
                abs_path,
            });
        }
    }
    Ok(out)
}

/// Subpath normalised to a relative PathBuf (empty ⇒ device root).
fn subpath_rel(subpath: &str) -> PathBuf {
    let trimmed = subpath.trim().trim_matches('/');
    if trimmed.is_empty() {
        PathBuf::new()
    } else {
        PathBuf::from(trimmed)
    }
}

/// `<deviceId>/<sub>/<name>`, forward-slash joined for cross-platform rel paths.
fn join_rel(device_id: &str, subpath: &str, name: &str) -> String {
    let sub = subpath.trim().trim_matches('/');
    if sub.is_empty() {
        format!("{device_id}/{name}")
    } else {
        format!("{device_id}/{sub}/{name}")
    }
}

/// Every device id with a library dir on disk, self first. Peer dirs are
/// filtered by the device-id shape so stray folders never show up as devices.
fn device_dirs(lib_root: &Path, cfg: &ConfigData) -> Vec<String> {
    let mut ids = Vec::new();
    if !cfg.device_id.is_empty() {
        ids.push(cfg.device_id.clone());
    }
    if let Ok(entries) = std::fs::read_dir(lib_root) {
        for e in entries.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Some(name) = e.file_name().to_str() {
                if name != cfg.device_id && crate::config::is_valid_device_id(name) {
                    ids.push(name.to_string());
                }
            }
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// upload
// ---------------------------------------------------------------------------

/// Copy each pending item into this device's library subtree at `subpath`,
/// overwriting same-name same-kind entries. Rejects same-name different-kind.
/// The caller commits + pushes after a successful batch.
pub fn upload(
    paths: &crate::config::Paths,
    cfg: &ConfigData,
    items: &[UploadItem],
    subpath: &str,
) -> AppResult<()> {
    if cfg.device_id.is_empty() {
        return Err(AppError::Config("device id not initialized".into()));
    }
    let dest_dir = paths
        .library
        .join(&cfg.device_id)
        .join(subpath_rel(subpath));
    std::fs::create_dir_all(&dest_dir)?;
    for item in items {
        let src = Path::new(&item.source_path);
        if !src.exists() {
            return Err(AppError::Config(format!(
                "source not found: {}",
                src.display()
            )));
        }
        let name = item.target_name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".gitkeep" {
            return Err(AppError::Config(format!("invalid target name: {name}")));
        }
        let dst = dest_dir.join(name);
        // Reject same-name different-kind (a path cannot be both file and dir,
        // and the delete-then-create it would need is destructive).
        if dst.exists() {
            match (src.is_dir(), dst.is_dir()) {
                (true, false) => {
                    return Err(AppError::Config(format!(
                        "{name} exists as a file; cannot overwrite with a directory"
                    )));
                }
                (false, true) => {
                    return Err(AppError::Config(format!(
                        "{name} exists as a directory; cannot overwrite with a file"
                    )));
                }
                _ => {}
            }
        }
        // Overwrite same-kind: drop the existing target first.
        if dst.exists() {
            if dst.is_dir() {
                std::fs::remove_dir_all(&dst)?;
            } else {
                std::fs::remove_file(&dst)?;
            }
        }
        if src.is_dir() {
            copy_dir_recursive(src, &dst)?;
            let _ = ensure_gitkeep(&dst);
        } else {
            std::fs::copy(src, &dst)?;
        }
    }
    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Git does not track empty directories — drop a `.gitkeep` so an emptied /
/// newly-empty dir still syncs.
fn ensure_gitkeep(dir: &Path) -> AppResult<()> {
    if std::fs::read_dir(dir)?.next().is_none() {
        std::fs::write(dir.join(".gitkeep"), b"")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// export / delete / rename
// ---------------------------------------------------------------------------

/// Copy a library entry (file or dir) into a target dir the user chose. The
/// entry keeps its name; VaultOne never writes into an AI tool's own paths.
pub fn export_entry(
    paths: &crate::config::Paths,
    rel_path: &str,
    target_dir: &str,
) -> AppResult<()> {
    let src = resolve_rel(paths, rel_path)?;
    let name = src
        .file_name()
        .ok_or_else(|| AppError::Config("entry has no name".into()))?;
    let dst = Path::new(target_dir).join(name);
    if src.is_dir() {
        copy_dir_recursive(&src, &dst)?;
    } else {
        std::fs::copy(&src, &dst)?;
    }
    Ok(())
}

/// Delete a library entry (file or dir). The caller commits + pushes.
pub fn delete_entry(paths: &crate::config::Paths, rel_path: &str) -> AppResult<()> {
    let target = resolve_rel(paths, rel_path)?;
    if target.is_dir() {
        std::fs::remove_dir_all(&target)?;
    } else {
        std::fs::remove_file(&target)?;
    }
    // Re-seal the now-maybe-empty parent with .gitkeep.
    if let Some(parent) = target.parent() {
        let _ = ensure_gitkeep(parent);
    }
    Ok(())
}

/// Rename a library entry in place. The caller commits + pushes.
pub fn rename_entry(paths: &crate::config::Paths, rel_path: &str, new_name: &str) -> AppResult<()> {
    let target = resolve_rel(paths, rel_path)?;
    let name = new_name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(AppError::Config(format!("invalid name: {name}")));
    }
    let dst = target.parent().unwrap_or_else(|| Path::new(".")).join(name);
    if dst.exists() && dst != target {
        return Err(AppError::Config(format!("{name} already exists")));
    }
    std::fs::rename(&target, &dst)?;
    Ok(())
}

/// Resolve a `<deviceId>/<sub>/<name>` rel path under the library root, then
/// canonicalize and confirm it stays inside the root (defends against `../`).
fn resolve_rel(paths: &crate::config::Paths, rel_path: &str) -> AppResult<PathBuf> {
    let rel = rel_path.trim().trim_matches('/');
    if rel.is_empty() {
        return Err(AppError::Config("empty library path".into()));
    }
    let p = paths.library.join(rel);
    let canon = p
        .canonicalize()
        .map_err(|_| AppError::Config(format!("library entry not found: {rel_path}")))?;
    let root_canon = paths
        .library
        .canonicalize()
        .unwrap_or_else(|_| paths.library.clone());
    if !canon.starts_with(&root_canon) {
        return Err(AppError::Config("library path escapes the root".into()));
    }
    Ok(canon)
}

// ---------------------------------------------------------------------------
// device forget (migrate / delete a peer's subtree)
// ---------------------------------------------------------------------------

/// Sanitise a peer display name into a safe `from-<name>` folder label,
/// falling back to the device id when the name is empty. Path separators and
/// Windows-reserved chars become `_`; length is capped.
fn migrate_folder_name(name: &str, fallback_id: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string();
    let base = if cleaned.is_empty() {
        fallback_id.to_string()
    } else {
        cleaned.chars().take(64).collect()
    };
    format!("from-{base}")
}

/// Apply a [`LibraryForgetAction`] to a peer's library subtree. Local-only —
/// no Git push (matches `forget_device`'s existing `repo/data/<id>/` cleanup;
/// a peer still active elsewhere reappears on the next sync). `Migrate` moves
/// the subtree into THIS device's library under `from-<peerName>/` (a unique
/// suffix is appended on collision so repeated migrations never overwrite).
/// No-op when the peer has no library subtree.
pub fn forget_device_library(
    paths: &crate::config::Paths,
    cfg: &ConfigData,
    peer_id: &str,
    action: LibraryForgetAction,
    peer_name: &str,
) -> AppResult<()> {
    let peer_dir = paths.library.join(peer_id);
    if !peer_dir.exists() {
        return Ok(());
    }
    match action {
        LibraryForgetAction::Delete => {
            std::fs::remove_dir_all(&peer_dir)?;
        }
        LibraryForgetAction::Migrate => {
            let self_root = paths.library.join(&cfg.device_id);
            std::fs::create_dir_all(&self_root)?;
            let folder = migrate_folder_name(peer_name, peer_id);
            let mut target = self_root.join(&folder);
            let mut n = 2;
            while target.exists() {
                target = self_root.join(format!("{folder}-{n}"));
                n += 1;
            }
            std::fs::rename(&peer_dir, &target)?;
        }
    }
    Ok(())
}

/// Recursively count files (excl. `.gitkeep`) and folders under a device's
/// library subtree; `{0, 0}` when it does not exist.
fn count_subtree(dir: &Path) -> DeviceLibrarySummary {
    fn walk(dir: &Path, files: &mut f64, dirs: &mut f64) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy() == ".gitkeep" {
                continue;
            }
            let Ok(ft) = e.file_type() else {
                continue;
            };
            if ft.is_dir() {
                *dirs += 1.0;
                walk(&e.path(), files, dirs);
            } else {
                *files += 1.0;
            }
        }
    }
    let mut files = 0.0;
    let mut dirs = 0.0;
    if dir.is_dir() {
        walk(dir, &mut files, &mut dirs);
    }
    DeviceLibrarySummary { files, dirs }
}

// ---------------------------------------------------------------------------
// commit + push (best-effort, Synced only)
// ---------------------------------------------------------------------------

/// Stage + commit + push any library change (best-effort, Synced only).
/// Standalone is a no-op — the files already sit in the worktree, nothing to
/// push. Delegates to sync's commit+push core; push failures are logged there,
/// not propagated — the next collect/sync round carries the change up.
fn commit_push_library(paths: &crate::config::Paths, cfg: &ConfigData) {
    crate::sync::commit_and_push_best_effort(paths, cfg, "vaultone: library sync");
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn scan_library(
    state: State<'_, AppState>,
    device_scope: String,
    subpath: String,
) -> AppResult<Vec<LibraryEntry>> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || scan(&config, &device_scope, &subpath))
        .await
        .map_err(|e| AppError::Internal(format!("library scan task failed: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn upload_to_library(
    state: State<'_, AppState>,
    items: Vec<UploadItem>,
    subpath: String,
) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let cfg = config.get();
        let paths = config.paths();
        upload(&paths, &cfg, &items, &subpath)?;
        commit_push_library(&paths, &cfg);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(format!("library upload task failed: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn export_from_library(
    state: State<'_, AppState>,
    rel_path: String,
    target_dir: String,
) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let paths = config.paths();
        export_entry(&paths, &rel_path, &target_dir)
    })
    .await
    .map_err(|e| AppError::Internal(format!("library export task failed: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn delete_from_library(state: State<'_, AppState>, rel_path: String) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let cfg = config.get();
        let paths = config.paths();
        delete_entry(&paths, &rel_path)?;
        commit_push_library(&paths, &cfg);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(format!("library delete task failed: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn rename_in_library(
    state: State<'_, AppState>,
    rel_path: String,
    new_name: String,
) -> AppResult<()> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let cfg = config.get();
        let paths = config.paths();
        rename_entry(&paths, &rel_path, &new_name)?;
        commit_push_library(&paths, &cfg);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(format!("library rename task failed: {e}")))?
}

/// File/folder counts for one device's library subtree — used by the
/// forget-device dialog to show what would be migrated or deleted.
#[tauri::command]
#[specta::specta]
pub async fn library_device_summary(
    state: State<'_, AppState>,
    device_id: String,
) -> AppResult<DeviceLibrarySummary> {
    let config = state.config.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = config.paths();
        Ok(count_subtree(&paths.library.join(&device_id)))
    })
    .await
    .map_err(|e| AppError::Internal(format!("library summary task failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    fn self_cfg() -> ConfigData {
        ConfigData {
            device_id: "aabbccddeeff".to_string(),
            ..Default::default()
        }
    }

    fn write_file(p: &Path, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn migrate_moves_subtree_into_self() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let cfg = self_cfg();
        let peer = "112233445566";

        let peer_dir = paths.library.join(peer);
        std::fs::create_dir_all(peer_dir.join("sub")).unwrap();
        write_file(&peer_dir.join("note.txt"), "hi");
        write_file(&peer_dir.join("sub").join("inner.txt"), "yo");
        write_file(&peer_dir.join(".gitkeep"), "");

        forget_device_library(&paths, &cfg, peer, LibraryForgetAction::Migrate, "MacBook").unwrap();

        // Peer subtree gone; contents now under self/from-MacBook/.
        assert!(!peer_dir.exists());
        let moved = paths.library.join(&cfg.device_id).join("from-MacBook");
        assert!(moved.is_dir());
        assert_eq!(
            std::fs::read_to_string(moved.join("note.txt")).unwrap(),
            "hi"
        );
        assert!(moved.join("sub").is_dir());
        assert_eq!(
            std::fs::read_to_string(moved.join("sub").join("inner.txt")).unwrap(),
            "yo"
        );
    }

    #[test]
    fn migrate_collision_appends_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let cfg = self_cfg();
        let peer = "112233445566";

        // Pre-existing migrate target must NOT be overwritten.
        let existing = paths.library.join(&cfg.device_id).join("from-MacBook");
        std::fs::create_dir_all(&existing).unwrap();
        write_file(&existing.join("keeper.txt"), "keep");

        let peer_dir = paths.library.join(peer);
        std::fs::create_dir_all(&peer_dir).unwrap();
        write_file(&peer_dir.join("note.txt"), "hi");

        forget_device_library(&paths, &cfg, peer, LibraryForgetAction::Migrate, "MacBook").unwrap();

        assert_eq!(
            std::fs::read_to_string(existing.join("keeper.txt")).unwrap(),
            "keep"
        );
        let moved = paths.library.join(&cfg.device_id).join("from-MacBook-2");
        assert_eq!(
            std::fs::read_to_string(moved.join("note.txt")).unwrap(),
            "hi"
        );
    }

    #[test]
    fn delete_removes_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let cfg = self_cfg();
        let peer = "112233445566";

        let peer_dir = paths.library.join(peer);
        std::fs::create_dir_all(peer_dir.join("sub")).unwrap();
        write_file(&peer_dir.join("note.txt"), "hi");

        forget_device_library(&paths, &cfg, peer, LibraryForgetAction::Delete, "MacBook").unwrap();

        assert!(!peer_dir.exists());
    }

    #[test]
    fn forget_is_noop_when_peer_has_no_library() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let cfg = self_cfg();
        // No library/<peer>/ exists for either action.
        forget_device_library(
            &paths,
            &cfg,
            "112233445566",
            LibraryForgetAction::Delete,
            "MacBook",
        )
        .unwrap();
        forget_device_library(
            &paths,
            &cfg,
            "112233445566",
            LibraryForgetAction::Migrate,
            "MacBook",
        )
        .unwrap();
        // Migrate must not spuriously create the self root on a no-op.
        assert!(!paths.library.join(&cfg.device_id).exists());
    }

    #[test]
    fn count_subtree_excludes_gitkeep() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let peer_dir = paths.library.join("112233445566");
        std::fs::create_dir_all(peer_dir.join("d1")).unwrap();
        write_file(&peer_dir.join("a.txt"), "a");
        write_file(&peer_dir.join(".gitkeep"), "");
        write_file(&peer_dir.join("d1").join("b.txt"), "b");

        let s = count_subtree(&peer_dir);
        assert_eq!(s.files, 2.0); // a.txt + d1/b.txt (.gitkeep excluded)
        assert_eq!(s.dirs, 1.0); // d1
    }

    #[test]
    fn count_subtree_missing_dir_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(tmp.path());
        let s = count_subtree(&paths.library.join("nope"));
        assert_eq!(s.files, 0.0);
        assert_eq!(s.dirs, 0.0);
    }

    #[test]
    fn migrate_folder_name_sanitises() {
        assert_eq!(
            migrate_folder_name("MacBook", "112233445566"),
            "from-MacBook"
        );
        assert_eq!(migrate_folder_name("a/b\\c", "112233445566"), "from-a_b_c");
        assert_eq!(migrate_folder_name("", "112233445566"), "from-112233445566");
        assert_eq!(
            migrate_folder_name("   ", "112233445566"),
            "from-112233445566"
        );
    }
}
