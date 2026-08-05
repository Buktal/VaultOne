//! Device registry CRUD + self-heal + local forget.

use super::*;

impl super::Store {
    // ---------------- Devices ----------------

    /// Register/refresh a device in the registry.
    pub fn upsert_device(
        &self,
        device_id: &str,
        display_name: &str,
        is_self: bool,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO device (device_id, display_name, is_self, first_seen)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id) DO UPDATE SET
               display_name=excluded.display_name,
               is_self=excluded.is_self",
            params![
                device_id,
                display_name,
                is_self as i64,
                crate::time::now_iso()
            ],
        )?;
        Ok(())
    }

    /// Self-heal the `device` table from `usage_records`: any device that has
    /// usage rows but no `device` row (e.g. a peer that never published its
    /// `config/devices_<id>.json` name artifact) gets a fallback row with a
    /// generated `Device-<prefix>` name. `ON CONFLICT DO NOTHING` preserves
    /// names already learned via `reload_devices_into_store` — this only fills
    /// gaps, never overwrites. `is_self` is left 0 here; the command layer
    /// re-derives it from `cfg.device_id` on read, so a stale stored value can
    /// never mislabel a peer as "this device". `first_seen` takes the device's
    /// earliest usage timestamp (more truthful than `now`).
    pub fn discover_devices_from_usage(&self) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT device_id, MIN(timestamp)
             FROM usage_records
             WHERE device_id NOT IN (SELECT device_id FROM device)
             GROUP BY device_id",
        )?;
        let gaps: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for (device_id, first_seen) in gaps {
            let name = crate::config::default_display_name(&device_id);
            conn.execute(
                "INSERT INTO device (device_id, display_name, is_self, first_seen)
                 VALUES (?1, ?2, 0, ?3)
                 ON CONFLICT(device_id) DO NOTHING",
                params![device_id, name, first_seen],
            )?;
        }
        Ok(())
    }

    pub fn list_devices(&self) -> AppResult<Vec<crate::model::DeviceInfo>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT device_id, display_name, is_self, first_seen FROM device ORDER BY is_self DESC, device_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::model::DeviceInfo {
                device_id: r.get(0)?,
                display_name: r.get(1)?,
                is_self: r.get::<_, i64>(2)? != 0,
                first_seen: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// All device_ids currently in the registry. Reconcile uses this to find
    /// rows whose backing git presence has vanished.
    pub fn list_device_ids(&self) -> AppResult<Vec<String>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare("SELECT device_id FROM device")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Locally forget a device: drop its registry row and ALL its usage data
    /// (usage_records, turn_durations). No Git effect — a peer still in the repo
    /// reappears on the next pull, which re-imports its registry entry and data
    /// artifacts. The caller MUST guard `is_self` (this device is never
    /// forgettable). Returns the total rows removed.
    pub fn forget_device_local(&self, device_id: &str) -> AppResult<usize> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut deleted = 0;
        deleted += tx.execute(
            "DELETE FROM usage_records WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM turn_durations WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM device WHERE device_id = ?1",
            params![device_id],
        )?;
        tx.commit()?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testutil::*;

    #[test]
    fn forget_device_local_purges_all_its_data() {
        let s = mem();
        s.upsert_device("aaaaaaaaaaaa", "Device-aaaa", false)
            .unwrap();
        s.upsert_device("bbbbbbbbbbbb", "Device-bbbb", false)
            .unwrap();
        s.ingest(&[
            rec("u1", "2026-07-13", "glm-5.2", "aaaaaaaaaaaa", 100, 50, 0.0),
            rec("u2", "2026-07-13", "glm-5.2", "bbbbbbbbbbbb", 200, 80, 0.0),
        ])
        .unwrap();

        let deleted = s.forget_device_local("aaaaaaaaaaaa").unwrap();
        // device row + usage_records.
        assert!(deleted >= 2, "expected several rows deleted, got {deleted}");

        let ids = s.list_device_ids().unwrap();
        assert!(
            !ids.iter().any(|i| i == "aaaaaaaaaaaa"),
            "forgotten device must be gone from the registry"
        );
        assert!(ids.iter().any(|i| i == "bbbbbbbbbbbb"));

        // Forgotten device's usage is gone; the survivor keeps its row.
        let gone = s
            .count_logs(&UsageFilter {
                device_scope: Some("aaaaaaaaaaaa".into()),
                ..UsageFilter::default()
            })
            .unwrap();
        assert_eq!(gone, 0);
        let kept = s
            .count_logs(&UsageFilter {
                device_scope: Some("bbbbbbbbbbbb".into()),
                ..UsageFilter::default()
            })
            .unwrap();
        assert_eq!(kept, 1);
    }
}
