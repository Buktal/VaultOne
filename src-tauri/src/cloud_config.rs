//! Cloud-config conflict sync (Synced-mode only).
//!
//! Detects conflicts between local worktree edits and remote changes on the
//! shared config files (app / user / pricing), and either reports them for the
//! UI to resolve or applies the user's per-file verdicts. Pulled pricing and
//! the device registry are reloaded into the Store. Git primitives (pull /
//! commit / push / fetch) live in [`crate::sync`]; this module owns the
//! config-specific conflict model and the dirty-preserving pull.

use git2::{Oid, Repository, Status};

use crate::config::ConfigData;
use crate::error::{AppError, AppResult};
use crate::sync::{
    author_email, commit_all, fetch_origin, has_changes, open_or_clone_for_device, pull,
    push, reload_devices_into_store, require_synced,
};

/// A cloud-config file under `repo/config/`. Crosses the boundary as
/// a snake_case tag (`"pricing"` …) so the UI can switch on it without path math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFile {
    App,
    User,
    Pricing,
}

impl ConfigFile {
    /// Repo-relative path of this config file, e.g. `config/pricing.json`.
    pub fn rel_path(self) -> &'static str {
        match self {
            ConfigFile::App => "config/app.json",
            ConfigFile::User => "config/user.json",
            ConfigFile::Pricing => "config/pricing.json",
        }
    }
}

/// Recognize a tracked cloud-config path from a git status / diff entry.
/// Returns `None` for anything that is not one of the three config files.
fn parse_config_file(path: &str) -> Option<ConfigFile> {
    match path.trim_start_matches("./") {
        "config/app.json" => Some(ConfigFile::App),
        "config/user.json" => Some(ConfigFile::User),
        "config/pricing.json" => Some(ConfigFile::Pricing),
        _ => None,
    }
}

/// User's per-file verdict for a conflict ("pick a version").
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSyncChoice {
    /// Discard the remote change, keep the local worktree version.
    KeepLocal,
    /// Discard the local worktree change, take the remote version.
    KeepRemote,
}

/// One per-file verdict the UI submits to resolve a batch of conflicts. Preferred
/// over a `(ConfigFile, ConfigSyncChoice)` tuple so the JS contract is a named
/// object (`{ file, choice }`) rather than a positional pair.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigConflictResolution {
    pub file: ConfigFile,
    pub choice: ConfigSyncChoice,
}

/// One conflicting config file with both sides for the UI to preview.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ConfigConflict {
    pub file: ConfigFile,
    /// Repo-relative path, for display (`config/pricing.json`).
    pub path: String,
    /// Worktree version (truncated).
    pub local_preview: String,
    /// Remote-tip version (truncated).
    pub remote_preview: String,
}

/// Outcome of a cloud-config sync round. Flattened (not a tagged enum) so the
/// contract is stable and trivial to narrow on the JS side:
/// `if (outcome.has_conflict) { show conflicts } else { toast(pushed) }`.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct ConfigSyncOutcome {
    /// True when a conflict blocks the sync; resolve via `resolve_config_conflict`.
    pub has_conflict: bool,
    /// Populated iff `has_conflict`.
    pub conflicts: Vec<ConfigConflict>,
    /// True iff a local change was committed and pushed this round.
    pub pushed: bool,
    /// Config files this pull updated from the remote.
    pub pulled_files: Vec<ConfigFile>,
    /// True iff `pricing.json` changed remotely and was reloaded into the Store.
    pub pricing_changed: bool,
}

/// The remote tip Oid for the current branch, or `None` if the remote does not
/// yet carry this branch (first push pending — nothing to pull).
fn origin_tip_oid(repo: &Repository) -> AppResult<Option<Oid>> {
    let head = repo.head()?;
    let branch = head
        .shorthand()
        .ok_or_else(|| AppError::Sync("HEAD is detached; cannot resolve remote tip".into()))?;
    match repo.refname_to_id(&format!("refs/remotes/origin/{branch}")) {
        Ok(oid) => Ok(Some(oid)),
        Err(_) => Ok(None),
    }
}

/// Config files with uncommitted worktree changes (modified or new).
fn dirty_config_files(repo: &Repository) -> AppResult<Vec<ConfigFile>> {
    let mut out = Vec::new();
    for entry in repo.statuses(None)?.iter() {
        let Some(p) = entry.path() else { continue };
        let s = entry.status();
        if !s.contains(Status::WT_MODIFIED) && !s.contains(Status::WT_NEW) {
            continue;
        }
        if let Some(f) = parse_config_file(p) {
            out.push(f);
        }
    }
    Ok(out)
}

/// Config files the remote tip changed relative to our local HEAD.
fn remote_changed_config_files(repo: &Repository, origin_oid: Oid) -> AppResult<Vec<ConfigFile>> {
    let head_tree = repo.head()?.peel_to_commit()?.tree()?;
    let origin_tree = repo.find_commit(origin_oid)?.tree()?;
    // diff(old=head, new=origin) ⇒ what the remote changed vs our HEAD.
    let diff = repo.diff_tree_to_tree(Some(&head_tree), Some(&origin_tree), None)?;
    let mut out = Vec::new();
    for d in diff.deltas() {
        let path = d.new_file().path().or_else(|| d.old_file().path());
        if let Some(f) = path.and_then(|p| p.to_str()).and_then(parse_config_file) {
            out.push(f);
        }
    }
    Ok(out)
}

/// Read a blob at a repo-relative path from the given commit, if present.
fn read_blob(repo: &Repository, commit_oid: Oid, rel_path: &str) -> Option<Vec<u8>> {
    let commit = repo.find_commit(commit_oid).ok()?;
    let tree = commit.tree().ok()?;
    let entry = tree.get_path(std::path::Path::new(rel_path)).ok()?;
    let obj = entry.to_object(repo).ok()?;
    let blob = obj.as_blob()?;
    Some(blob.content().to_vec())
}

/// Trim a config blob to a UI-friendly preview (UTF-8 lossy, ≤ 240 chars).
fn preview(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    const MAX: usize = 240;
    if s.chars().count() > MAX {
        let head: String = s.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        s.to_string()
    }
}

/// A cheap content fingerprint of the local pricing.json (empty when absent).
fn pricing_fingerprint(paths: &crate::config::Paths) -> String {
    std::fs::read_to_string(paths.pricing_json()).unwrap_or_default()
}

/// Reload the (just-pulled) cloud `pricing.json` into the Store.
fn reload_pricing_into_store(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
) -> AppResult<()> {
    let path = paths.pricing_json();
    if !path.exists() {
        return Ok(());
    }
    store.reload_pricing_from_path(&path)?;
    Ok(())
}

/// Fast-forward pull that preserves uncommitted worktree edits to files the
/// remote did NOT touch. Conflict files (local dirty ∩ remote changed) MUST be
/// resolved by the caller beforehand — `sync_config` pre-checks and surfaces
/// them, `resolve_config_conflict` rewrites them first.
///
/// Why not SAFE checkout: git2's `checkout_head(SAFE)` treats a stale worktree
/// copy of a file the remote changed as a "local modification" and refuses to
/// update it, so incoming changes silently fail to land. `force` lands them but
/// clobbers genuine local edits. So we snapshot every modified/new worktree
/// file, run the ordinary force fast-forward (which updates incoming files),
/// then write the snapshot back. Because conflicts are pre-excluded, no
/// snapshot path collides with an incoming change, so restoring cannot clobber a
/// remote update.
fn pull_preserving_dirty(repo: &Repository, token: &str) -> AppResult<()> {
    let dirty: Vec<(String, Vec<u8>)> = {
        let statuses = repo.statuses(None)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| AppError::Sync("repo has no workdir".into()))?;
        statuses
            .iter()
            .filter_map(|e| {
                let s = e.status();
                if !(s.contains(Status::WT_MODIFIED) || s.contains(Status::WT_NEW)) {
                    return None;
                }
                let p = e.path()?.to_string();
                let content = std::fs::read(workdir.join(&p)).ok()?;
                Some((p, content))
            })
            .collect()
    };

    pull(repo, token)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| AppError::Sync("repo has no workdir".into()))?;
    for (p, content) in &dirty {
        let abs = workdir.join(p);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs, content)?;
    }
    Ok(())
}

/// Manual cloud-config sync (Synced-only). Detects conflicts between
/// local worktree edits and remote changes on shared config files; if clean,
/// pulls (SAFE), commits + pushes any local change, and reloads pricing.
pub fn sync_config(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<ConfigSyncOutcome> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone_for_device(&url, &paths.repo, &token, &cfg.device_id)?;
    fetch_origin(&repo, &token)?;

    let dirty = dirty_config_files(&repo)?;
    let origin_oid_opt = origin_tip_oid(&repo)?;
    let remote_changed = match origin_oid_opt {
        Some(oid) => remote_changed_config_files(&repo, oid)?,
        None => Vec::new(),
    };

    // Conflict = worktree-dirty ∩ remote-changed.
    let conflicts: Vec<ConfigConflict> = dirty
        .iter()
        .copied()
        .filter(|f| remote_changed.contains(f))
        .map(|f| {
            let rel = f.rel_path();
            let local_bytes = std::fs::read(paths.repo.join(rel)).unwrap_or_default();
            let remote_bytes = origin_oid_opt
                .and_then(|oid| read_blob(&repo, oid, rel))
                .unwrap_or_default();
            ConfigConflict {
                file: f,
                path: rel.to_string(),
                local_preview: preview(&local_bytes),
                remote_preview: preview(&remote_bytes),
            }
        })
        .collect();

    if !conflicts.is_empty() {
        return Ok(ConfigSyncOutcome {
            has_conflict: true,
            conflicts,
            pushed: false,
            pulled_files: Vec::new(),
            pricing_changed: false,
        });
    }

    // No conflict: pull (preserving unrelated local edits), then commit + push.
    let pricing_before = pricing_fingerprint(paths);
    pull_preserving_dirty(&repo, &token)?;
    let pricing_changed = pricing_before != pricing_fingerprint(paths);
    if pricing_changed {
        reload_pricing_into_store(store, paths)?;
    }
    // Device-name registry rides in the same pull (config/devices_<id>.json).
    reload_devices_into_store(store, paths, cfg)?;

    let pushed = if has_changes(&repo)? {
        let email = author_email(cfg);
        commit_all(
            &repo,
            "vaultone: cloud config sync",
            &cfg.display_name,
            &email,
        )?;
        push(&repo, &token)?;
        true
    } else {
        false
    };

    Ok(ConfigSyncOutcome {
        has_conflict: false,
        conflicts: Vec::new(),
        pushed,
        pulled_files: remote_changed,
        pricing_changed,
    })
}

/// Apply the user's per-file conflict verdicts, then pull + commit + push
/// ("pick a version", Synced-only). `choices` should cover every file
/// reported as conflicting by `sync_config`.
pub fn resolve_config_conflict(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
    choices: &[ConfigConflictResolution],
) -> AppResult<ConfigSyncOutcome> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone_for_device(&url, &paths.repo, &token, &cfg.device_id)?;
    fetch_origin(&repo, &token)?;
    let origin_oid = origin_tip_oid(&repo)?
        .ok_or_else(|| AppError::Sync("remote has no branch to resolve against".into()))?;
    let head_oid = repo
        .head()?
        .target()
        .ok_or_else(|| AppError::Sync("HEAD is detached; cannot resolve".into()))?;

    // Rewrite the worktree so the SAFE pull can fast-forward without hitting a
    // file both sides changed:
    //  - KeepRemote: write the remote blob now (== post-pull target ⇒ no-op).
    //  - KeepLocal : stash the local bytes, reset the file to HEAD (or delete if
    //    locally new) so checkout can advance; we restore the local bytes after.
    let mut local_cache: Vec<(ConfigFile, Vec<u8>)> = Vec::new();
    for r in choices {
        let file = r.file;
        let choice = r.choice;
        let rel = file.rel_path();
        let abs = paths.repo.join(rel);
        match choice {
            ConfigSyncChoice::KeepRemote => {
                if let Some(remote_bytes) = read_blob(&repo, origin_oid, rel) {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&abs, &remote_bytes)?;
                }
            }
            ConfigSyncChoice::KeepLocal => {
                let local_bytes = std::fs::read(&abs).unwrap_or_default();
                local_cache.push((file, local_bytes));
                match read_blob(&repo, head_oid, rel) {
                    Some(head_bytes) => std::fs::write(&abs, &head_bytes)?,
                    // File is locally new (untracked at HEAD): remove it so the
                    // pull can materialize the remote copy, then we overwrite.
                    None => {
                        let _ = std::fs::remove_file(&abs);
                    }
                }
            }
        }
    }

    pull_preserving_dirty(&repo, &token)?;

    // Restore local-wins files (overwrite whatever the remote just applied).
    for (file, bytes) in &local_cache {
        std::fs::write(paths.repo.join(file.rel_path()), bytes)?;
    }

    // After resolution the pricing file holds its final content (remote or
    // local version); always reload so the dashboard matches the file.
    let pricing_changed = paths.pricing_json().exists();
    if pricing_changed {
        reload_pricing_into_store(store, paths)?;
    }
    // Device-name registry rides in the same pull (config/devices_<id>.json).
    reload_devices_into_store(store, paths, cfg)?;

    let email = author_email(cfg);
    commit_all(
        &repo,
        "vaultone: config conflict resolved",
        &cfg.display_name,
        &email,
    )?;
    push(&repo, &token)?;

    Ok(ConfigSyncOutcome {
        has_conflict: false,
        conflicts: Vec::new(),
        pushed: true,
        pulled_files: Vec::new(),
        pricing_changed,
    })
}
