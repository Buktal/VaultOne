//! Local groups (device-private) CRUD.

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
