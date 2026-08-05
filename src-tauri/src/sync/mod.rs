//! GitHub-repo sync over libgit2.
//!
//! Synced-mode only: the high-level entry (`ensure_repo`) refuses to
//! run unless a repo URL *and* a PAT are configured, so Standalone mode never
//! touches a remote. Auth is an in-process git2 credential callback — the
//! fine-grained PAT lives only in Rust memory; it never appears in
//! the URL, a credential helper, or an env var.
//!
//! Primitives provided here (timing — startup pull / flush push /
//! periodic push / manual — is wired in S2b):
//! - `open_or_clone`    — open the local repo, or clone on first use
//! - `pull`             — fetch `origin` + fast-forward only; on divergent
//!   histories returns `Diverged` for the caller to resolve (refuses to
//!   auto-merge or push)
//! - `rebase_and_push`  — rebase local-only commits onto a given upstream tip
//!   and push (the diverge self-heal `pull` declines to do)
//! - `commit_all`       — stage every change (add/modify/delete) + commit
//! - `push`             — push the current branch to `origin`

use std::path::Path;

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{
    AnnotatedCommit, Cred, FetchOptions, Index, Oid, ProxyOptions, PushOptions, RemoteCallbacks,
    Repository, ResetType, Signature, Status,
};

use crate::config::ConfigData;
use crate::error::{AppError, AppResult};
use crate::snapshot_policy::{decide_snapshot_action, presence_mismatches, SnapshotAction};

// The remote probe (Settings「测试连接」) is an independent feature with its own
// types (`VerifyReport`) and error model (a failed probe is `ok: false`, never
// an `AppError`); see `remote_probe`. Re-exported here so the command layer
// keeps using `crate::sync::verify_remote` / `crate::sync::VerifyReport`, which
// leaves the tauri-specta binding for `verify_sync_repo` unchanged.
mod remote_probe;
pub use remote_probe::{verify_remote, VerifyReport};

// ---------------------------------------------------------------------------
// Credential callback (in-process PAT)
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
/// token is cloned into the closure (cheap; sync is low-frequency).
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

/// Declare a `FetchOptions` (named `$fo`) wired with the PAT callback AND the
/// system proxy discovered at this instant. A macro, not a function: libgit2's
/// `ProxyOptions` borrows the proxy URL by reference, so the URL must outlive
/// the options — expanding inline keeps the borrowed URL and the options in the
/// caller's scope, where the subsequent `fetch` / `clone` consumes them before
/// either can drop.
macro_rules! fetch_options_with_proxy {
    ($fo:ident, $token:expr) => {
        let mut $fo = FetchOptions::new();
        $fo.remote_callbacks(build_callbacks($token));
        let __proxy_url = crate::proxy::discover_system_proxy();
        if let Some(ref __pu) = __proxy_url {
            let mut __p = ProxyOptions::new();
            __p.url(__pu);
            $fo.proxy_options(__p);
        }
    };
}

/// Declare a `PushOptions` (named `$po`) wired with the PAT callback AND the
/// live system proxy. Same lifetime rationale as `fetch_options_with_proxy!`.
macro_rules! push_options_with_proxy {
    ($po:ident, $token:expr) => {
        let mut $po = PushOptions::new();
        $po.remote_callbacks(build_callbacks($token));
        let __proxy_url = crate::proxy::discover_system_proxy();
        if let Some(ref __pu) = __proxy_url {
            let mut __p = ProxyOptions::new();
            __p.url(__pu);
            $po.proxy_options(__p);
        }
    };
}

// ---------------------------------------------------------------------------
// clone / open
// ---------------------------------------------------------------------------

/// Default branch we bootstrap (empty remote / Standalone→Synced switch).
/// libgit2's `init` defaults to `master`; we pin `main` to match the GitHub
/// default. A non-empty remote is always followed verbatim (`pick_origin_branch`).
const DEFAULT_BRANCH: &str = "main";

/// Open the local repo at `local`, or clone it from `repo_url` on first use.
/// Idempotent: once `.git` exists, reopens instead of re-cloning.
pub fn open_or_clone(repo_url: &str, local: &Path, token: &str) -> AppResult<Repository> {
    open_or_clone_impl(repo_url, local, token)
}

fn open_or_clone_impl(repo_url: &str, local: &Path, token: &str) -> AppResult<Repository> {
    if local.join(".git").exists() {
        return Ok(Repository::open(local)?);
    }
    // Standalone collects write JSONL artifacts into `local/data/`.
    // When the user later switches to Synced, `local` is non-empty but has no
    // `.git`, and libgit2's `clone` (which demands an empty target) fails with
    // "exists and is not an empty directory". Detect that and bootstrap the repo
    // in place instead — preserving the locally-collected artifacts.
    let dir_has_entries = local
        .read_dir()
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);
    if dir_has_entries {
        return init_with_remote(repo_url, local, token);
    }
    fetch_options_with_proxy!(fo, token);
    let mut builder = RepoBuilder::new();
    builder.fetch_options(fo);
    let repo = builder.clone(repo_url, local)?;
    // Force LF so JSONL artifacts round-trip byte-identically across Windows /
    // POSIX (deterministic interop). libgit2's platform-default text
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

/// Drop the local `.git` so a fresh re-bind starts clean (used by
/// `clear_sync_repo`). `data/` and `config/` are preserved — Standalone keeps
/// writing artifacts to `data/`, and they carry no per-repo identity. Only
/// `.git` pins the worktree to the old remote + branch; the DB is the source of
/// truth for usage rows, so this loses git history, never data. Best-effort: a
/// removal failure is logged, not fatal (the unbind's primary effect — clearing
/// the config — already succeeded).
pub fn reset_local_git(repo: &Path) {
    let dot_git = repo.join(".git");
    if dot_git.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dot_git) {
            eprintln!("[vaultone] reset_local_git: failed to remove .git: {e}");
        }
    }
}

/// Bootstrap a sync repo inside an already-populated `local` — the Standalone →
/// Synced switch, or the unbind→re-bind case. `clone` refuses a non-empty target,
/// so init in place, fetch the remote, and force-checkout the remote tip. Force
/// is safe even though it may overwrite this device's own `data/<deviceId>/`
/// files: collect writes the store, not the Artifact, so unpushed rows live in
/// SQLite (flagged in `dirty_days`) and the next push recomputes this device's
/// files from the store. No snapshot/restore is needed — the store is the
/// source of truth.
fn init_with_remote(repo_url: &str, local: &Path, token: &str) -> AppResult<Repository> {
    let repo = Repository::init(local)?;
    repo.config()?.set_str("core.autocrlf", "false")?;
    {
        let mut remote = repo.remote("origin", repo_url)?;
        fetch_options_with_proxy!(fo, token);
        remote.fetch(
            &["+refs/heads/*:refs/remotes/origin/*"],
            Some(&mut fo),
            None,
        )?;
    }
    // Point HEAD at the remote's default branch and force-checkout its tree. If
    // the remote is unborn (empty repo) there is nothing to check out — pin HEAD
    // at our `main` (unborn) so the first commit+push creates `main`, not
    // libgit2's hardcoded `master`. Force: the worktree may already hold files
    // the remote also carries (the unbind→re-bind case — `.git` was dropped but
    // `data/` remains, so those files are now untracked and a SAFE checkout
    // rejects them as conflicts). Overwriting this device's own (possibly
    // staler) files is fine — see the doc comment above: push recomputes them.
    if let Some((branch, tip)) = pick_origin_branch(&repo)? {
        let commit = repo.find_commit(tip)?;
        repo.branch(&branch, &commit, true)?;
        repo.set_head(&format!("refs/heads/{branch}"))?;
        let mut co = CheckoutBuilder::new();
        co.force();
        repo.checkout_head(Some(&mut co))?;
    } else {
        // Empty remote: libgit2's init default (`master`) would otherwise win.
        // Pin to our `main` as an unborn HEAD; the first commit lands on it.
        repo.set_head(&format!("refs/heads/{DEFAULT_BRANCH}"))?;
    }
    Ok(repo)
}

/// Resolve the remote's default branch + tip. `clone` records `origin/HEAD`, but
/// an in-place init+fetch does not, so prefer `main`, then `master`, then any
/// remote branch. `None` when the remote carries no branches yet (unborn).
fn pick_origin_branch(repo: &Repository) -> AppResult<Option<(String, Oid)>> {
    for name in ["main", "master"] {
        if let Ok(oid) = repo.refname_to_id(&format!("refs/remotes/origin/{name}")) {
            return Ok(Some((name.to_string(), oid)));
        }
    }
    for item in repo.branches(Some(git2::BranchType::Remote))? {
        let (branch, _) = item?;
        let raw = branch.name_bytes()?;
        let s = String::from_utf8_lossy(raw);
        if let Some(rest) = s.strip_prefix("origin/") {
            if let Ok(oid) = repo.refname_to_id(&format!("refs/remotes/origin/{rest}")) {
                return Ok(Some((rest.to_string(), oid)));
            }
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// pull (fetch + fast-forward) + rebase_and_push (diverge self-heal)
// ---------------------------------------------------------------------------

/// Outcome of [`pull`]. `pull` is fetch + fast-forward only — it never rebases
/// or pushes. When the local and remote histories have diverged (a lost push
/// race — another device pushed between our last pull and push) it does NOT
/// mutate, returning [`PullOutcome::Diverged`] with the upstream tip so the
/// caller can resolve it explicitly with [`rebase_and_push`]. This keeps `pull`
/// honest about its name: no hidden rebase, no hidden push.
pub enum PullOutcome<'a> {
    /// Local is already at the remote tip — or there is no branch/upstream to
    /// advance (unborn HEAD, first push pending). No mutation.
    UpToDate,
    /// Local branch was fast-forwarded to the remote tip and the worktree
    /// synced to it.
    FastForwarded,
    /// Histories diverged. `pull` did nothing; the caller decides whether to
    /// rebase + push via [`rebase_and_push`].
    Diverged(AnnotatedCommit<'a>),
}

/// Fetch `origin` and advance the current branch to its tip when possible.
/// Fast-forwards when it can; returns [`PullOutcome::Diverged`] WITHOUT
/// mutating when the local branch has commits the remote doesn't (a lost push
/// race). The caller — typically [`pull_and_import`] — then resolves the
/// diverge explicitly with [`rebase_and_push`]. Device isolation
/// (`data/<deviceId>/`) means a local-only commit only touches files the remote
/// didn't, so that rebase applies without conflict. Usage artifacts are the
/// only thing in the repo and they are per-device isolated (`data/<deviceId>/`),
/// so no shared file two devices could diverge on.
pub fn pull<'a>(repo: &'a Repository, token: &str) -> AppResult<PullOutcome<'a>> {
    // Unborn HEAD (fresh init, first commit still pending): no local branch to
    // fast-forward, so there is nothing to pull — the first commit+push creates
    // the branch. Covers the Standalone→Synced switch against an empty remote,
    // where `head()` would otherwise error on the missing HEAD ref.
    let mut head = match repo.head() {
        Ok(h) => h,
        Err(ref e) if e.code() == git2::ErrorCode::UnbornBranch => {
            return Ok(PullOutcome::UpToDate)
        }
        Err(e) => return Err(e.into()),
    };
    fetch_options_with_proxy!(fo, token);
    repo.find_remote("origin")?.fetch(
        &["+refs/heads/*:refs/remotes/origin/*"],
        Some(&mut fo),
        None,
    )?;
    let branch = head
        .shorthand()
        .ok_or_else(|| AppError::Sync("HEAD is detached; cannot pull".into()))?;
    let upstream_ref = format!("refs/remotes/origin/{branch}");
    // Remote may not yet have this branch (first push pending) — nothing to pull.
    let upstream_oid = match repo.refname_to_id(&upstream_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(PullOutcome::UpToDate),
    };

    let upstream = repo.find_annotated_commit(upstream_oid)?;
    let (analysis, _pref) = repo.merge_analysis(&[&upstream])?;
    if analysis.is_up_to_date() {
        return Ok(PullOutcome::UpToDate);
    }
    if !analysis.is_fast_forward() {
        // Diverged: surface the upstream tip only. `pull` declines to rebase/push
        // — the caller resolves it via `rebase_and_push`.
        return Ok(PullOutcome::Diverged(upstream));
    }
    // Fast-forward: move the branch ref to the remote tip, then sync the tree.
    head.set_target(upstream_oid, "pull: fast-forward")?;
    let mut co = CheckoutBuilder::new();
    co.force();
    repo.checkout_head(Some(&mut co))?;
    Ok(PullOutcome::FastForwarded)
}

/// Rebase this branch's local-only commits onto `upstream` and push. The
/// explicit diverge step [`pull`] declines to do — [`pull_and_import`]
/// invokes it when [`pull`] returns [`PullOutcome::Diverged`].
///
/// `git rebase` needs a clean worktree, so any in-worktree change here is
/// hard-reset away before rebasing. That is safe: usage rows live in the store
/// (not the worktree Artifacts), and the next push recomputes this device's
/// files from it — the worktree is always regenerable. Device isolation
/// (`data/<deviceId>/`) guarantees the rebase applies without conflict. A
/// commit whose diff is already on the upstream tip (e.g. a device-cleanup a
/// peer pushed first) is dropped as already-applied; any other failure is a
/// real conflict, so we abort and surface it instead of silently merging, and
/// the caller reports it as a plain failed sync.
///
/// `author_name` / `author_email` are this device's commit identity, reused as
/// the rebaser's signature (authors of replayed commits are preserved). Synced
/// callers pass `cfg.display_name` / `author_email(cfg)`.
pub(crate) fn rebase_and_push(
    repo: &Repository,
    upstream: &AnnotatedCommit,
    token: &str,
    author_name: &str,
    author_email: &str,
) -> AppResult<()> {
    if has_changes(repo)? {
        let head_oid = repo
            .head()?
            .target()
            .ok_or_else(|| AppError::Sync("HEAD has no target; cannot rebase".into()))?;
        let head_obj = repo.find_object(head_oid, None)?;
        repo.reset(&head_obj, ResetType::Hard, None)?;
    }
    let committer = Signature::now(author_name, author_email)?;
    let mut rebase = repo.rebase(None, Some(upstream), Some(upstream), None)?;
    while let Some(op) = rebase.next() {
        op?;
        match rebase.commit(None, &committer, None) {
            Ok(_) => {}
            // This commit's diff is already on the upstream tip — e.g. a
            // device-cleanup a peer pushed first — so libgit2 reports it as
            // already-applied. Drop it and keep rebasing; the device-isolation
            // layout means the surviving commits apply cleanly, so any other
            // error here is a real conflict we refuse to auto-merge.
            Err(ref e) if e.code() == git2::ErrorCode::Applied => continue,
            Err(e) => {
                let _ = rebase.abort();
                return Err(AppError::Sync(format!(
                    "rebase onto remote tip would conflict; refusing to auto-merge: {e}"
                )));
            }
        }
    }
    rebase.finish(Some(&committer))?;
    push(repo, token)?;
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
    push_options_with_proxy!(po, token);
    repo.find_remote("origin")?
        .push(&[&refspec], Some(&mut po))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// High-level entry (Standalone guard)
// ---------------------------------------------------------------------------

/// Return the configured repo URL + PAT, or an error in Standalone mode.
/// S2b command-layer callers that must be no-ops in Standalone check
/// `ConfigData::is_synced()` directly instead of erroring.
pub fn require_synced(cfg: &ConfigData) -> AppResult<(String, String)> {
    if !cfg.is_synced() {
        return Err(AppError::Sync(
            "not in Synced mode: no repo URL / PAT configured".into(),
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
// High-level sync flow: pull → import JSONL → commit → push
// ---------------------------------------------------------------------------

/// Deterministic commit identity for this device (device-scoped).
pub(crate) fn author_email(cfg: &ConfigData) -> String {
    format!("{}@devices.vaultone", cfg.device_id)
}

/// Whether the worktree has any change to commit.
pub(crate) fn has_changes(repo: &Repository) -> AppResult<bool> {
    Ok(!repo.statuses(None)?.is_empty())
}

/// Whether the local branch has commits the remote tip lacks — the state a
/// failed push leaves behind: the commit landed locally, the worktree is clean,
/// and `has_changes` alone would no-op the retry forever. An unborn HEAD or a
/// never-fetched remote ref is conservatively "not ahead" (there is nothing
/// pushable either way).
fn is_ahead_of_origin(repo: &Repository) -> AppResult<bool> {
    let Ok(head) = repo.head() else {
        return Ok(false); // unborn HEAD: nothing to be ahead with
    };
    let local = head.peel_to_commit()?;
    let remote_ref = format!(
        "refs/remotes/origin/{}",
        head.shorthand().unwrap_or("master")
    );
    let remote = match repo.find_reference(&remote_ref) {
        Ok(r) => r.peel_to_commit()?,
        Err(_) => return Ok(false),
    };
    let (ahead, _behind) = repo.graph_ahead_behind(local.id(), remote.id())?;
    Ok(ahead > 0)
}

/// Pull the remote and import every device's JSONL Artifact into the Local
/// Store (deduped by the store's `(uuid, device_id)` primary key). Synced-only.
///
/// A fast-forward force-checkout (or a diverge rebase's hard reset) may rewrite
/// this device's own `data/<deviceId>/` files — fine: collect writes the store,
/// not the Artifact, so unpushed rows live in SQLite (flagged in `dirty_days`)
/// and the next push recomputes this device's files from the store. The old
/// snapshot/restore mechanism existed only to protect collect appends.
pub fn pull_and_import(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<u32> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
    // Two-step sync: pull (fetch + fast-forward), then — only on diverge —
    // rebase local-only commits onto the remote tip and push.
    match pull(&repo, &token)? {
        PullOutcome::Diverged(upstream) => {
            rebase_and_push(
                &repo,
                &upstream,
                &token,
                &cfg.display_name,
                &author_email(cfg),
            )?;
        }
        PullOutcome::UpToDate | PullOutcome::FastForwarded => {}
    }
    let records = crate::ingest::read_all_artifacts(paths)?;
    let inserted = store.ingest(&records)?;
    // Per-turn durations (separate grain, uuid-deduped).
    let turns = crate::ingest::read_all_turn_artifacts(paths)?;
    store.ingest_turn_durations(&turns)?;
    // Sessions: import peers' snapshots (self is local-authoritative, skipped
    // on read) and propagate cross-device un-favorites.
    import_peer_sessions(store, paths, &cfg.device_id)?;
    // Device-name registry: pull may have added/updated config/devices/*.json.
    crate::devices::reload_devices_into_store(store, paths, cfg)?;
    Ok(inserted.len() as u32)
}

/// Import peers' session snapshots into the store and propagate cross-device
/// un-favorites. Self's own snapshots are skipped on read
/// ([`crate::ingest::read_all_session_snapshots`]), so self's rows are never
/// overwritten by a possibly-stale git copy of itself. For every peer that has
/// (or had) a favorited session row, sessions whose snapshot file vanished since
/// the last pull are un-favorited and their shared messages dropped — the
/// pull-side counterpart to the push-side jsonl deletion.
fn import_peer_sessions(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    self_device_id: &str,
) -> AppResult<()> {
    let snapshots = crate::ingest::read_all_session_snapshots(paths, self_device_id)?;
    // still-favorited ids per peer = the snapshot files that exist this pull.
    let mut per_device: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for snap in &snapshots {
        per_device
            .entry(snap.device_id.clone())
            .or_default()
            .insert(snap.meta.id.clone());
        store.import_session_snapshot(&snap.device_id, &snap.meta, &snap.messages)?;
    }
    // Reconcile every peer with a favorited row — including ones that shipped
    // no files this pull (they may have un-favorited everything). The sessions
    // to un-favorite here = the peer's favorited sessions whose snapshot file
    // vanished, computed by the shared snapshot_policy oracle so push and pull
    // agree on what "in sync" means (the push path enforces the same invariant
    // for this device via `decide_snapshot_action`).
    for peer in store.favorited_session_devices(self_device_id)? {
        let still_present = per_device.remove(&peer).unwrap_or_default();
        let peer_favorited: std::collections::BTreeSet<String> =
            store.favorited_session_ids(&peer)?.into_iter().collect();
        let to_unfavorite =
            presence_mismatches(&still_present, &peer_favorited).favorites_without_files;
        store.bulk_unfavorite_sessions(&peer, &to_unfavorite)?;
    }
    Ok(())
}

/// Commit any local Artifact/config change and push it (push). A clean worktree
/// AND no commits ahead of origin is a no-op (returns `false`). `message` is
/// the commit body — pass the semantic of the change so the log reads
/// "vaultone: usage sync" vs "vaultone: library sync". Errors propagate; for
/// daemon/exit paths that must not bubble, use [`commit_and_push_best_effort`].
/// Synced-only.
///
/// The "ahead of origin" half matters for retry: if a previous push failed
/// after its commit landed, the worktree is clean but the local branch is
/// ahead — skipping the push there would strand the commit until an unrelated
/// change re-dirtied the worktree.
pub fn commit_and_push(
    paths: &crate::config::Paths,
    cfg: &ConfigData,
    message: &str,
) -> AppResult<bool> {
    let (url, token) = require_synced(cfg)?;
    let repo = open_or_clone(&url, &paths.repo, &token)?;
    let changed = has_changes(&repo)?;
    let ahead = is_ahead_of_origin(&repo)?;
    if !changed && !ahead {
        return Ok(false);
    }
    if changed {
        let email = author_email(cfg);
        commit_all(&repo, message, &cfg.display_name, &email)?;
    }
    push(&repo, &token)?;
    Ok(true)
}

/// Best-effort commit + push for background/exit paths. Standalone is a no-op;
/// a push failure is logged, never propagated — the next collect/sync round
/// carries the change up. The one caller that needs the error surfaced (manual
/// 「立即同步」) calls [`commit_and_push`] directly.
pub fn commit_and_push_best_effort(paths: &crate::config::Paths, cfg: &ConfigData, message: &str) {
    if !cfg.is_synced() {
        return;
    }
    if let Err(e) = commit_and_push(paths, cfg, message) {
        eprintln!("[vaultone] push failed: {e}");
    }
}

/// Sync push: materialize this device's un-pushed days AND session snapshots
/// from the store, then commit + push, clearing the dirty flags only once the
/// push lands. This is the push-side counterpart to collect's store-only
/// writes: collect flags days/sessions dirty; this recomputes each dirty day's
/// per-day Artifact (`recompute_usage_day` / `recompute_turns_day`) and each
/// dirty session's jsonl snapshot (`recompute_session_snapshot`), commits the
/// rewritten files, pushes, and on success clears the flags (a failed push
/// leaves them dirty for the next retry). Synced-only; a no-op (`false`) when
/// there is nothing dirty to recompute and nothing else to push.
///
/// The session favorites gate lives HERE (not in collect): a favorited dirty
/// session gets its snapshot rewritten; a non-favorited dirty session gets any
/// leftover `sessions/<id>.jsonl` removed — the local half of un-favorite
/// propagation (a peer pulling sees the file vanish). The clear is scoped to
/// recompute-time row/message counts so a raced new row/message keeps its
/// day/session dirty (see [`crate::db::Store::clear_dirty_days_if_unchanged`] /
/// [`crate::db::Store::clear_dirty_sessions_if_unchanged`]).
///
/// Library sync does NOT call this — it has no store/dirty concern and uses
/// [`commit_and_push`] directly.
pub fn push_usage(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) -> AppResult<bool> {
    let dirty = store.dirty_days()?;
    // (day, usage row count, turn row count) at recompute time — the clear
    // boundary: rows that land AFTER these snapshots must keep their day dirty.
    let mut day_snapshots: Vec<(String, usize, usize)> = Vec::with_capacity(dirty.len());
    for day in &dirty {
        let usage = crate::ingest::recompute_usage_day(store, paths, &cfg.device_id, day)?;
        let turns = crate::ingest::recompute_turns_day(store, paths, &cfg.device_id, day)?;
        day_snapshots.push((day.clone(), usage, turns));
    }

    // Sessions: recompute a derived jsonl per favorited dirty session; delete
    // any leftover jsonl for non-favorited dirty sessions (un-favorite local).
    let dirty_sessions = store.dirty_sessions()?;
    let mut recomputed: Vec<(String, usize)> = Vec::with_capacity(dirty_sessions.len());
    let mut removed: Vec<String> = Vec::new();
    for sid in &dirty_sessions {
        let favorited = store
            .get_session_favorited(&cfg.device_id, sid)?
            .unwrap_or(false);
        match decide_snapshot_action(favorited) {
            // favorited ⇒ the snapshot must exist: recompute it from the store.
            SnapshotAction::Write => {
                let count =
                    crate::ingest::recompute_session_snapshot(store, paths, &cfg.device_id, sid)?;
                recomputed.push((sid.clone(), count));
            }
            // not favorited ⇒ the snapshot must not exist. Idempotent: a
            // never-favorited session has no file to remove.
            SnapshotAction::Remove => {
                let path = crate::ingest::transcript_path(paths, &cfg.device_id, sid);
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
                removed.push(sid.clone());
            }
        }
    }

    let pushed = commit_and_push(paths, cfg, "vaultone: sync")?;
    if pushed {
        // Push landed ⇒ the recomputed days/sessions are on the remote; drop
        // them so the next push only touches things with fresh local changes.
        // A push failure returns early via `?` above, leaving flags dirty.
        store.clear_dirty_days_if_unchanged(&day_snapshots, &cfg.device_id)?;
        store.clear_dirty_sessions_if_unchanged(&recomputed, &cfg.device_id, &removed)?;
    }
    Ok(pushed)
}

/// Best-effort [`push_usage`] for the exit flush. Standalone is a no-op; a push
/// failure is logged, never propagated.
pub fn push_usage_best_effort(
    store: &crate::db::Store,
    paths: &crate::config::Paths,
    cfg: &ConfigData,
) {
    if !cfg.is_synced() {
        return;
    }
    if let Err(e) = push_usage(store, paths, cfg) {
        eprintln!("[vaultone] usage push failed: {e}");
    }
}

/// Seed a bare "remote" with one initial commit so it has a cloneable HEAD.
/// Module-level (not inside `mod tests`) and `pub(crate)` so the sibling test
/// module `sync::remote_probe::tests` can build a `file://` remote without
/// duplicating the fixture. Compiled only under `cfg(test)`.
#[cfg(test)]
pub(crate) fn seed_remote(remote_path: &Path) {
    Repository::init_bare(remote_path).unwrap();
    let work = tempfile::tempdir().unwrap();
    let repo = Repository::init(work.path()).unwrap();
    repo.remote("origin", &remote_path.to_string_lossy())
        .unwrap();
    std::fs::write(work.path().join("README"), "vaultone sync seed\n").unwrap();
    commit_all(&repo, "seed", "VaultOne", "seed@devices.vaultone").unwrap();
    push(&repo, "").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Standalone → Synced switch: `local` already holds collected artifacts and no
    /// `.git`. `init_with_remote` must bootstrap the repo, pull the remote, and
    /// keep the local data intact.
    #[test]
    fn init_with_remote_preserves_local_data_and_pulls_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        let local = tmp.path().join("device");
        let local_data = local.join("data").join("localdev");
        std::fs::create_dir_all(&local_data).unwrap();
        std::fs::write(
            local_data.join("usage-2026-07-22.jsonl"),
            "{\"uuid\":\"local-1\"}\n",
        )
        .unwrap();

        let repo = init_with_remote(&url, &local, "").unwrap();
        assert!(local.join(".git").exists());
        // Local artifact survives the SAFE checkout (untracked, not clobbered).
        assert!(local_data.join("usage-2026-07-22.jsonl").exists());
        // Remote content landed (seed_remote committed a README).
        assert!(local.join("README").exists());
        drop(repo);
    }

    #[test]
    fn init_with_remote_handles_unborn_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        Repository::init_bare(&remote).unwrap(); // unborn — no commits
        let url = remote.to_string_lossy().to_string();

        let local = tmp.path().join("device");
        let local_data = local.join("data").join("localdev");
        std::fs::create_dir_all(&local_data).unwrap();
        std::fs::write(local_data.join("usage.jsonl"), "{}\n").unwrap();

        // No branches on the remote ⇒ no checkout, but local data survives + repo
        // is init'd (first commit+push will create the branch).
        let repo = init_with_remote(&url, &local, "").unwrap();
        assert!(local.join(".git").exists());
        assert!(local_data.join("usage.jsonl").exists());
        drop(repo);
    }

    /// Against an empty remote the bootstrapped repo has an unborn HEAD; `pull`
    /// must short-circuit instead of erroring on the missing HEAD ref.
    #[test]
    fn pull_is_noop_on_unborn_head() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        Repository::init_bare(&remote).unwrap();
        let url = remote.to_string_lossy().to_string();
        let local = tmp.path().join("dev");
        let repo = init_with_remote(&url, &local, "").unwrap();
        assert!(
            matches!(pull(&repo, "").unwrap(), PullOutcome::UpToDate),
            "unborn HEAD must short-circuit as UpToDate"
        );
    }

    /// End-to-end Standalone→Synced against an EMPTY remote: `local` already holds
    /// collected artifacts and no `.git`. open_or_clone bootstraps in place, the
    /// first commit+push creates the branch and ships the local data upstream.
    #[test]
    fn open_or_clone_then_push_ships_local_data_into_empty_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        Repository::init_bare(&remote).unwrap(); // empty — unborn
                                                 // GitHub's empty repos default to `main`; mirror that so the bare HEAD
                                                 // lines up with the branch we push (libgit2's init_bare defaults `master`).
        std::fs::write(remote.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let url = remote.to_string_lossy().to_string();

        let local = tmp.path().join("device");
        let artifact = local
            .join("data")
            .join("localdev")
            .join("usage-2026-07-22.jsonl");
        std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        std::fs::write(&artifact, "{\"uuid\":\"local-1\"}\n").unwrap();

        // Non-empty `local`, no `.git` ⇒ init_with_remote (unborn HEAD).
        let repo = open_or_clone(&url, &local, "").unwrap();
        commit_all(&repo, "first sync", "DevA", "a@devices.vaultone").unwrap();
        push(&repo, "").unwrap();

        // The first push creates our pinned default branch `main`, not master.
        let bare = Repository::open_bare(&remote).unwrap();
        assert!(
            bare.refname_to_id("refs/heads/main").is_ok(),
            "first push must create `main`, not libgit2's default `master`"
        );

        // A fresh clone now sees the local artifact on the remote.
        let check = tmp.path().join("check");
        let _r2 = open_or_clone(&url, &check, "").unwrap();
        assert!(check.join("data/localdev/usage-2026-07-22.jsonl").exists());
    }

    /// `clear_sync_repo` drops `.git` so a re-bind starts clean, but must leave
    /// the per-device usage artifacts (`data/`) intact — Standalone keeps
    /// writing there and they are not git state.
    #[test]
    fn reset_local_git_removes_dot_git_but_keeps_data() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("data/dev")).unwrap();
        std::fs::write(repo.join("data/dev/usage.jsonl"), "{}\n").unwrap();
        Repository::init(&repo).unwrap();
        assert!(repo.join(".git").exists());

        reset_local_git(&repo);

        assert!(!repo.join(".git").exists(), "clear must drop .git");
        assert!(
            repo.join("data/dev/usage.jsonl").exists(),
            "usage artifacts must survive a clear"
        );
    }

    /// Unbind (`reset_local_git`) then re-bind the SAME repo: locally-collected
    /// `data/` survives, the remote history is re-fetched, and a fresh
    /// collect+push round-trips. Proves clearing `.git` on unbind loses nothing
    /// when the user re-binds the very same repo.
    #[test]
    fn rebind_same_repo_after_reset_keeps_data_and_resyncs() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // Bind, collect own data, push.
        let local = tmp.path().join("device");
        let repo = open_or_clone(&url, &local, "").unwrap();
        let own = local.join("data/localdev");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("usage-2026-07-22.jsonl"), "{\"uuid\":\"x\"}\n").unwrap();
        commit_all(&repo, "collect", "Dev", "d@devices.vaultone").unwrap();
        push(&repo, "").unwrap();
        drop(repo);

        // Unbind: `.git` dropped, `data/` kept.
        reset_local_git(&local);
        assert!(!local.join(".git").exists());
        assert!(own.join("usage-2026-07-22.jsonl").exists());

        // Re-bind the same repo: re-init + fetch + SAFE checkout.
        let repo2 = open_or_clone(&url, &local, "").unwrap();
        assert!(
            own.join("usage-2026-07-22.jsonl").exists(),
            "own data survives rebind"
        );
        assert!(local.join("README").exists(), "remote content re-lands");

        // A new collect after rebind commits + pushes cleanly.
        std::fs::write(own.join("usage-2026-07-23.jsonl"), "{\"uuid\":\"y\"}\n").unwrap();
        commit_all(&repo2, "collect 2", "Dev", "d@devices.vaultone").unwrap();
        push(&repo2, "").unwrap();

        // A fresh device sees both days.
        let check = tmp.path().join("check");
        let _r3 = open_or_clone(&url, &check, "").unwrap();
        assert!(check.join("data/localdev/usage-2026-07-22.jsonl").exists());
        assert!(check.join("data/localdev/usage-2026-07-23.jsonl").exists());
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

    // ---- S2b high-level flow tests ----

    fn raw_usage(uuid: &str) -> crate::providers::RawUsage {
        use crate::model::{ServerToolUse, TokenCounts};
        crate::providers::RawUsage {
            uuid: uuid.into(),
            timestamp: "2026-07-13T16:55:22.467Z".into(),
            model: "glm-5.2".into(),
            source: "claude_code".into(),
            session_id: String::new(),
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

        // Re-pulling is a no-op (uuid already in the store).
        let n2 = pull_and_import(&store, &paths_b, &cfg_b).unwrap();
        assert_eq!(n2, 0, "re-pull dedups via the store's primary key");
    }

    /// Regression: `pull` used to be fast-forward-only and errored on divergent
    /// histories ("pull would diverge on 'main'; refusing to auto-merge") — the
    /// exact state a lost push race leaves a device in, with no way out. pull now
    /// rebases local-only commits onto the remote tip and pushes, so BOTH
    /// devices' data survive on the remote (a soft/reset-only fix would replay
    /// the local tree verbatim and silently clobber the peer's data).
    #[test]
    fn pull_rebases_diverged_local_commits_onto_remote_tip() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // A: baseline under its own data dir + push (remote = A1).
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let a_file = paths_a
            .repo
            .join("data/aaaaaaaaaaaa/usage-2026-07-30.jsonl");
        std::fs::create_dir_all(a_file.parent().unwrap()).unwrap();
        std::fs::write(&a_file, "a-1\n").unwrap();
        commit_all(&repo_a, "A1", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();

        // Peer B pushes under its OWN data dir (remote = B1 on A1).
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();
        let b_file = paths_b
            .repo
            .join("data/bbbbbbbbbbbb/usage-2026-07-30.jsonl");
        std::fs::create_dir_all(b_file.parent().unwrap()).unwrap();
        std::fs::write(&b_file, "b-1\n").unwrap();
        commit_all(&repo_b, "B1", "B", "b@devices.vaultone").unwrap();
        push(&repo_b, "").unwrap();
        drop(repo_b);

        // A commits a second local-only change WITHOUT pushing ⇒ diverge.
        std::fs::write(&a_file, "a-1\na-2\n").unwrap();
        commit_all(&repo_a, "A2", "A", "a@devices.vaultone").unwrap();

        // pull surfaces the diverge (does NOT rebase/push itself); rebase_and_push
        // self-heals — rebases A2 onto B1 and pushes — as an explicit step.
        let outcome = pull(&repo_a, "").unwrap();
        let upstream = match outcome {
            PullOutcome::Diverged(u) => u,
            _ => panic!("expected PullOutcome::Diverged after A's local-only commit"),
        };
        rebase_and_push(&repo_a, &upstream, "", "A", "a@devices.vaultone").unwrap();

        // A fresh clone sees BOTH devices' data — A's local-only a-2 change
        // landed on top of B1 without clobbering B (a soft/reset-only fix would
        // replay A's tree verbatim and B's data would vanish from the remote).
        let paths_c = crate::config::Paths::resolve(&tmp.path().join("c"));
        let _repo_c = open_or_clone(&url, &paths_c.repo, "").unwrap();
        let a_text = std::fs::read_to_string(
            paths_c
                .repo
                .join("data/aaaaaaaaaaaa/usage-2026-07-30.jsonl"),
        )
        .unwrap();
        assert!(
            a_text.contains("a-2"),
            "A's local-only change reached the remote: {a_text}"
        );
        let b_text = std::fs::read_to_string(
            paths_c
                .repo
                .join("data/bbbbbbbbbbbb/usage-2026-07-30.jsonl"),
        )
        .unwrap();
        assert!(
            b_text.contains("b-1"),
            "B's data survived the rebase: {b_text}"
        );
    }

    /// Regression: when a device's local commit duplicates a patch a peer
    /// already pushed (e.g. the same device-cleanup run on two machines),
    /// rebase reports the local copy as "already applied". pull must drop it
    /// and keep rebasing instead of aborting the whole sync — the stuck
    /// diverge 1.5.0 hit on Ubuntu when a device-cleanup landed on the remote
    /// first.
    #[test]
    fn pull_rebase_skips_local_commit_whose_patch_is_already_upstream() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let rel_a = "data/aaaaaaaaaaaa/usage-2026-07-30.jsonl";
        let rel_b = "data/bbbbbbbbbbbb/usage-2026-07-30.jsonl";

        // A writes one file under its data dir and pushes (remote = A1).
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let a_file = paths_a.repo.join(rel_a);
        std::fs::create_dir_all(a_file.parent().unwrap()).unwrap();
        std::fs::write(&a_file, "a-1\n").unwrap();
        commit_all(&repo_a, "A1", "A", "a@devices.vaultone").unwrap();
        push(&repo_a, "").unwrap();
        drop(repo_a);

        // B clones (sees A1) then rewinds to the seed base — simulating B
        // never pulling A1 and independently making the SAME change A1 did.
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();
        let head_b = repo_b.head().unwrap().peel_to_commit().unwrap();
        let base = head_b.parents().next().unwrap();
        repo_b
            .reset(
                &repo_b.find_object(base.id(), None).unwrap(),
                ResetType::Hard,
                None,
            )
            .unwrap();
        // B_dup: an identical patch to A1 (same file, same contents).
        let a_file_b = paths_b.repo.join(rel_a);
        std::fs::create_dir_all(a_file_b.parent().unwrap()).unwrap();
        std::fs::write(&a_file_b, "a-1\n").unwrap();
        commit_all(&repo_b, "B dup of A1", "B", "b@devices.vaultone").unwrap();
        // B_unique: B's own data, not yet on the remote.
        let b_file = paths_b.repo.join(rel_b);
        std::fs::create_dir_all(b_file.parent().unwrap()).unwrap();
        std::fs::write(&b_file, "b-1\n").unwrap();
        commit_all(&repo_b, "B unique", "B", "b@devices.vaultone").unwrap();

        // pull surfaces the diverge (does NOT rebase/push itself); rebase_and_push
        // self-heals — drops B_dup (patch == A1, already upstream), rebases
        // B_unique onto A1, and pushes — as an explicit step.
        let outcome = pull(&repo_b, "").unwrap();
        let upstream = match outcome {
            PullOutcome::Diverged(u) => u,
            _ => panic!("expected PullOutcome::Diverged after B's local-only commits"),
        };
        rebase_and_push(&repo_b, &upstream, "", "B", "b@devices.vaultone").unwrap();

        // A fresh clone sees A's file (from A1) AND B's unique file; B_dup was
        // skipped rather than turning the rebase into a conflict.
        let paths_c = crate::config::Paths::resolve(&tmp.path().join("c"));
        let _repo_c = open_or_clone(&url, &paths_c.repo, "").unwrap();
        let a_text = std::fs::read_to_string(paths_c.repo.join(rel_a)).unwrap();
        assert!(a_text.contains("a-1"), "A's data still on remote: {a_text}");
        let b_text = std::fs::read_to_string(paths_c.repo.join(rel_b)).unwrap();
        assert!(
            b_text.contains("b-1"),
            "B's unique data reached the remote: {b_text}"
        );
    }

    /// New semantic (ticket 02): a row collect writes to the store — but NOT to a
    /// file, and NOT yet pushed — survives a pull that force-checks-out the
    /// worktree, because it lives in the store (pull only ADDS to the store). The
    /// next push recomputes the dirty day from the store and ships it to git.
    /// Replaces the old own-data-snapshot test: collect no longer appends files
    /// between pushes, so there is no uncommitted file-append to protect.
    #[test]
    fn unpushed_collect_survives_pull_and_reaches_git_on_next_push() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let dev_a = "aaaaaaaaaaaa";

        // A clones + collects one row into its store (dirty). No file, no push.
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let _repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let cfg_a = ConfigData {
            repo_url: Some(url.clone()),
            github_token: Some("tok".into()),
            device_id: dev_a.into(),
            ..Default::default()
        };
        let store_a = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let book = crate::pricing::seed_book();
        let rec = crate::ingest::recordify(&raw_usage("a-1"), dev_a, &book);
        store_a
            .ingest_marking_dirty(std::slice::from_ref(&rec))
            .unwrap();
        let day_file = paths_a
            .device_data_dir(dev_a)
            .join("usage-2026-07-13.jsonl");
        assert!(!day_file.exists(), "collect wrote the store, not a file");

        // Peer B advances the remote tip so A's pull fast-forwards + force-checks-out
        // (揉 the worktree). A's unpushed row is safe in the store.
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let repo_b = open_or_clone(&url, &paths_b.repo, "").unwrap();
        let b_file = paths_b
            .device_data_dir("bbbbbbbbbbbb")
            .join("usage-2026-07-30.jsonl");
        std::fs::create_dir_all(b_file.parent().unwrap()).unwrap();
        std::fs::write(&b_file, "{\"uuid\":\"b-1\"}\n").unwrap();
        commit_all(&repo_b, "B new data", "B", "b@devices.vaultone").unwrap();
        push(&repo_b, "").unwrap();

        // A pulls (imports B; force-checkout揉s the worktree — A's row is untouched
        // in the store, and still no file for it).
        pull_and_import(&store_a, &paths_a, &cfg_a).unwrap();
        assert!(
            !day_file.exists(),
            "pull does not write A's file either; the row lives in the store"
        );

        // A pushes: recompute the dirty day from the store ⇒ file ⇒ commit ⇒ push,
        // and the dirty day is cleared on success.
        let pushed = push_usage(&store_a, &paths_a, &cfg_a).unwrap();
        assert!(pushed, "A had its collected day to recompute + push");
        assert!(
            store_a.dirty_days().unwrap().is_empty(),
            "successful push clears the dirty day"
        );
        assert!(day_file.exists(), "push materialized A's day file");

        // A fresh clone + pull sees A's row on the remote.
        let paths_c = crate::config::Paths::resolve(&tmp.path().join("c"));
        let _repo_c = open_or_clone(&url, &paths_c.repo, "").unwrap();
        let store_c = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        pull_and_import(&store_c, &paths_c, &synced_cfg(&url, "tok")).unwrap();
        let stats_a = store_c
            .query_stats(&crate::model::UsageFilter {
                device_scope: Some(dev_a.into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            stats_a.request_count, 1,
            "A's unpushed row reached the remote after the next push"
        );
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
        let pushed = commit_and_push(&paths, &cfg, "vaultone: usage sync").unwrap();
        assert!(!pushed, "clean worktree ⇒ no commit/push");
    }

    /// Regression (review): a push that failed after its commit landed leaves a
    /// clean worktree whose branch is ahead of origin — the retry must still
    /// push. The pre-review `has_changes`-only gate no-op'd the retry, stranding
    /// the commit until an unrelated change re-dirtied the worktree.
    #[test]
    fn commit_and_push_retries_ahead_of_origin_with_clean_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let paths = crate::config::Paths::resolve(&tmp.path().join("dev"));
        let cfg = synced_cfg(&url, "tok");
        let repo = open_or_clone(&url, &paths.repo, "").unwrap();

        // A pushed commit: ships one day file to the remote.
        let f1 = paths
            .device_data_dir("aaaaaaaaaaaa")
            .join("usage-2026-07-13.jsonl");
        std::fs::create_dir_all(f1.parent().unwrap()).unwrap();
        std::fs::write(&f1, "{\"uuid\":\"a-1\"}\n").unwrap();
        assert!(commit_and_push(&paths, &cfg, "vaultone: usage sync").unwrap());

        // Simulate the failed-push residue: a second commit that the remote
        // never got — worktree clean, local branch ahead.
        let f2 = paths
            .device_data_dir("aaaaaaaaaaaa")
            .join("usage-2026-07-14.jsonl");
        std::fs::write(&f2, "{\"uuid\":\"a-2\"}\n").unwrap();
        commit_all(
            &repo,
            "usage sync (push failed)",
            "VaultOne",
            "a@devices.vaultone",
        )
        .unwrap();
        assert!(
            !has_changes(&repo).unwrap(),
            "worktree clean after the commit"
        );
        assert!(
            is_ahead_of_origin(&repo).unwrap(),
            "local branch ahead of origin"
        );

        // Retry: push must happen despite the clean worktree.
        assert!(commit_and_push(&paths, &cfg, "vaultone: usage sync").unwrap());
        assert!(
            !is_ahead_of_origin(&repo).unwrap(),
            "the stranded commit shipped"
        );
    }

    /// Regression (review): push_usage recovers the same stranded-commit state
    /// end to end — the retry ships the leftover commit, the recomputed day is
    /// already on git, and the dirty day is cleared.
    #[test]
    fn push_usage_recovers_stranded_commit_and_clears_days() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let paths = crate::config::Paths::resolve(&tmp.path().join("dev"));
        let cfg = ConfigData {
            repo_url: Some(url.clone()),
            github_token: Some("tok".into()),
            device_id: "aaaaaaaaaaaa".into(),
            ..Default::default()
        };
        let repo = open_or_clone(&url, &paths.repo, "").unwrap();
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();

        // Collect one row into the store (dirty, no file).
        let book = crate::pricing::seed_book();
        let rec = crate::ingest::recordify(&raw_usage("a-1"), "aaaaaaaaaaaa", &book);
        store
            .ingest_marking_dirty(std::slice::from_ref(&rec))
            .unwrap();

        // Stranded-commit state: the day file is committed but the push never
        // landed (worktree clean, branch ahead).
        let day_file = paths
            .device_data_dir("aaaaaaaaaaaa")
            .join("usage-2026-07-13.jsonl");
        std::fs::create_dir_all(day_file.parent().unwrap()).unwrap();
        std::fs::write(&day_file, "{\"uuid\":\"a-1\"}\n").unwrap();
        commit_all(
            &repo,
            "usage sync (push failed)",
            "VaultOne",
            "a@devices.vaultone",
        )
        .unwrap();
        assert!(!has_changes(&repo).unwrap());

        // push_usage: recompute is byte-identical (no worktree churn), but the
        // retry must still ship the stranded commit and clear the day.
        assert!(push_usage(&store, &paths, &cfg).unwrap());
        assert!(
            store.dirty_days().unwrap().is_empty(),
            "push landed ⇒ dirty day cleared"
        );
        assert!(
            !is_ahead_of_origin(&repo).unwrap(),
            "stranded commit shipped"
        );
    }

    /// push_usage with no dirty days and a clean worktree is a no-op: it does not
    /// push, does not error, and (trivially) clears nothing.
    #[test]
    fn push_usage_is_noop_with_nothing_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let paths = crate::config::Paths::resolve(&tmp.path().join("dev"));
        let cfg = synced_cfg(&url, "tok");
        let _repo = open_or_clone(&url, &paths.repo, "").unwrap();
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let pushed = push_usage(&store, &paths, &cfg).unwrap();
        assert!(!pushed, "no dirty days + clean worktree ⇒ no push");
        assert!(store.dirty_days().unwrap().is_empty());
    }

    /// Session snapshots round-trip across devices and un-favorite propagates:
    /// A favorites + pushes a snapshot; B pulls it in (meta + favorited +
    /// message); A un-favorites + pushes (the file vanishes from git); B pulls
    /// again and the un-favorite propagates (favorited clears, shared messages
    /// drop). Exercises the whole 3b-2/3 loop end to end.
    #[test]
    fn session_snapshots_roundtrip_and_unfavorite_propagates() {
        use crate::model::{SessionMessage, SessionMessageRole, SessionSystemData};

        fn dev_cfg(url: &str, dev: &str) -> ConfigData {
            let mut cfg = synced_cfg(url, "tok");
            cfg.device_id = dev.to_string();
            cfg
        }
        fn sys(id: &str) -> SessionSystemData {
            SessionSystemData {
                id: id.into(),
                source: "claude_code".into(),
                project_dir: "/p".into(),
                title_orig: format!("Title {id}"),
                started_at: "2026-08-01T00:00:00.000Z".into(),
                last_active_at: "2026-08-02T00:00:00.000Z".into(),
            }
        }
        fn msg(uuid: &str, sid: &str) -> SessionMessage {
            SessionMessage {
                uuid: uuid.into(),
                session_id: sid.into(),
                role: SessionMessageRole::User,
                ts: "2026-08-01T10:00:00.000Z".into(),
                model: None,
                name: None,
                content: format!("body {uuid}"),
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();
        let dev_a = "aabbccddeeff";
        let dev_b = "bbccddee0011";

        // Device A: collect a favorited session + message, then push its snapshot.
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let cfg_a = dev_cfg(&url, dev_a);
        let _repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let store_a = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        crate::ingest::ingest_sessions(&store_a, dev_a, &[sys("sx")], &[msg("u1", "sx")]).unwrap();
        store_a.set_session_favorited(dev_a, "sx", true).unwrap();
        assert!(
            push_usage(&store_a, &paths_a, &cfg_a).unwrap(),
            "A pushed the snapshot"
        );
        assert!(
            crate::ingest::transcript_path(&paths_a, dev_a, "sx").exists(),
            "snapshot written"
        );

        // Device B: pull → imports A's session (meta + favorited + message). B's
        // own snapshot dir stays empty (B favorited nothing, so it pushes nothing).
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let cfg_b = dev_cfg(&url, dev_b);
        let store_b = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        pull_and_import(&store_b, &paths_b, &cfg_b).unwrap();
        let b_sx = store_b
            .query_sessions(None)
            .unwrap()
            .into_iter()
            .find(|r| r.id == "sx")
            .expect("B sees A's session");
        assert_eq!(b_sx.device_id, dev_a);
        assert!(b_sx.favorited, "favorited rode the snapshot meta line");
        assert_eq!(b_sx.title, "Title sx");
        assert_eq!(
            store_b.query_session_messages(dev_a, "sx").unwrap().len(),
            1,
            "message imported"
        );

        // A un-favorites + pushes → the snapshot file vanishes from git.
        store_a.set_session_favorited(dev_a, "sx", false).unwrap();
        assert!(push_usage(&store_a, &paths_a, &cfg_a).unwrap());
        assert!(
            !crate::ingest::transcript_path(&paths_a, dev_a, "sx").exists(),
            "A removed the snapshot on un-favorite"
        );

        // B pulls again → un-favorite propagates: favorited clears, shared
        // messages drop (the cross-device un-favorite path).
        pull_and_import(&store_b, &paths_b, &cfg_b).unwrap();
        let b_sx2 = store_b
            .query_sessions(None)
            .unwrap()
            .into_iter()
            .find(|r| r.id == "sx")
            .expect("meta row kept");
        assert!(!b_sx2.favorited, "un-favorite propagated to B");
        assert!(
            store_b
                .query_session_messages(dev_a, "sx")
                .unwrap()
                .is_empty(),
            "shared messages dropped on un-favorite"
        );
    }

    /// Fast (no-git) check of the pull un-favorite composition: a peer's
    /// favorited sessions whose snapshot file is absent this pull are exactly
    /// what `presence_mismatches` flags, and `bulk_unfavorite_sessions` clears
    /// those (favorited flag + shared messages) while leaving the still-filed
    /// ones alone. Mirrors `import_peer_sessions`'s per-peer loop without a real
    /// git round-trip.
    #[test]
    fn pull_unfavorite_matches_presence_mismatches_without_git() {
        use crate::model::{SessionMessage, SessionMessageRole, SessionSystemData};

        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let peer = "peerdevice01";

        fn sys(id: &str) -> SessionSystemData {
            SessionSystemData {
                id: id.into(),
                source: "claude_code".into(),
                project_dir: "/p".into(),
                title_orig: id.into(),
                started_at: "2026-08-01T00:00:00.000Z".into(),
                last_active_at: "2026-08-02T00:00:00.000Z".into(),
            }
        }
        fn msg(uuid: &str, sid: &str) -> SessionMessage {
            SessionMessage {
                uuid: uuid.into(),
                session_id: sid.into(),
                role: SessionMessageRole::User,
                ts: "2026-08-01T10:00:00.000Z".into(),
                model: None,
                name: None,
                content: format!("body {uuid}"),
            }
        }

        // Three favorited sessions; each carries one shared message.
        for sid in ["s1", "s2", "s3"] {
            store.upsert_session(peer, &sys(sid)).unwrap();
            store.set_session_favorited(peer, sid, true).unwrap();
            store
                .ingest_session_messages_marking_dirty(
                    peer,
                    std::slice::from_ref(&msg(&format!("u-{sid}"), sid)),
                )
                .unwrap();
        }

        // Only s1 and s2 still have a snapshot file this pull → s3 was un-favorited.
        let still_present: std::collections::BTreeSet<String> =
            ["s1".to_string(), "s2".to_string()].into_iter().collect();
        let peer_favorited: std::collections::BTreeSet<String> = store
            .favorited_session_ids(peer)
            .unwrap()
            .into_iter()
            .collect();
        let to_unfavorite =
            presence_mismatches(&still_present, &peer_favorited).favorites_without_files;
        assert_eq!(to_unfavorite, vec!["s3".to_string()]);

        store.bulk_unfavorite_sessions(peer, &to_unfavorite).unwrap();
        assert_eq!(
            store.favorited_session_ids(peer).unwrap(),
            vec!["s1".to_string(), "s2".to_string()],
            "s3 un-favorited; s1/s2 kept"
        );
        assert!(
            store.query_session_messages(peer, "s3").unwrap().is_empty(),
            "s3 shared messages dropped"
        );
        assert_eq!(
            store.query_session_messages(peer, "s1").unwrap().len(),
            1,
            "untouched session keeps its message"
        );
    }

    #[test]
    fn sync_roundtrips_usage_across_devices() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        seed_remote(&remote);
        let url = remote.to_string_lossy().to_string();

        // Device A: write an artifact, then pull (no-op) + commit+push.
        let paths_a = crate::config::Paths::resolve(&tmp.path().join("a"));
        let cfg_a = synced_cfg(&url, "tok");
        let _repo_a = open_or_clone(&url, &paths_a.repo, "").unwrap();
        let book = crate::pricing::seed_book();
        let rec = crate::ingest::recordify(&raw_usage("round-1"), "aabbccddeeff", &book);
        crate::ingest::append_jsonl(&paths_a, "aabbccddeeff", &[rec]).unwrap();
        let store_a = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let imported_a = pull_and_import(&store_a, &paths_a, &cfg_a).unwrap();
        let pushed_a = commit_and_push(&paths_a, &cfg_a, "vaultone: usage sync").unwrap();
        assert!(pushed_a, "A had a local change to push");
        assert_eq!(imported_a, 1, "A imports its own artifact into its store");

        // Device B: pull A's artifact into B's fresh store.
        let paths_b = crate::config::Paths::resolve(&tmp.path().join("b"));
        let cfg_b = synced_cfg(&url, "tok");
        let store_b = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let imported_b = pull_and_import(&store_b, &paths_b, &cfg_b).unwrap();
        let pushed_b = commit_and_push(&paths_b, &cfg_b, "vaultone: usage sync").unwrap();
        assert_eq!(imported_b, 1, "B imported A's record");
        assert!(!pushed_b, "B has no local change beyond what it pulled");
        let stats = store_b
            .query_stats(&crate::model::UsageFilter::default())
            .unwrap();
        assert_eq!(stats.request_count, 1);
    }
}
