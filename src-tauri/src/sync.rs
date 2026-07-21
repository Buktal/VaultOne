//! GitHub-repo sync over libgit2 (ADR-0005).
//!
//! Synced-mode only (ADR-0006): the high-level entry (`ensure_repo`) refuses to
//! run unless a repo URL *and* a PAT are configured, so Standalone mode never
//! touches a remote. Auth is an in-process git2 credential callback — the
//! fine-grained PAT (ADR-0004) lives only in Rust memory; it never appears in
//! the URL, a credential helper, or an env var.
//!
//! Primitives provided here (ADR-0005 timing — startup pull / flush push /
//! periodic push / manual — is wired in S2b):
//! - `open_or_clone` — open the local repo, or clone on first use
//! - `pull`           — fetch `origin` + fast-forward (refuses to auto-merge)
//! - `commit_all`     — stage every change (add/modify/delete) + commit
//! - `push`           — push the current branch to `origin`

use std::path::Path;

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{
    Cred, FetchOptions, Index, Oid, PushOptions, RemoteCallbacks, Repository, Signature, Status,
};

use crate::config::ConfigData;
use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------------------
// Credential callback (ADR-0005: in-process PAT)
// ---------------------------------------------------------------------------

/// Build a GitHub PAT credential. GitHub accepts the fine-grained PAT as the
/// password under any username; we use the conventional `x-access-token` when
/// libgit2 does not hand us one from the URL.
fn pat_credential(username_from_url: Option<&str>, token: &str) -> Result<Cred, git2::Error> {
    let user = username_from_url.unwrap_or("x-access-token");
    Cred::userpass_plaintext(user, token)
}

/// Remote callbacks that inject the PAT, with a one-shot guard so a rejected
/// token does not loop forever (libgit2 may re-invoke the callback on auth
/// failure). git2 0.19's `RemoteCallbacks` holds a `'static` callback, so the
/// token is cloned into the closure (cheap; sync is low-frequency, ADR-0005).
// The borrowed `&str` is unrelated to the returned `RemoteCallbacks` (its
// callback is 'static), so rustc's mismatched_lifetime_syntaxes misfires here.
#[allow(mismatched_lifetime_syntaxes)]
fn build_callbacks(token: &str) -> RemoteCallbacks {
    let token = token.to_string();
    let mut attempts = 0u32;
    let mut cb = RemoteCallbacks::new();
    cb.credentials(move |_url, username_from_url, _allowed| {
        if attempts > 0 {
            return Err(git2::Error::from_str(
                "git credentials rejected: PAT invalid or expired",
            ));
        }
        attempts += 1;
        pat_credential(username_from_url, &token)
    });
    cb
}

// ---------------------------------------------------------------------------
// clone / open
// ---------------------------------------------------------------------------

/// Open the local repo at `local`, or clone it from `repo_url` on first use.
/// Idempotent: once `.git` exists, reopens instead of re-cloning.
pub fn open_or_clone(repo_url: &str, local: &Path, token: &str) -> AppResult<Repository> {
    if local.join(".git").exists() {
        return Ok(Repository::open(local)?);
    }
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(build_callbacks(token));
    let mut builder = RepoBuilder::new();
    builder.fetch_options(fo);
    let repo = builder.clone(repo_url, local)?;
    // Force LF so JSONL artifacts round-trip byte-identically across Windows /
    // POSIX (ADR-0004 deterministic interop). libgit2's platform-default text
    // conversion would otherwise flip \n ↔ \r\n and corrupt line-oriented JSONL.
    repo.config()?.set_str("core.autocrlf", "false")?;
    // The initial checkout ran under libgit2's platform-default autocrlf; under
    // the new LF policy the worktree can look "modified" vs the index until we
    // re-materialize it (force is safe — a fresh clone has no local changes).
    let mut co = CheckoutBuilder::new();
    co.force();
    repo.checkout_head(Some(&mut co))?;
    Ok(repo)
}

// ---------------------------------------------------------------------------
// pull (fetch + fast-forward)
// ---------------------------------------------------------------------------

/// Fetch `origin` and fast-forward the current branch to its tip. Refuses to
/// auto-merge divergent histories — usage data should never diverge (each
/// device writes its own `data/<deviceId>/` subtree), and config conflict
/// handling is deferred to S3 (ADR-0005).
pub fn pull(repo: &Repository, token: &str) -> AppResult<()> {
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(build_callbacks(token));
    repo.find_remote("origin")?.fetch(
        &["+refs/heads/*:refs/remotes/origin/*"],
        Some(&mut fo),
        None,
    )?;

    let mut head = repo.head()?;
    let branch = head
        .shorthand()
        .ok_or_else(|| AppError::Sync("HEAD is detached; cannot pull".into()))?;
    let upstream_ref = format!("refs/remotes/origin/{branch}");
    // Remote may not yet have this branch (first push pending) — nothing to pull.
    let upstream_oid = match repo.refname_to_id(&upstream_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(()),
    };

    let upstream = repo.find_annotated_commit(upstream_oid)?;
    let (analysis, _pref) = repo.merge_analysis(&[&upstream])?;
    if analysis.is_up_to_date() {
        return Ok(());
    }
    if !analysis.is_fast_forward() {
        return Err(AppError::Sync(format!(
            "pull would diverge on '{branch}'; refusing to auto-merge (ADR-0005 / S3)"
        )));
    }
    // Fast-forward: move the branch ref to the remote tip, then sync the tree.
    head.set_target(upstream_oid, "pull: fast-forward")?;
    let mut co = CheckoutBuilder::new();
    co.force();
    repo.checkout_head(Some(&mut co))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// commit
// ---------------------------------------------------------------------------

/// Stage every worktree change (add / modify / delete) and commit it. Supports
/// an unborn HEAD (first commit). Usage artifacts are keyed by `<deviceId>/<day>`
/// so files are only added or appended in place — never renamed — hence no
/// rename handling.
pub fn commit_all(
    repo: &Repository,
    message: &str,
    author_name: &str,
    author_email: &str,
) -> AppResult<git2::Oid> {
    let mut index = repo.index()?;
    stage_all(repo, &mut index)?;
    index.write()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = Signature::now(author_name, author_email)?;
    let oid = match repo.head() {
        Ok(head) => {
            let parent = head.peel_to_commit()?;
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?
        }
        Err(_) => repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?, // unborn HEAD
    };
    Ok(oid)
}

/// `git add -A` over the worktree: stage new + modified files, drop deleted ones.
fn stage_all(repo: &Repository, index: &mut Index) -> AppResult<()> {
    let statuses = repo.statuses(None)?;
    for entry in statuses.iter() {
        let Some(p) = entry.path() else { continue };
        let s = entry.status();
        if s.contains(Status::WT_NEW) || s.contains(Status::WT_MODIFIED) {
            index.add_path(Path::new(p))?;
        } else if s.contains(Status::WT_DELETED) {
            index.remove_path(Path::new(p))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// push
// ---------------------------------------------------------------------------

/// Push the current branch to `origin` (creating the remote branch on first push).
pub fn push(repo: &Repository, token: &str) -> AppResult<()> {
    let head = repo.head()?;
    let refname = head
        .name()
        .ok_or_else(|| AppError::Sync("HEAD has no symbolic name; cannot push".into()))?;
    let refspec = format!("{refname}:{refname}");
    let mut po = PushOptions::new();
    po.remote_callbacks(build_callbacks(token));
    repo.find_remote("origin")?
        .push(&[&refspec], Some(&mut po))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// High-level entry (Standalone guard, ADR-0006)
// ---------------------------------------------------------------------------

/// Return the configured repo URL + PAT, or an error in Standalone mode.
/// S2b command-layer callers that must be no-ops in Standalone check
/// `ConfigData::is_synced()` directly instead of erroring.
pub fn require_synced(cfg: &ConfigData) -> AppResult<(String, String)> {
    if !cfg.is_synced() {
        return Err(AppError::Sync(
            "not in Synced mode (ADR-0006): no repo URL / PAT configured".into(),
        ));
    }
    // `is_synced` guarantees both are present and non-blank.
    let url = cfg.repo_url.as_deref().unwrap().trim().to_string();
    let token = cfg.github_token.as_deref().unwrap().trim().to_string();
    Ok((url, token))
}

/// Open or clone the configured sync repo into `local`. Synced-only.
#[cfg(test)]
pub fn ensure_repo(cfg: &ConfigData, local: &Path) -> AppResult<Repository> {
    let (url, token) = require_synced(cfg)?;
    open_or_clone(&url, local, &token)
}

// ---------------------------------------------------------------------------
// High-level sync flow (ADR-0005): pull → import JSONL → commit → push
// ---------------------------------------------------------------------------

/// Outcome of one sync round, surfaced to the UI.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct SyncReport {
    /// New rows imported from the remote (uuid-deduped) this pull.
    pub imported: u32,
    /// True if a local change was committed and pushed.
    pub pushed: bool,
}

/// Deterministic commit identity for this device (ADR-0002 device-scoped).
fn author_email(cfg: &ConfigData) -> String {
    format!("{}@devices.vaultone", cfg.device_id)
}

/// Whether the worktree has any change to commit.
fn has_changes(repo: &Repository) -> AppResult<bool> {
    Ok(!repo.statuses(None)?.is_empty())
}

/// Pull the remote and import every device's JSONL Artifact into the Local
/// Store (uuid-deduped via the ledger, ADR-0005). Synced-only.
pub fn pull_and_import(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<u32> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
    pull(&repo, &token)?;
    let records = crate::ingest::read_all_artifacts(paths)?;
    let inserted = store.ingest(&records)?;
    // Per-turn durations (separate grain, uuid-deduped).
    let turns = crate::ingest::read_all_turn_artifacts(paths)?;
    store.ingest_turn_durations(&turns)?;
    Ok(inserted.len() as u32)
}

/// Commit any local Artifact/config change and push it (ADR-0005 push). A clean
/// worktree is a no-op (returns `false`). Synced-only.
pub fn commit_and_push(paths: &crate::config::Paths, cfg: &ConfigData) -> AppResult<bool> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
    if !has_changes(&repo)? {
        return Ok(false);
    }
    let email = author_email(cfg);
    commit_all(&repo, "vaultone: usage sync", &cfg.display_name, &email)?;
    push(&repo, &token)?;
    Ok(true)
}

/// Manual「立即同步」(ADR-0005): pull + import, then commit + push.
pub fn sync_now(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<SyncReport> {
    let imported = pull_and_import(store, paths, cfg)?;
    let pushed = commit_and_push(paths, cfg)?;
    Ok(SyncReport { imported, pushed })
}

// ===========================================================================
// Cloud-config sync (ADR-0005 / #6 — Synced-only, S3)
// ===========================================================================
//
// Usage artifacts live under `data/<deviceId>/` and so can never collide across
// devices — the usage path (above) fast-forwards freely. Cloud config
// (`config/{app,user,pricing}.json`) is *shared*: two devices can each edit the
// same file, and a blind pull would clobber one side. So config sync is manual
// (ADR-0005) and detects conflicts before touching the worktree:
//
//   1. fetch origin
//   2. conflict = (files dirty in the worktree) ∩ (files the remote changed
//      relative to our HEAD)
//   3. if any conflict ⇒ return it for the UI to resolve (never last-write-wins)
//   4. otherwise pull (SAFE checkout — preserves unrelated local edits) →
//      commit → push, then reload pricing into the Store if it changed.
//
// Conflict resolution rewrites the worktree so the SAFE pull can advance, then
// restores local-wins files afterward (ADR-0005 "pick a version").

/// A cloud-config file under `repo/config/` (ADR-0007). Crosses the boundary as
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

/// User's per-file verdict for a conflict (ADR-0005 "pick a version").
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

// ---- low-level helpers ----------------------------------------------------

/// Fetch `origin` into `refs/remotes/origin/*` (no merge).
fn fetch_origin(repo: &Repository, token: &str) -> AppResult<()> {
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(build_callbacks(token));
    repo.find_remote("origin")?.fetch(
        &["+refs/heads/*:refs/remotes/origin/*"],
        Some(&mut fo),
        None,
    )?;
    Ok(())
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

/// Reload the (just-pulled) cloud `pricing.json` into the Store (ADR-0007).
fn reload_pricing_into_store(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
) -> AppResult<()> {
    let path = paths.pricing_json();
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path)?;
    let entries = crate::pricing::parse_pricing_doc(&text)?;
    for e in &entries {
        store.upsert_pricing(&e.to_entry())?;
    }
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

/// Manual cloud-config sync (ADR-0005, Synced-only). Detects conflicts between
/// local worktree edits and remote changes on shared config files; if clean,
/// pulls (SAFE), commits + pushes any local change, and reloads pricing.
pub fn sync_config(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<ConfigSyncOutcome> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
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
/// (ADR-0005 "pick a version", Synced-only). `choices` should cover every file
/// reported as conflicting by `sync_config`.
pub fn resolve_config_conflict(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
    choices: &[ConfigConflictResolution],
) -> AppResult<ConfigSyncOutcome> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed a bare "remote" with one initial commit so it has a cloneable HEAD.
    fn seed_remote(remote_path: &Path) {
        Repository::init_bare(remote_path).unwrap();
        let work = tempfile::tempdir().unwrap();
        let repo = Repository::init(work.path()).unwrap();
        repo.remote("origin", &remote_path.to_string_lossy())
            .unwrap();
        std::fs::write(work.path().join("README"), "vaultone sync seed\n").unwrap();
        commit_all(&repo, "seed", "VaultOne", "seed@devices.vaultone").unwrap();
        push(&repo, "").unwrap();
    }

    /// A Synced-mode config (values are trimmed by `require_synced`).
    fn synced_cfg(repo_url: &str, github_token: &str) -> ConfigData {
        ConfigData {
            repo_url: Some(repo_url.into()),
            github_token: Some(github_token.into()),
            ..Default::default()
        }
    }

    #[test]
    fn pat_credential_builds_userpass() {
        // pat_credential is a thin wrapper over Cred::userpass_plaintext; we
        // assert it succeeds (and forwards an explicit username). git2 0.19's
        // Cred::credtype returns a raw c_int that does not compare to the
        // CredentialType constants, so we don't assert the enum here.
        assert!(pat_credential(None, "ghp_token").is_ok());
        assert!(pat_credential(Some("octocat"), "ghp_token").is_ok());
    }

    #[test]
    fn require_synced_guard() {
        // Standalone ⇒ refused.
        assert!(matches!(
            require_synced(&ConfigData::default()).unwrap_err(),
            AppError::Sync(_)
        ));

        // Synced ⇒ returns trimmed url + token.
        let (u, t) =
            require_synced(&synced_cfg("  https://github.com/x/y  ", "  ghp_t  ")).unwrap();
        assert_eq!(u, "https://github.com/x/y");
        assert_eq!(t, "ghp_t");

        // Token present but blank ⇒ Standalone.
        assert!(matches!(
            require_synced(&synced_cfg("  https://github.com/x/y  ", "   ")).unwrap_err(),
            AppError::Sync(_)
        ));
    }

    #[test]
    fn clone_sees_seeded_content_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let dest = tmp.path().join("device-b");
        let repo = open_or_clone(&url, &dest, "").unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("README"))
                .unwrap()
                .trim_end(),
            "vaultone sync seed"
        );
        drop(repo);

        // Second call reopens the existing repo (does not re-clone).
        let _repo2 = open_or_clone(&url, &dest, "").unwrap();
    }

    #[test]
    fn ensure_repo_clones_when_synced_then_opens() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        // Local file:// transport needs no auth; the token is unused but keeps
        // the config in Synced mode so the guard passes.
        let cfg = synced_cfg(&remote.to_string_lossy(), "local-no-auth");

        let dir = tmp.path().join("dev");
        let _r1 = ensure_repo(&cfg, &dir).unwrap(); // clones
        assert!(dir.join(".git").exists());
        assert!(dir.join("README").exists());
        let _r2 = ensure_repo(&cfg, &dir).unwrap(); // opens (idempotent)
    }

    #[test]
    fn ensure_repo_refuses_standalone() {
        let cfg = ConfigData::default(); // Standalone
        let tmp = tempfile::tempdir().unwrap();
        // Repository doesn't impl Debug, so match on the Result directly.
        assert!(matches!(
            ensure_repo(&cfg, tmp.path()),
            Err(AppError::Sync(_))
        ));
    }

    #[test]
    fn two_devices_sync_via_push_and_pull() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // Device A clones, writes its per-device usage artifact, commits, pushes.
        let dir_a = tmp.path().join("a");
        let repo_a = open_or_clone(&url, &dir_a, "").unwrap();
        let a_data = dir_a.join("data/dev_a");
        std::fs::create_dir_all(&a_data).unwrap();
        std::fs::write(a_data.join("usage-2026-07-16.jsonl"), "{\"uuid\":\"u1\"}\n").unwrap();
        commit_all(&repo_a, "device A usage", "DevA", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();

        // Device B clones and immediately sees A's artifact.
        let dir_b = tmp.path().join("b");
        let repo_b = open_or_clone(&url, &dir_b, "").unwrap();
        assert!(dir_b.join("data/dev_a/usage-2026-07-16.jsonl").exists());

        // A pushes a second day; B pulls and sees it (fast-forward).
        std::fs::write(a_data.join("usage-2026-07-17.jsonl"), "{\"uuid\":\"u2\"}\n").unwrap();
        commit_all(&repo_a, "device A day 2", "DevA", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        pull(&repo_b, "").unwrap();
        assert!(dir_b.join("data/dev_a/usage-2026-07-17.jsonl").exists());

        // B's local artifact (its own device subtree) survives the pull untouched.
        let b_data = dir_b.join("data/dev_b");
        std::fs::create_dir_all(&b_data).unwrap();
        std::fs::write(b_data.join("usage-2026-07-16.jsonl"), "{\"uuid\":\"b1\"}\n").unwrap();
        pull(&repo_b, "").unwrap();
        assert_eq!(
            std::fs::read_to_string(b_data.join("usage-2026-07-16.jsonl")).unwrap(),
            "{\"uuid\":\"b1\"}\n",
            "B's own untracked artifact must survive a fast-forward pull"
        );
    }

    // ---- S2b high-level flow tests (ADR-0005) ----

    fn raw_usage(uuid: &str) -> crate::providers::RawUsage {
        use crate::model::{ServerToolUse, TokenCounts};
        crate::providers::RawUsage {
            uuid: uuid.into(),
            timestamp: "2026-07-13T16:55:22.467Z".into(),
            model: "glm-5.2".into(),
            source: "claude_code".into(),
            tokens: TokenCounts {
                input: 1000,
                output: 500,
                cache_creation: 0,
                cache_read: 0,
            },
            server_tool_use: ServerToolUse::default(),
            stop_reason: "end_turn".into(),
            service_tier: "standard".into(),
            iterations: 0,
        }
    }

    #[test]
    fn pull_and_import_brings_remote_artifacts_into_store() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // Device A: clone, write a usage artifact, commit, push.
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let book = crate::pricing::seed_book();
        let rec = crate::ingest::recordify(&raw_usage("import-1"), "aabbccddeeff", &book);
        crate::ingest::append_jsonl(&paths_a, "aabbccddeeff", &[rec]).unwrap();
        commit_all(&repo_a, "A usage", "DevA", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();

        // Device B: pull_and_import into a fresh in-memory store.
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let cfg_b = synced_cfg(&url, "tok");
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let n = pull_and_import(&store, &paths_b, &cfg_b).unwrap();
        assert_eq!(n, 1, "one new record imported from A");
        let stats = store
            .query_stats(&crate::model::UsageFilter::default())
            .unwrap();
        assert_eq!(stats.request_count, 1);

        // Re-pulling is a no-op (uuid already in the ledger).
        let n2 = pull_and_import(&store, &paths_b, &cfg_b).unwrap();
        assert_eq!(n2, 0, "re-pull dedups via ledger");
    }

    #[test]
    fn commit_and_push_is_noop_when_worktree_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let paths = crate::config::Paths::resolve(&tmp.path().join("dev"));
        let cfg = synced_cfg(&url, "tok");
        // Clone ⇒ clean worktree ⇒ nothing to push.
        let _repo = open_or_clone(&url, &paths.repo, "").unwrap();
        let pushed = commit_and_push(&paths, &cfg).unwrap();
        assert!(!pushed, "clean worktree ⇒ no commit/push");
    }

    #[test]
    fn sync_now_roundtrips_usage_across_devices() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // Device A: write an artifact, then sync_now (pull no-op + commit+push).
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let cfg_a = synced_cfg(&url, "tok");
        let _repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let book = crate::pricing::seed_book();
        let rec = crate::ingest::recordify(&raw_usage("round-1"), "aabbccddeeff", &book);
        crate::ingest::append_jsonl(&paths_a, "aabbccddeeff", &[rec]).unwrap();
        let store_a = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let rep_a = sync_now(&store_a, &paths_a, &cfg_a).unwrap();
        assert!(rep_a.pushed, "A had a local change to push");
        assert_eq!(
            rep_a.imported, 1,
            "A imports its own artifact into its store"
        );

        // Device B: sync_now pulls A's artifact into B's fresh store.
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let cfg_b = synced_cfg(&url, "tok");
        let store_b = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let rep_b = sync_now(&store_b, &paths_b, &cfg_b).unwrap();
        assert_eq!(rep_b.imported, 1, "B imported A's record");
        assert!(!rep_b.pushed, "B has no local change beyond what it pulled");
        let stats = store_b
            .query_stats(&crate::model::UsageFilter::default())
            .unwrap();
        assert_eq!(stats.request_count, 1);
    }

    // ---- S3 cloud-config sync tests (ADR-0005 / #6) ----

    fn write_pricing(paths: &crate::config::Paths, body: &str) {
        let p = paths.pricing_json();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// A minimal but valid pricing doc with one entry keyed `tag`.
    fn pricing_doc(tag: &str) -> String {
        format!(
            r#"{{"models":[{{"model_key":"{tag}","display_name":"{tag}","input_per_million":1.0,"output_per_million":2.0,"cache_read_per_million":0.1,"cache_creation_per_million":1.25,"is_builtin":false}}]}}"#
        )
    }

    /// Device A: clone, commit + push an initial `pricing.json`.
    fn seed_pricing_on_a(tmp: &Path, url: &str) -> crate::config::Paths {
        let paths_a = crate::config::Paths::resolve(&tmp.join("a"));
        let repo_a = open_or_clone(url, &paths_a.repo, "").unwrap();
        write_pricing(&paths_a, &pricing_doc("base"));
        commit_all(&repo_a, "A pricing base", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        paths_a
    }

    #[test]
    fn sync_config_detects_conflict_when_both_sides_edit_pricing() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let paths_a = seed_pricing_on_a(tmp.path(), &url);

        // B clones (gets A's base pricing).
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let _repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();
        assert!(paths_b.pricing_json().exists());

        // A edits + pushes; B edits locally (dirty, uncommitted) — divergent edit.
        write_pricing(&paths_a, &pricing_doc("a-remote"));
        let repo_a = Repository::open(&paths_a.repo).unwrap();
        commit_all(&repo_a, "A pricing v2", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        write_pricing(&paths_b, &pricing_doc("b-local"));

        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg_b = synced_cfg(&url, "tok");
        let outcome = sync_config(&store, &paths_b, &cfg_b).unwrap();

        assert!(outcome.has_conflict, "both sides edited pricing ⇒ conflict");
        assert!(!outcome.pushed);
        assert_eq!(outcome.conflicts.len(), 1);
        assert_eq!(outcome.conflicts[0].file, ConfigFile::Pricing);
        assert!(outcome.conflicts[0].local_preview.contains("b-local"));
        assert!(outcome.conflicts[0].remote_preview.contains("a-remote"));
    }

    #[test]
    fn sync_config_pulls_remote_pricing_when_local_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let paths_a = seed_pricing_on_a(tmp.path(), &url);

        // B clones first (clean — gets A's base pricing).
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let _repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();

        // A pushes a newer pricing with a distinct model key.
        write_pricing(&paths_a, &pricing_doc("a-remote"));
        let repo_a = Repository::open(&paths_a.repo).unwrap();
        commit_all(&repo_a, "A pricing v2", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();

        // (B clones were clean — no local edit.)

        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg_b = synced_cfg(&url, "tok");
        let outcome = sync_config(&store, &paths_b, &cfg_b).unwrap();

        assert!(!outcome.has_conflict, "B did not edit pricing locally");
        assert!(outcome.pricing_changed, "remote pricing pulled + reloaded");
        assert!(outcome.pulled_files.contains(&ConfigFile::Pricing));
        assert!(
            std::fs::read_to_string(paths_b.pricing_json())
                .unwrap()
                .contains("a-remote"),
            "worktree now reflects the remote pricing"
        );
        // …and reloaded into the Store.
        let keys: Vec<String> = store
            .list_pricing()
            .unwrap()
            .into_iter()
            .map(|e| e.model_key)
            .collect();
        assert!(keys.contains(&"a-remote".to_string()));
    }

    #[test]
    fn resolve_config_conflict_keep_remote_takes_remote_version() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let paths_a = seed_pricing_on_a(tmp.path(), &url);
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let _repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();

        // Both sides edit pricing ⇒ conflict on B.
        write_pricing(&paths_a, &pricing_doc("a-remote"));
        let repo_a = Repository::open(&paths_a.repo).unwrap();
        commit_all(&repo_a, "A pricing v2", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        write_pricing(&paths_b, &pricing_doc("b-local"));

        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg_b = synced_cfg(&url, "tok");
        let conflict = sync_config(&store, &paths_b, &cfg_b).unwrap();
        assert!(conflict.has_conflict);

        // B keeps the remote version.
        let resolved = resolve_config_conflict(
            &store,
            &paths_b,
            &cfg_b,
            &[ConfigConflictResolution {
                file: ConfigFile::Pricing,
                choice: ConfigSyncChoice::KeepRemote,
            }],
        )
        .unwrap();
        assert!(resolved.pushed);
        assert!(
            resolved.pricing_changed,
            "remote pricing reloaded into Store"
        );

        let text = std::fs::read_to_string(paths_b.pricing_json()).unwrap();
        assert!(text.contains("a-remote"), "remote version wins locally");
        assert!(!text.contains("b-local"));
    }

    #[test]
    fn resolve_config_conflict_keep_local_pushes_local_version() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let paths_a = seed_pricing_on_a(tmp.path(), &url);
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let _repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();

        write_pricing(&paths_a, &pricing_doc("a-remote"));
        let repo_a = Repository::open(&paths_a.repo).unwrap();
        commit_all(&repo_a, "A pricing v2", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        write_pricing(&paths_b, &pricing_doc("b-local"));

        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let cfg_b = synced_cfg(&url, "tok");
        let conflict = sync_config(&store, &paths_b, &cfg_b).unwrap();
        assert!(conflict.has_conflict);

        // B keeps its local version.
        let resolved = resolve_config_conflict(
            &store,
            &paths_b,
            &cfg_b,
            &[ConfigConflictResolution {
                file: ConfigFile::Pricing,
                choice: ConfigSyncChoice::KeepLocal,
            }],
        )
        .unwrap();
        assert!(resolved.pushed);

        // Local worktree keeps the local version.
        let text = std::fs::read_to_string(paths_b.pricing_json()).unwrap();
        assert!(text.contains("b-local"), "local version wins locally");

        // …and it was pushed: a fresh clone sees the local version.
        let paths_c = crate::config::Paths::resolve(&tmp.path().join("c"));
        let _repo_c = open_or_clone(&url, &paths_c.repo, "").unwrap();
        let remote_text = std::fs::read_to_string(paths_c.pricing_json()).unwrap();
        assert!(
            remote_text.contains("b-local"),
            "local version pushed to remote"
        );
    }
}
