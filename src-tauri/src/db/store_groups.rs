//! Local groups (device-private) CRUD.

use super::*;

impl super::Store {
    // ---------------- Local groups (SQLite, device-private) ----------------

    pub fn list_local_groups(&self) -> AppResult<Vec<LocalGroup>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        // User-ordered by position; name breaks ties so rows created before
        // drag-to-reorder (all position 0) keep the legacy alphabetical order.
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, position FROM local_groups \
             ORDER BY position, name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LocalGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                position: r.get::<_, i64>(3)? as u32,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Create a local group appended at the END of the current order (max
    /// position + 1). Returns the full row so the command layer has the
    /// assigned position without a second read. The ON CONFLICT branch only
    /// refreshes the name — a recreated id keeps its position.
    pub fn create_local_group(
        &self,
        id: &str,
        name: &str,
        created_at: &str,
    ) -> AppResult<LocalGroup> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM local_groups",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO local_groups (id, name, created_at, position)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name",
            params![id, name, created_at, position],
        )?;
        Ok(LocalGroup {
            id: id.to_string(),
            name: name.to_string(),
            created_at: created_at.to_string(),
            position: position as u32,
        })
    }

    pub fn rename_local_group(&self, id: &str, name: &str) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "UPDATE local_groups SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
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

    /// Apply a full new display order: each id's `position` becomes its index
    /// in `ordered_ids` (the sidebar's complete track order after a drag).
    /// Unknown ids are ignored and absent ids keep their old position — the
    /// frontend always sends the full list, so a mismatch is a stale caller
    /// (e.g. a group deleted between fetch and drop) and must not fail the
    /// whole reorder. One transaction so the new order lands atomically.
    pub fn reorder_local_groups(&self, ordered_ids: &[String]) -> AppResult<()> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        for (i, id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE local_groups SET position = ?2 WHERE id = ?1",
                params![id, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::testutil::*;

    #[test]
    fn create_appends_new_groups_after_existing_ones() {
        let s = mem();
        s.create_local_group("a", "Alpha", "2026-08-01T00:00:00Z")
            .unwrap();
        s.create_local_group("b", "Beta", "2026-08-01T00:00:00Z")
            .unwrap();
        s.create_local_group("c", "Gamma", "2026-08-01T00:00:00Z")
            .unwrap();
        let names: Vec<String> = s
            .list_local_groups()
            .unwrap()
            .into_iter()
            .map(|g| g.name)
            .collect();
        // Creation order, not alphabetical.
        assert_eq!(names, ["Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn reorder_rewrites_positions_and_keeps_list_order() {
        let s = mem();
        for id in ["a", "b", "c"] {
            s.create_local_group(id, id, "2026-08-01T00:00:00Z")
                .unwrap();
        }
        s.reorder_local_groups(&["c".into(), "a".into(), "b".into()])
            .unwrap();
        let ids: Vec<String> = s
            .list_local_groups()
            .unwrap()
            .into_iter()
            .map(|g| g.id)
            .collect();
        assert_eq!(ids, ["c", "a", "b"]);
    }

    #[test]
    fn reorder_tolerates_stale_or_unknown_ids() {
        let s = mem();
        s.create_local_group("a", "Alpha", "2026-08-01T00:00:00Z")
            .unwrap();
        s.create_local_group("b", "Beta", "2026-08-01T00:00:00Z")
            .unwrap();
        s.create_local_group("c", "Gamma", "2026-08-01T00:00:00Z")
            .unwrap();
        // "c" was deleted between fetch and drop — the reorder still lands,
        // and an injected unknown id is ignored.
        s.reorder_local_groups(&["b".into(), "zz".into(), "a".into()])
            .unwrap();
        let ids: Vec<String> = s
            .list_local_groups()
            .unwrap()
            .into_iter()
            .map(|g| g.id)
            .collect();
        // "b" got index 0, "a" index 2; "c" kept its old position 2 and ties
        // with "a" → name order puts it after "a" (deterministic fallback).
        assert_eq!(ids, ["b", "a", "c"]);
    }
}
