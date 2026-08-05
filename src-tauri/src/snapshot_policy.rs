//! Snapshot presence policy: the rule that a session's git snapshot file
//! exists exactly when the session is favorited, and the action that enforces
//! it. Pure — no IO, no knowledge of sync (git transport) or db (SQLite).
//!
//! Two enforcement sites consult this one definition of "in sync" so they
//! cannot drift apart:
//! - **push** (this device): for each dirty session, `decide_snapshot_action`
//!   says whether to write or remove *this device's* snapshot file.
//! - **pull** (a peer): `presence_mismatches` says which of a peer's favorited
//!   sessions have no snapshot file this pull — i.e. the peer un-favorited
//!   them — so the local rows can be cleared.

use std::collections::BTreeSet;

/// The action that brings one session's snapshot file into sync with its
/// favorited state. `Write` makes the file exist (recompute it from the store);
/// `Remove` makes it not exist. `Remove` is idempotent at the executor — a
/// missing file is a no-op, not an error — because "the file must not exist"
/// is satisfied whether or not it was ever there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotAction {
    Write,
    Remove,
}

/// Decide the snapshot action for one session from its favorited flag alone.
/// favorited ⇒ the file must exist ⇒ `Write`; not favorited ⇒ it must not ⇒
/// `Remove`. The rule is deliberately binary — "file exists ⇔ favorited" has
/// no third state; "the file is already absent" is the executor's idempotent
/// detail, not a distinct case.
pub fn decide_snapshot_action(is_favorited: bool) -> SnapshotAction {
    if is_favorited {
        SnapshotAction::Write
    } else {
        SnapshotAction::Remove
    }
}

/// The two ways a set of sessions can be out of sync with their snapshot
/// files. Each enforcement path consumes the half it can fix:
/// - push, after acting per session, leaves both halves empty (the test oracle);
/// - pull reads `favorites_without_files` — a favorited session whose file
///   vanished since the last pull was un-favorited on the peer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotMismatches {
    /// Favorited in the store but no snapshot file on disk.
    pub favorites_without_files: Vec<String>,
    /// Snapshot file on disk but not favorited in the store.
    pub files_without_favorites: Vec<String>,
}

/// Compute how a set of snapshot files diverges from a set of favorited
/// sessions — the single, pure definition of "in sync" shared by push and
/// pull. `file_ids` is the set of sessions with a snapshot file on disk;
/// `favorited_ids` is the set favorited in the store. Their intersection is in
/// sync (contributes to neither half); each side's unique elements land in its
/// own breach half. Output halves are sorted (BTreeSet difference order), so
/// callers compare deterministically.
pub fn presence_mismatches(
    file_ids: &BTreeSet<String>,
    favorited_ids: &BTreeSet<String>,
) -> SnapshotMismatches {
    SnapshotMismatches {
        favorites_without_files: favorited_ids.difference(file_ids).cloned().collect(),
        files_without_favorites: file_ids.difference(favorited_ids).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// favorited ⇒ Write, not favorited ⇒ Remove — the whole rule.
    #[test]
    fn decide_writes_when_favorited_removes_when_not() {
        assert!(matches!(
            decide_snapshot_action(true),
            SnapshotAction::Write
        ));
        assert!(matches!(
            decide_snapshot_action(false),
            SnapshotAction::Remove
        ));
    }

    /// Every set relationship maps to the right breach half; the intersection
    /// is in sync and lands in neither.
    #[test]
    fn presence_mismatches_partitions_the_four_set_cases() {
        // empty / empty → fully in sync.
        assert_eq!(
            presence_mismatches(&set(&[]), &set(&[])),
            SnapshotMismatches::default()
        );

        // files only → every file lacks a favorite.
        let m = presence_mismatches(&set(&["a", "b"]), &set(&[]));
        assert_eq!(
            m.files_without_favorites,
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(m.favorites_without_files.is_empty());

        // favorites only → every favorite lacks a file.
        let m = presence_mismatches(&set(&[]), &set(&["a", "b"]));
        assert_eq!(
            m.favorites_without_files,
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(m.files_without_favorites.is_empty());

        // overlap: a is file-only, c is favorite-only, b is in sync.
        let m = presence_mismatches(&set(&["a", "b"]), &set(&["b", "c"]));
        assert_eq!(m.files_without_favorites, vec!["a".to_string()]);
        assert_eq!(m.favorites_without_files, vec!["c".to_string()]);
    }

    /// The oracle: applying each session's `decide_snapshot_action` to its file
    /// presence drives `presence_mismatches` to empty — i.e. Write/Remove really
    /// do enforce "file exists ⇔ favorited". This is the git-free invariant
    /// check the push path satisfies once it has acted.
    #[test]
    fn applying_actions_clears_mismatches() {
        let favorited = set(&["a", "b"]);
        let mut files = set(&[]); // nothing on disk yet

        for sid in ["a", "b", "c", "d"] {
            match decide_snapshot_action(favorited.contains(sid)) {
                SnapshotAction::Write => {
                    files.insert(sid.to_string());
                }
                SnapshotAction::Remove => {
                    files.remove(sid);
                }
            }
        }
        assert_eq!(
            presence_mismatches(&files, &favorited),
            SnapshotMismatches::default(),
            "in sync after the actions"
        );
    }
}
