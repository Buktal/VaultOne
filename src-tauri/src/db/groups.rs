//! Local groups (device-private) CRUD + ghost-session reconciliation.

use super::*;

impl super::Store {
    // ---------------- Local groups (SQLite, device-private) ----------------

    pub fn list_local_groups(&self) -> AppResult<Vec<LocalGroup>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT id, name, created_at FROM local_groups ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(LocalGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    pub fn create_local_group(&self, id: &str, name: &str, created_at: &str) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO local_groups (id, name, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name",
            params![id, name, created_at],
        )?;
        Ok(())
    }

    pub fn rename_local_group(&self, id: &str, name: &str) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "UPDATE local_groups SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }

    /// Delete this device's sessions for `source` whose id was NOT seen by the
    /// latest collect — the file-backed reality check that keeps the sessions
    /// table from accumulating ghosts (deleted session files, previously
    /// scanned agent sub-sessions). Returns the deleted ids so the caller can
    /// also remove their transcript files. An empty `seen_ids` is a NO-OP —
    /// a transiently invisible source dir must not wipe real rows (the caller
    /// only passes a non-empty set anyway; this is the second line of defense).
    /// One transaction; `(device_id, source, id)` scoping never touches a
    /// peer's rows or another source.
    pub fn reconcile_sessions(
        &self,
        device_id: &str,
        source: &str,
        seen_ids: &[String],
    ) -> AppResult<Vec<String>> {
        if seen_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        // The seen set rides as a JSON array through json_each — a single
        // parameter with no SQLite variable-count ceiling for large sets.
        let json = serde_json::to_string(seen_ids)
            .map_err(|e| AppError::Internal(format!("reconcile seen ids: {e}")))?;
        let ghosts: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM sessions \
                 WHERE device_id = ?1 AND source = ?2 \
                   AND id NOT IN (SELECT value FROM json_each(?3))",
            )?;
            let rows = stmt.query_map(params![device_id, source, json], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !ghosts.is_empty() {
            tx.execute(
                "DELETE FROM sessions \
                 WHERE device_id = ?1 AND source = ?2 \
                   AND id NOT IN (SELECT value FROM json_each(?3))",
                params![device_id, source, json],
            )?;
            // A ghost session's messages are dead weight too — drop them in the
            // same transaction so the row and its transcript never split apart.
            // `session_messages` has no `source` column, so scope by device plus
            // the ghost id set (the very rows just deleted from `sessions`).
            let ghost_json = serde_json::to_string(&ghosts)
                .map_err(|e| AppError::Internal(format!("reconcile ghost ids: {e}")))?;
            tx.execute(
                "DELETE FROM session_messages \
                 WHERE device_id = ?1 \
                   AND session_id IN (SELECT value FROM json_each(?2))",
                params![device_id, ghost_json],
            )?;
        }
        tx.commit()?;
        Ok(ghosts)
    }

    /// Delete a local group AND clear it off every session that carried it
    /// (sessions stay, just ungrouped). One transaction so the cleanup never
    /// leaves dangling group_id references.
    pub fn delete_local_group(&self, id: &str) -> AppResult<()> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE sessions SET local_group_id = '' WHERE local_group_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM local_groups WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::testutil::*;
    use crate::model::SessionMessageRole;

    // ---- reconcile_sessions ----

    #[test]
    fn reconcile_deletes_ghosts_keeps_seen_and_user_data() {
        let s = mem();
        seed_session(&s, "real", "dev", "2026-08-15T10:00:00.000Z");
        seed_session(&s, "ghost", "dev", "2026-08-10T10:00:00.000Z");
        // User data on the survivor must survive reconciliation.
        s.set_session_custom_title("dev", "real", Some("Renamed"))
            .unwrap();
        s.set_session_favorited("dev", "real", true).unwrap();
        s.set_session_local_group("dev", "real", Some("lg1"))
            .unwrap();

        // Messages for both sessions; the ghost's messages must be dropped in
        // the same transaction as its row — a session and its transcript are
        // one unit, never split.
        s.ingest_session_messages_marking_dirty(
            "dev",
            &[
                msg(
                    "u-real",
                    "real",
                    SessionMessageRole::User,
                    "2026-08-15T10:00:00Z",
                ),
                msg(
                    "u-ghost",
                    "ghost",
                    SessionMessageRole::User,
                    "2026-08-10T10:00:00Z",
                ),
            ],
        )
        .unwrap();

        let ghosts = s
            .reconcile_sessions("dev", "claude_code", &["real".to_string()])
            .unwrap();
        assert_eq!(ghosts, ["ghost"], "ghost row deleted, real row kept");

        let rows = s.query_sessions(None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "real");
        assert_eq!(rows[0].title, "Renamed", "custom_title preserved");
        assert!(rows[0].favorited, "favorited preserved");
        assert_eq!(rows[0].local_group_id, "lg1", "group preserved");

        assert_eq!(
            s.query_session_messages("dev", "real").unwrap().len(),
            1,
            "survivor's messages kept"
        );
        assert!(
            s.query_session_messages("dev", "ghost").unwrap().is_empty(),
            "ghost's messages dropped with its row"
        );
    }

    #[test]
    fn reconcile_is_scoped_by_device_and_source() {
        let s = mem();
        seed_session(&s, "same", "dev1", "2026-08-15T10:00:00.000Z");
        seed_session(&s, "same", "dev2", "2026-08-15T10:00:00.000Z");
        // Another session id under a different source on the same device.
        seed_session_source(
            &s,
            "codex-same",
            "dev1",
            "codex_cli",
            "2026-08-15T10:00:00.000Z",
        );

        // Reconcile dev1/claude_code with nothing seen → dev1's claude row
        // goes, dev2's row and the codex row stay.
        let ghosts = s.reconcile_sessions("dev1", "claude_code", &[]).unwrap();
        assert!(ghosts.is_empty(), "empty seen set is a no-op");
        let ghosts = s
            .reconcile_sessions("dev1", "claude_code", &["other".to_string()])
            .unwrap();
        assert_eq!(ghosts, ["same"], "dev1 claude row is the ghost");

        let survivors: std::collections::BTreeSet<String> = s
            .query_sessions(None)
            .unwrap()
            .into_iter()
            .map(|r| format!("{}/{}", r.device_id, r.source))
            .collect();
        assert_eq!(
            survivors,
            std::collections::BTreeSet::from([
                "dev2/claude_code".to_string(),
                "dev1/codex_cli".to_string(),
            ]),
            "peer + other-source rows untouched"
        );
    }

    #[test]
    fn reconcile_is_idempotent_and_empty_seen_is_noop() {
        let s = mem();
        seed_session(&s, "a", "dev", "2026-08-15T10:00:00.000Z");
        seed_session(&s, "b", "dev", "2026-08-15T10:00:00.000Z");
        // Empty seen → nothing deleted (protects a transiently invisible dir).
        assert!(s
            .reconcile_sessions("dev", "claude_code", &[])
            .unwrap()
            .is_empty());
        assert_eq!(s.query_sessions(None).unwrap().len(), 2);
        // First pass deletes the ghost.
        assert_eq!(
            s.reconcile_sessions("dev", "claude_code", &["a".to_string()])
                .unwrap(),
            ["b"]
        );
        // Second pass: nothing left to delete.
        assert!(s
            .reconcile_sessions("dev", "claude_code", &["a".to_string()])
            .unwrap()
            .is_empty());
        assert_eq!(s.query_sessions(None).unwrap().len(), 1);
    }
}
