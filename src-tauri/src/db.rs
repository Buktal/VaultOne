//! SQLite Local Store.
//!
//! Owns the schema, the dedup ledger, daily rollups cache, pricing table and
//! device registry. Exposes typed read methods (stats / trend / logs / models)
//! and write methods (ingest, pricing CRUD, rebill) — the JS layer never sees
//! SQL (typed command boundary).
//!
//! Cost columns are `rust_decimal::Decimal` stored as TEXT; sums over
//! them read back as REAL for display (f64 is display-only — JS never recomputes
//! cost).

mod migrate;
mod schema;

use std::sync::Mutex;

use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::{
    LocalGroup, LogsQuery, ModelStatsRow, PricingEntry, SessionFilter, SessionMetaSync,
    SessionRow, SessionSystemData, TokenCounts, TrendBucket, TrendPoint, TurnDuration, UsageFilter,
    UsageLogRow, UsageRecord, UsageStats,
};
use crate::pricing::{ModelPricing, PricingBook};
use crate::providers::{FileCursor, ScanProgress, ScanProgressDelta};

/// Thread-safe wrapper over a single SQLite connection.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (or create) `vaultone.db` and ensure the schema + seed pricing.
    pub fn open(path: &std::path::Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        // Tables → migrate → indexes, in that order. A legacy DB's usage_records
        // predates the session_id column, so idx_usage_session must not run until
        // migrate_schema has ALTERed the column on — building it first panics on
        // upgrade ("no such column: session_id"). The fresh-DB path is unaffected:
        // schema_tables_sql creates every table at its final column set already.
        conn.execute_batch(&schema::schema_tables_sql())?;
        migrate::migrate_schema(&conn)?;
        conn.execute_batch(&schema::schema_indexes_sql())?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.ensure_pricing_seed()?;
        Ok(store)
    }

    /// Seed the pricing table from the built-in book if it is empty.
    fn ensure_pricing_seed(&self) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM model_pricing", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }
        let now = crate::time::now_iso();
        let mut stmt = conn.prepare(
            "INSERT INTO model_pricing
             (model_key, display_name, input_per_million, output_per_million,
              cache_read_per_million, cache_creation_per_million, is_builtin, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )?;
        for p in crate::pricing::builtin_seed() {
            stmt.execute(params![
                p.model_key,
                p.display_name,
                p.input.to_string(),
                p.output.to_string(),
                p.cache_read.to_string(),
                p.cache_creation.to_string(),
                p.is_builtin as i64,
                now,
            ])?;
        }
        Ok(())
    }

    // ---------------- Pricing ----------------

    /// Load all pricing rows into a `PricingBook` for ingest-time cost calc.
    pub fn load_pricing_book(&self) -> AppResult<PricingBook> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT model_key, display_name, input_per_million, output_per_million,
                    cache_read_per_million, cache_creation_per_million, is_builtin
             FROM model_pricing",
        )?;
        let rows = stmt.query_map([], row_to_pricing)?;
        Ok(PricingBook::from_iter(rows.filter_map(Result::ok)))
    }

    /// Snapshot all pricing entries (DTO) for the UI.
    pub fn list_pricing(&self) -> AppResult<Vec<PricingEntry>> {
        Ok(self
            .load_pricing_models()?
            .iter()
            .map(ModelPricing::to_entry)
            .collect())
    }

    /// Load all pricing rows (model form), ordered by key. Used by file export.
    pub fn load_pricing_models(&self) -> AppResult<Vec<ModelPricing>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT model_key, display_name, input_per_million, output_per_million,
                    cache_read_per_million, cache_creation_per_million, is_builtin
             FROM model_pricing ORDER BY model_key",
        )?;
        let rows = stmt.query_map([], row_to_pricing)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Reload pricing from a JSON doc on disk (the local `pricing.json`) into
    /// the Store — the manual reload command's parse → upsert loop. Returns
    /// the entry count.
    pub fn reload_pricing_from_path(&self, path: &std::path::Path) -> AppResult<u32> {
        let text = std::fs::read_to_string(path)?;
        let entries = crate::pricing::parse_pricing_doc(&text)?;
        for e in &entries {
            self.upsert_pricing(&e.to_entry())?;
        }
        Ok(entries.len() as u32)
    }

    /// Upsert a pricing entry from the UI; user edits are `is_builtin = false`.
    pub fn upsert_pricing(&self, entry: &PricingEntry) -> AppResult<()> {
        let p = ModelPricing::from_entry(entry)?;
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO model_pricing
             (model_key, display_name, input_per_million, output_per_million,
              cache_read_per_million, cache_creation_per_million, is_builtin, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(model_key) DO UPDATE SET
               display_name=excluded.display_name,
               input_per_million=excluded.input_per_million,
               output_per_million=excluded.output_per_million,
               cache_read_per_million=excluded.cache_read_per_million,
               cache_creation_per_million=excluded.cache_creation_per_million,
               is_builtin=excluded.is_builtin,
               updated_at=excluded.updated_at",
            params![
                p.model_key,
                p.display_name,
                p.input.to_string(),
                p.output.to_string(),
                p.cache_read.to_string(),
                p.cache_creation.to_string(),
                p.is_builtin as i64,
                crate::time::now_iso(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_pricing(&self, model_key: &str) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "DELETE FROM model_pricing WHERE model_key = ?1",
            params![model_key],
        )?;
        Ok(())
    }

    // ---------------- Ingest ----------------

    /// Insert a batch of records, deduping by uuid via the ledger.
    /// Returns the newly imported rows (in order). Recomputes affected rollups.
    pub fn ingest(&self, records: &[UsageRecord]) -> AppResult<Vec<UsageRecord>> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        // Batch existence check: which (uuid, device_id) pairs are already in the
        // ledger? Dedup is scoped by device — the same source event replayed on two
        // devices must be counted per device, never collapsed into one row.
        let uuids: Vec<String> = records.iter().map(|r| r.uuid.clone()).collect();
        let placeholders = (0..uuids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT uuid, device_id FROM ledger WHERE uuid IN ({placeholders})");
        let known: std::collections::HashSet<(String, String)> = {
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(uuids.iter()), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(Result::ok).collect()
        };

        let now = crate::time::now_iso();
        let mut new_days: std::collections::HashSet<(String, String, String)> = Default::default();
        let mut inserted: Vec<UsageRecord> = Vec::new();
        for r in records {
            if known.contains(&(r.uuid.clone(), r.device_id.clone())) {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO usage_records
                 (uuid, timestamp, day, model, pricing_model, source, session_id, device_id,
                  input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                  server_tool_use, stop_reason, service_tier, iterations,
                  input_cost_usd, output_cost_usd, cache_read_cost_usd,
                  cache_creation_cost_usd, total_cost_usd)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![
                    r.uuid,
                    r.timestamp,
                    r.day,
                    r.model,
                    r.pricing_model,
                    r.source,
                    r.session_id,
                    r.device_id,
                    r.tokens.input as i64,
                    r.tokens.output as i64,
                    r.tokens.cache_creation as i64,
                    r.tokens.cache_read as i64,
                    serde_json::to_string(&r.server_tool_use).unwrap_or_else(|_| "{}".into()),
                    r.stop_reason,
                    r.service_tier,
                    r.iterations as i64,
                    r.cost.input_usd.to_string(),
                    r.cost.output_usd.to_string(),
                    r.cost.cache_read_usd.to_string(),
                    r.cost.cache_creation_usd.to_string(),
                    r.cost.total_usd.to_string(),
                ],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO ledger (uuid, source, device_id, ingested_at) VALUES (?1,?2,?3,?4)",
                params![r.uuid, r.source, r.device_id, now],
            )?;
            new_days.insert((r.day.clone(), r.model.clone(), r.device_id.clone()));
            inserted.push(r.clone());
        }

        // Recompute rollups only for affected (day, model, device) buckets.
        for (day, model, device) in &new_days {
            recompute_rollup(&tx, day, model, device)?;
        }

        tx.commit()?;
        Ok(inserted)
    }

    /// Insert per-turn durations, deduping by uuid (INSERT OR IGNORE). Separate
    /// grain from per-call usage_records. Returns the newly inserted subset
    /// (mirrors `ingest`) so only new rows are appended to the JSONL Artifact.
    pub fn ingest_turn_durations(&self, tds: &[TurnDuration]) -> AppResult<Vec<TurnDuration>> {
        if tds.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut inserted = Vec::new();
        for td in tds {
            let n = conn.execute(
                "INSERT OR IGNORE INTO turn_durations
                 (uuid, timestamp, day, device_id, duration_ms)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    td.uuid,
                    td.timestamp,
                    td.day,
                    td.device_id,
                    td.duration_ms as i64
                ],
            )?;
            if n > 0 {
                inserted.push(td.clone());
            }
        }
        Ok(inserted)
    }

    /// Load all incremental scan cursors. Empty on a fresh/cleared
    /// DB ⇒ the next collect is a full scan (safe fallback — the ledger dedups).
    pub fn load_scan_progress(&self) -> AppResult<ScanProgress> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT file_path, last_modified, last_line_offset FROM scan_progress")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                FileCursor {
                    last_modified: r.get::<_, i64>(1)?,
                    last_line_offset: r.get::<_, i64>(2)?,
                },
            ))
        })?;
        let mut map = ScanProgress::new();
        for row in rows {
            let (path, cursor) = row?;
            map.insert(path, cursor);
        }
        Ok(map)
    }

    /// Bulk UPSERT incremental scan cursors. Called AFTER a
    /// successful ingest so the cursor never advances past un-ingested rows.
    pub fn save_scan_progress(&self, delta: &ScanProgressDelta) -> AppResult<()> {
        if delta.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "INSERT INTO scan_progress (file_path, last_modified, last_line_offset)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
               last_modified = excluded.last_modified,
               last_line_offset = excluded.last_line_offset",
        )?;
        for (path, cursor) in delta {
            stmt.execute(params![path, cursor.last_modified, cursor.last_line_offset])?;
        }
        Ok(())
    }

    /// Drop every scan cursor so the next collect re-parses all source lines
    /// (a full scan). Used by the Artifact-gap reconcile to backfill rows that
    /// reached SQLite but not the JSONL Artifact — a full rescan re-appends
    /// them idempotently. Safe: the ledger dedups the SQLite side.
    pub fn clear_scan_progress(&self) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute("DELETE FROM scan_progress", [])?;
        Ok(())
    }

    /// UUIDs of every usage row owned by `device_id`. Used by the Artifact-gap
    /// reconcile to detect rows that reached SQLite but never the JSONL Artifact.
    pub fn usage_uuids_for_device(
        &self,
        device_id: &str,
    ) -> AppResult<std::collections::HashSet<String>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare("SELECT uuid FROM usage_records WHERE device_id = ?")?;
        let rows = stmt.query_map(params![device_id], |r| r.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    /// Rebill zero-cost rows whose model now has a price (freeze +
    /// top-up zero-cost only). Returns the number of rows rebilled.
    pub fn rebill_zero_cost(&self, book: &PricingBook) -> AppResult<usize> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare(
            "SELECT uuid, device_id, pricing_model, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens
             FROM usage_records
             WHERE CAST(total_cost_usd AS REAL) <= 0",
        )?;
        let candidates: Vec<(String, String, String, TokenCounts)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    TokenCounts {
                        input: r.get::<_, i64>(3)? as u32,
                        output: r.get::<_, i64>(4)? as u32,
                        cache_creation: r.get::<_, i64>(5)? as u32,
                        cache_read: r.get::<_, i64>(6)? as u32,
                    },
                ))
            })?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        let mut rebilled = 0usize;
        let mut affected: std::collections::HashSet<(String, String, String)> = Default::default();
        for (uuid, device, model, tokens) in candidates {
            let Some(rate) = book.resolve(&model) else {
                continue;
            };
            let cost = crate::pricing::CostCalculator::calc(tokens, Some(rate));
            if cost.total_usd <= rust_decimal::Decimal::ZERO {
                continue;
            }
            // uuid is no longer unique across devices, so scope the lookup/update by both.
            let day: String = tx.query_row(
                "SELECT day FROM usage_records WHERE uuid = ?1 AND device_id = ?2",
                params![uuid, device],
                |r| r.get(0),
            )?;
            tx.execute(
                "UPDATE usage_records SET
                   input_cost_usd=?1, output_cost_usd=?2, cache_read_cost_usd=?3,
                   cache_creation_cost_usd=?4, total_cost_usd=?5
                 WHERE uuid=?6 AND device_id=?7",
                params![
                    cost.input_usd.to_string(),
                    cost.output_usd.to_string(),
                    cost.cache_read_usd.to_string(),
                    cost.cache_creation_usd.to_string(),
                    cost.total_usd.to_string(),
                    uuid,
                    device,
                ],
            )?;
            affected.insert((day, model, device));
            rebilled += 1;
        }
        for (day, model, device) in &affected {
            recompute_rollup(&tx, day, model, device)?;
        }
        tx.commit()?;
        Ok(rebilled)
    }

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
    /// (usage_records, daily_rollups, turn_durations, ledger). No Git effect —
    /// a peer still in the repo reappears on the next pull, which re-imports
    /// its registry entry and data artifacts. The caller MUST guard `is_self`
    /// (this device is never forgettable). Returns the total rows removed.
    pub fn forget_device_local(&self, device_id: &str) -> AppResult<usize> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut deleted = 0;
        deleted += tx.execute(
            "DELETE FROM usage_records WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM daily_rollups WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM turn_durations WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM ledger WHERE device_id = ?1",
            params![device_id],
        )?;
        deleted += tx.execute(
            "DELETE FROM device WHERE device_id = ?1",
            params![device_id],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    // ---------------- Sessions ----------------

    /// Refresh a session's SYSTEM-data columns only. On conflict (same id +
    /// device_id), the ON CONFLICT clause updates exactly the refreshable
    /// columns (source / project_dir / title_orig / started_at / last_active_at)
    /// — it MUST NOT touch `custom_title` / `favorited` / `synced_group_id` /
    /// `local_group_id`. This is the SQLite-side encoding of the "re-extract
    /// never overwrites user data" invariant (architecture.md); the pure
    /// counterpart for the grain layer is `ingest::merge_session_user_data`.
    /// A regression test pins it.
    pub fn upsert_session(&self, device_id: &str, system: &SessionSystemData) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO sessions
             (id, device_id, source, project_dir, title_orig, started_at, last_active_at,
              custom_title, favorited, synced_group_id, local_group_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id, device_id) DO UPDATE SET
               source=excluded.source,
               project_dir=excluded.project_dir,
               title_orig=excluded.title_orig,
               started_at=excluded.started_at,
               last_active_at=excluded.last_active_at",
            params![
                system.id,
                device_id,
                system.source,
                system.project_dir,
                system.title_orig,
                system.started_at,
                system.last_active_at,
                "",        // custom_title — empty on insert; never updated here
                0,         // favorited — false on insert; never updated here
                "",        // synced_group_id — empty on insert; never updated here
                "",        // local_group_id — empty on insert; never updated here
            ],
        )?;
        Ok(())
    }

    /// Read the syncable user-data subset (id, source, custom_title, favorited,
    /// synced_group_id, ...) for one session as a [`SessionMetaSync`] — used by
    /// `ingest_sessions` to preserve user data when writing the grain snapshot.
    /// Returns `None` when the session is not yet in the table.
    pub fn get_session_meta_sync(
        &self,
        device_id: &str,
        session_id: &str,
    ) -> AppResult<Option<SessionMetaSync>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let row = conn
            .query_row(
                "SELECT id, source, project_dir, title_orig, started_at, last_active_at,
                        custom_title, favorited, synced_group_id
                 FROM sessions WHERE id = ?1 AND device_id = ?2",
                params![session_id, device_id],
                |r| {
                    Ok(SessionMetaSync {
                        id: r.get(0)?,
                        source: r.get(1)?,
                        project_dir: r.get(2)?,
                        title_orig: r.get(3)?,
                        started_at: r.get(4)?,
                        last_active_at: r.get(5)?,
                        custom_title: r.get(6)?,
                        favorited: r.get::<_, i64>(7)? != 0,
                        synced_group_id: r.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Import a PEER device's session grain row: write system data + the
    /// syncable user-data columns from the grain, while PRESERVING this row's
    /// `local_group_id` (device-private — never in the grain, so never
    /// overwritten on re-pull). Distinct from [`Self::upsert_session`] (which
    /// preserves ALL user data, for the local re-collect path) — the grain IS
    /// the synced source of truth for the system + syncable columns, so those
    /// are updated; only `local_group_id` is held back. The caller MUST skip
    /// this device's own id (own user data is authoritative locally; re-importing
    /// a staler own-grain would revert a just-applied favorite).
    pub fn import_session_grain(
        &self,
        device_id: &str,
        grain: &SessionMetaSync,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO sessions
             (id, device_id, source, project_dir, title_orig, started_at, last_active_at,
              custom_title, favorited, synced_group_id, local_group_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id, device_id) DO UPDATE SET
               source=excluded.source,
               project_dir=excluded.project_dir,
               title_orig=excluded.title_orig,
               started_at=excluded.started_at,
               last_active_at=excluded.last_active_at,
               custom_title=excluded.custom_title,
               favorited=excluded.favorited,
               synced_group_id=excluded.synced_group_id",
            params![
                grain.id,
                device_id,
                grain.source,
                grain.project_dir,
                grain.title_orig,
                grain.started_at,
                grain.last_active_at,
                grain.custom_title,
                grain.favorited as i64,
                grain.synced_group_id,
                "", // local_group_id — empty on insert; preserved on conflict
            ],
        )?;
        Ok(())
    }

    /// Set a session's favorited flag (user action). Only mutates the column;
    /// the transcript is collected on the next collect pass, not here.
    pub fn set_session_favorited(
        &self,
        device_id: &str,
        session_id: &str,
        favorited: bool,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "UPDATE sessions SET favorited = ?3 WHERE id = ?1 AND device_id = ?2",
            params![session_id, device_id, favorited as i64],
        )?;
        Ok(())
    }

    /// Set/clear a session's custom title. `None` or empty clears it (reverts to
    /// `title_orig` for display).
    pub fn set_session_custom_title(
        &self,
        device_id: &str,
        session_id: &str,
        title: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let t = title.unwrap_or("").trim();
        conn.execute(
            "UPDATE sessions SET custom_title = ?3 WHERE id = ?1 AND device_id = ?2",
            params![session_id, device_id, t],
        )?;
        Ok(())
    }

    /// Set/clear a session's local group (device-private).
    pub fn set_session_local_group(
        &self,
        device_id: &str,
        session_id: &str,
        group_id: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let g = group_id.unwrap_or("");
        conn.execute(
            "UPDATE sessions SET local_group_id = ?3 WHERE id = ?1 AND device_id = ?2",
            params![session_id, device_id, g],
        )?;
        Ok(())
    }

    /// Set/clear a session's synced group (cross-device via grain).
    pub fn set_session_synced_group(
        &self,
        device_id: &str,
        session_id: &str,
        group_id: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let g = group_id.unwrap_or("");
        conn.execute(
            "UPDATE sessions SET synced_group_id = ?3 WHERE id = ?1 AND device_id = ?2",
            params![session_id, device_id, g],
        )?;
        Ok(())
    }

    /// List sessions for the UI, joined live with `usage_records` to compute
    /// per-session request_count / total_tokens / total_cost_usd (no second
    /// source of token truth — ADR 0001). Title = `custom_title` when set, else
    /// `title_orig`. `filter` is optional; `None` lists every session.
    pub fn query_sessions(&self, filter: Option<&SessionFilter>) -> AppResult<Vec<SessionRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_session_where(filter);
        let sql = format!(
            "SELECT s.id, s.device_id, s.source, s.project_dir,
                    COALESCE(NULLIF(s.custom_title,''), s.title_orig) AS title,
                    s.favorited, s.local_group_id, s.synced_group_id,
                    s.started_at, s.last_active_at,
                    COALESCE(agg.request_count, 0),
                    COALESCE(agg.total_tokens, 0),
                    COALESCE(agg.total_cost_usd, 0.0)
             FROM sessions s
             LEFT JOIN (
                SELECT session_id, device_id,
                       COUNT(*) AS request_count,
                       COALESCE(SUM(input_tokens+output_tokens+cache_creation_tokens+cache_read_tokens),0) AS total_tokens,
                       COALESCE(SUM(CAST(total_cost_usd AS REAL)),0) AS total_cost_usd
                FROM usage_records GROUP BY session_id, device_id
             ) agg ON agg.session_id = s.id AND agg.device_id = s.device_id
             {clause}
             ORDER BY s.last_active_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_vec.iter()), |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                device_id: r.get(1)?,
                source: r.get(2)?,
                project_dir: r.get(3)?,
                title: r.get(4)?,
                favorited: r.get::<_, i64>(5)? != 0,
                local_group_id: r.get(6)?,
                synced_group_id: r.get(7)?,
                started_at: r.get(8)?,
                last_active_at: r.get(9)?,
                request_count: r.get::<_, i64>(10)? as u32,
                total_tokens: r.get::<_, i64>(11)? as u32,
                total_cost_usd: r.get(12)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Per-session usage aggregate (request_count, total_tokens, total_cost) for
    /// the transcript / detail view. Public API for a future command; not yet
    /// wired to one (kept here so the live-aggregate read path is in place next
    /// to `query_sessions`).
    #[allow(dead_code)]
    pub fn query_session_usage(&self, session_id: &str) -> AppResult<(u32, u32, f64)> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let row = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(input_tokens+output_tokens+cache_creation_tokens+cache_read_tokens),0),
                    COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records WHERE session_id = ?1",
            params![session_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u32,
                    r.get::<_, i64>(1)? as u32,
                    r.get::<_, f64>(2)?,
                ))
            },
        ).optional()?;
        Ok(row.unwrap_or_default())
    }

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

    // ---------------- Reads (dashboard) ----------------

    /// Aggregate stats over a filter (BLUEPRINT 使用统计).
    pub fn query_stats(&self, filter: &UsageFilter) -> AppResult<UsageStats> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter, true);
        let sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(SUM(input_tokens),0),
                COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_creation_tokens),0),
                COALESCE(SUM(cache_read_tokens),0),
                COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records {clause}"
        );
        let row = conn.query_row(&sql, params_from_iter(params_vec.iter()), |r| {
            Ok(UsageStats {
                request_count: r.get::<_, i64>(0)? as u32,
                input_tokens: r.get::<_, i64>(1)? as u32,
                output_tokens: r.get::<_, i64>(2)? as u32,
                cache_creation_tokens: r.get::<_, i64>(3)? as u32,
                cache_read_tokens: r.get::<_, i64>(4)? as u32,
                total_cost_usd: r.get::<_, f64>(5)?,
                ..Default::default()
            })
        })?;
        let mut s = row;
        s.total_tokens = s
            .input_tokens
            .saturating_add(s.output_tokens)
            .saturating_add(s.cache_creation_tokens)
            .saturating_add(s.cache_read_tokens);
        let tokens = TokenCounts {
            input: s.input_tokens,
            output: s.output_tokens,
            cache_creation: s.cache_creation_tokens,
            cache_read: s.cache_read_tokens,
        };
        s.cache_hit_rate = tokens.cache_hit_rate();
        // Per-turn aggregates (separate grain, from turn_durations).
        let (tclause, tparams) = build_where(filter, false);
        let tsql =
            format!("SELECT COUNT(*), COALESCE(AVG(duration_ms),0) FROM turn_durations {tclause}");
        let (turn_count, avg_dur): (i64, f64) =
            conn.query_row(&tsql, params_from_iter(tparams.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        s.turn_count = turn_count as u32;
        s.avg_turn_duration_ms = avg_dur;
        Ok(s)
    }

    /// Per-model breakdown over a filter.
    pub fn query_models(&self, filter: &UsageFilter) -> AppResult<Vec<ModelStatsRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter, true);
        let sql = format!(
            "SELECT model,
                COUNT(*),
                COALESCE(SUM(input_tokens+output_tokens+cache_creation_tokens+cache_read_tokens),0),
                COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records {clause}
             GROUP BY model ORDER BY 4 DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_vec.iter()), |r| {
            Ok(ModelStatsRow {
                model: r.get(0)?,
                request_count: r.get::<_, i64>(1)? as u32,
                total_tokens: r.get::<_, i64>(2)? as u32,
                total_cost_usd: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Trend points over a filter (BLUEPRINT 使用趋势). `bucket` picks the
    /// granularity: `Day` groups on the UTC `day` column
    /// (cross-device deterministic); `Hour` groups on local-time hour for the
    /// single-day zoom where per-day resolution collapses to one bar. The
    /// TrendPoint `day` field carries the resolved bucket key.
    pub fn query_trend(
        &self,
        filter: &UsageFilter,
        bucket: TrendBucket,
    ) -> AppResult<Vec<TrendPoint>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter, true);
        // Hour buckets read the clock in the device's local zone so a UTC+8
        // "today" trends in hours the user recognizes; the day bucket stays on
        // the stored UTC `day` for cross-device determinism.
        let grouping: &str = match bucket {
            TrendBucket::Day => "day",
            TrendBucket::Hour => "strftime('%Y-%m-%dT%H', timestamp, 'localtime')",
        };
        let sql = format!(
            "SELECT {grouping} AS bucket,
                COALESCE(SUM(input_tokens),0),
                COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_creation_tokens),0),
                COALESCE(SUM(cache_read_tokens),0),
                COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records {clause}
             GROUP BY bucket ORDER BY bucket"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_vec.iter()), |r| {
            let input: i64 = r.get(1)?;
            let output: i64 = r.get(2)?;
            let cc: i64 = r.get(3)?;
            let cr: i64 = r.get(4)?;
            Ok(TrendPoint {
                day: r.get(0)?,
                input_tokens: input as u32,
                output_tokens: output as u32,
                cache_creation_tokens: cc as u32,
                cache_read_tokens: cr as u32,
                total_tokens: (input + output + cc + cr) as u32,
                total_cost_usd: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Distinct sources/models present (for filter dropdowns).
    pub fn query_distinct(&self, column: &str) -> AppResult<Vec<String>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        // column is a fixed whitelist below, not user input — safe to interpolate.
        let col = match column {
            "source" => "source",
            "model" => "model",
            _ => return Err(AppError::Db("bad distinct column".into())),
        };
        let sql =
            format!("SELECT DISTINCT {col} FROM usage_records WHERE {col} != '' ORDER BY {col}");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Request-log rows (BLUEPRINT 请求日志; columns).
    pub fn query_logs(&self, q: &LogsQuery) -> AppResult<Vec<UsageLogRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(&q.filter, true);
        let limit = q.limit.clamp(1, 1000) as i64;
        let offset = q.offset as i64;
        let sql = format!(
            "SELECT uuid, timestamp, model, source, device_id,
                    input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                    stop_reason, CAST(total_cost_usd AS REAL)
             FROM usage_records {clause}
             ORDER BY timestamp DESC LIMIT {limit} OFFSET {offset}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_vec.iter()), |r| {
            Ok(UsageLogRow {
                uuid: r.get(0)?,
                timestamp: r.get(1)?,
                model: r.get(2)?,
                source: r.get(3)?,
                device_id: r.get(4)?,
                tokens: TokenCounts {
                    input: r.get::<_, i64>(5)? as u32,
                    output: r.get::<_, i64>(6)? as u32,
                    cache_creation: r.get::<_, i64>(7)? as u32,
                    cache_read: r.get::<_, i64>(8)? as u32,
                },
                stop_reason: r.get(9)?,
                total_cost_usd: r.get(10)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Total row count (for paging display).
    pub fn count_logs(&self, filter: &UsageFilter) -> AppResult<u32> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter, true);
        let sql = format!("SELECT COUNT(*) FROM usage_records {clause}");
        let n: i64 = conn.query_row(&sql, params_from_iter(params_vec.iter()), |r| r.get(0))?;
        Ok(n as u32)
    }
}

fn row_to_pricing(r: &rusqlite::Row<'_>) -> rusqlite::Result<ModelPricing> {
    use std::str::FromStr;
    let parse =
        |s: String| rust_decimal::Decimal::from_str(&s).unwrap_or(rust_decimal::Decimal::ZERO);
    Ok(ModelPricing {
        model_key: r.get(0)?,
        display_name: r.get(1)?,
        input: parse(r.get(2)?),
        output: parse(r.get(3)?),
        cache_read: parse(r.get(4)?),
        cache_creation: parse(r.get(5)?),
        is_builtin: r.get::<_, i64>(6)? != 0,
    })
}

/// Recompute one (day, model, device) rollup bucket from usage_records.
fn recompute_rollup(
    tx: &rusqlite::Transaction,
    day: &str,
    model: &str,
    device: &str,
) -> AppResult<()> {
    let agg: Option<(i64, i64, i64, i64, i64, f64)> = tx
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_creation_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0),
                    COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records WHERE day=?1 AND model=?2 AND device_id=?3",
            params![day, model, device],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;

    // Store the rollup total as a TEXT decimal reconstructed from the REAL sum
    // (display-grade precision only; rollups are a derived cache).
    let total_text = match agg {
        Some((_, _, _, _, _, cost)) if cost > 0.0 => format!("{cost:.6}"),
        _ => "0".to_string(),
    };
    if let Some((cnt, inp, out, cc, cr, _)) = agg {
        tx.execute(
            "INSERT INTO daily_rollups
             (day, model, device_id, input_tokens, output_tokens, cache_creation_tokens,
              cache_read_tokens, request_count, total_cost_usd)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(day, model, device_id) DO UPDATE SET
               input_tokens=excluded.input_tokens,
               output_tokens=excluded.output_tokens,
               cache_creation_tokens=excluded.cache_creation_tokens,
               cache_read_tokens=excluded.cache_read_tokens,
               request_count=excluded.request_count,
               total_cost_usd=excluded.total_cost_usd",
            params![day, model, device, inp, out, cc, cr, cnt, total_text],
        )?;
    }
    Ok(())
}

/// Build a `WHERE` clause + bound params for a `UsageFilter` (timestamp range,
/// model, source, device scope). The range filters on `timestamp` (UTC), not
/// `day` — see `UsageFilter` for why. Returns `("WHERE ...", vec![...])` or
/// `("", [])`.
fn build_where(filter: &UsageFilter, include_model_source: bool) -> (String, Vec<SqlValue>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(ts) = &filter.from_ts {
        if !ts.is_empty() {
            conds.push("timestamp >= ?".into());
            params.push(SqlValue::Text(ts.clone()));
        }
    }
    if let Some(ts) = &filter.to_ts {
        if !ts.is_empty() {
            conds.push("timestamp <= ?".into());
            params.push(SqlValue::Text(ts.clone()));
        }
    }
    if include_model_source {
        if let Some(m) = &filter.model {
            if !m.is_empty() {
                conds.push("model = ?".into());
                params.push(SqlValue::Text(m.clone()));
            }
        }
        if let Some(s) = &filter.source {
            if !s.is_empty() {
                conds.push("source = ?".into());
                params.push(SqlValue::Text(s.clone()));
            }
        }
    }
    if let Some(d) = &filter.device_scope {
        if !d.is_empty() {
            conds.push("device_id = ?".into());
            params.push(SqlValue::Text(d.clone()));
        }
    }
    let clause = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    (clause, params)
}

/// Build a WHERE clause over the `sessions` table for a [`SessionFilter`]. The
/// clause prefixes every column with `s.` so it composes with the
/// `usage_records` subquery JOIN in [`Store::query_sessions`]. Empty filter ⇒
/// `("", [])`.
fn build_session_where(filter: Option<&SessionFilter>) -> (String, Vec<SqlValue>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    let Some(f) = filter else {
        return (String::new(), params);
    };
    if let Some(d) = &f.device_scope {
        if !d.is_empty() {
            conds.push("s.device_id = ?".into());
            params.push(SqlValue::Text(d.clone()));
        }
    }
    if let Some(s) = &f.source {
        if !s.is_empty() {
            conds.push("s.source = ?".into());
            params.push(SqlValue::Text(s.clone()));
        }
    }
    if let Some(fav) = f.favorited {
        conds.push(format!("s.favorited = {}", fav as i64));
    }
    if let Some(g) = &f.local_group_id {
        conds.push("s.local_group_id = ?".into());
        params.push(SqlValue::Text(g.clone()));
    }
    if let Some(g) = &f.synced_group_id {
        conds.push("s.synced_group_id = ?".into());
        params.push(SqlValue::Text(g.clone()));
    }
    if let Some(ts) = &f.from_ts {
        if !ts.is_empty() {
            conds.push("s.last_active_at >= ?".into());
            params.push(SqlValue::Text(ts.clone()));
        }
    }
    if let Some(ts) = &f.to_ts {
        if !ts.is_empty() {
            conds.push("s.last_active_at <= ?".into());
            params.push(SqlValue::Text(ts.clone()));
        }
    }
    let clause = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    (clause, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerToolUse;
    use std::path::Path;

    fn mem() -> Store {
        Store::open(Path::new(":memory:")).unwrap()
    }

    /// Build a stored record with a flat (input-only) cost for test simplicity.
    fn rec(
        uuid: &str,
        day: &str,
        model: &str,
        device: &str,
        input: u32,
        output: u32,
        cost_usd: f64,
    ) -> UsageRecord {
        let total =
            rust_decimal::Decimal::try_from(cost_usd).unwrap_or(rust_decimal::Decimal::ZERO);
        UsageRecord {
            uuid: uuid.into(),
            timestamp: format!("{day}T10:00:00.000Z"),
            day: day.into(),
            model: model.into(),
            pricing_model: crate::model::normalize_pricing_key(model),
            source: "claude_code".into(),
            session_id: String::new(),
            device_id: device.into(),
            tokens: TokenCounts {
                input,
                output,
                cache_creation: 0,
                cache_read: 0,
            },
            server_tool_use: ServerToolUse::default(),
            stop_reason: "end_turn".into(),
            service_tier: "standard".into(),
            iterations: 0,
            cost: crate::model::CostBreakdown {
                input_usd: total,
                output_usd: rust_decimal::Decimal::ZERO,
                cache_read_usd: rust_decimal::Decimal::ZERO,
                cache_creation_usd: rust_decimal::Decimal::ZERO,
                total_usd: total,
            },
        }
    }

    #[test]
    fn open_seeds_builtin_pricing() {
        let s = mem();
        let entries = s.list_pricing().unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.model_key == "glm-5.2"));
    }

    #[test]
    fn open_then_ingest_surfaces_stop_reason_end_to_end() {
        // A current-schema store (open runs SCHEMA + migrate) must ingest the
        // new per-call fields and surface them in the log query.
        let s = mem();
        s.ingest(std::slice::from_ref(&rec(
            "m1",
            "2026-07-21",
            "glm-5.2",
            "dev1",
            10,
            0,
            0.0,
        )))
        .unwrap();
        let logs = s
            .query_logs(&LogsQuery {
                filter: UsageFilter::default(),
                limit: 10,
                offset: 0,
            })
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].stop_reason, "end_turn");
    }

    #[test]
    fn ingest_inserts_then_dedups_same_uuid() {
        let s = mem();
        let r = rec("u1", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0);
        assert_eq!(s.ingest(std::slice::from_ref(&r)).unwrap().len(), 1);
        assert_eq!(s.ingest(&[r]).unwrap().len(), 0, "same uuid must dedupe");
    }

    /// Regression: the same uuid on two DIFFERENT devices must both be kept —
    /// dedup is scoped by (uuid, device_id), not uuid alone. The old uuid-only
    /// ledger dropped the peer device's row, so a source event replayed under two
    /// device ids (one ~/.claude/projects scanned twice, a restored opencode.db)
    /// silently erased one device. Both devices must be visible afterwards.
    #[test]
    fn ingest_keeps_same_uuid_across_devices() {
        let s = mem();
        let uuid_x = "codex:thread-v1:sess-1:1";
        let a = rec(
            uuid_x,
            "2026-07-30",
            "gpt-5.2-codex",
            "aaaaaa000001",
            100,
            10,
            0.0,
        );
        let b = rec(
            uuid_x,
            "2026-07-30",
            "gpt-5.2-codex",
            "bbbbbb000002",
            200,
            20,
            0.0,
        );
        assert_eq!(s.ingest(std::slice::from_ref(&a)).unwrap().len(), 1);
        // Same uuid, different device ⇒ must still ingest (previously dropped).
        assert_eq!(s.ingest(std::slice::from_ref(&b)).unwrap().len(), 1);

        let logs = s
            .query_logs(&LogsQuery {
                filter: UsageFilter::default(),
                limit: 10,
                offset: 0,
            })
            .unwrap();
        let devices: Vec<String> = logs.iter().map(|l| l.device_id.clone()).collect();
        assert!(devices.contains(&"aaaaaa000001".to_string()));
        assert!(devices.contains(&"bbbbbb000002".to_string()));

        // Re-ingesting the SAME (uuid, device) is still idempotent (re-pull dedup).
        assert_eq!(s.ingest(std::slice::from_ref(&a)).unwrap().len(), 0);
    }

    #[test]
    fn stats_and_trend_aggregate_over_records() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0),
            rec("b", "2026-07-13", "glm-5.2", "dev1", 200, 100, 2.0),
            rec("c", "2026-07-14", "gpt-4o", "dev1", 300, 0, 3.0),
        ])
        .unwrap();

        let stats = s.query_stats(&UsageFilter::default()).unwrap();
        assert_eq!(stats.request_count, 3);
        assert_eq!(stats.total_tokens, 750);
        assert!((stats.total_cost_usd - 6.0).abs() < 1e-9);

        let trend = s
            .query_trend(&UsageFilter::default(), TrendBucket::Day)
            .unwrap();
        assert_eq!(trend.len(), 2);
        assert_eq!(trend[0].day, "2026-07-13");
        assert_eq!(trend[0].total_tokens, 450);
    }

    #[test]
    fn filters_by_timestamp_range_and_model() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "d", 10, 0, 1.0),
            rec("b", "2026-07-14", "gpt-4o", "d", 20, 0, 2.0),
        ])
        .unwrap();
        // `b` lives at 2026-07-14T10:00Z; a from_ts at 2026-07-14T00:00Z
        // includes it and excludes `a` (2026-07-13T10:00Z). Range filters on
        // timestamp, never on the UTC `day` bucket (see UsageFilter).
        let from_ts = UsageFilter {
            from_ts: Some("2026-07-14T00:00:00.000Z".into()),
            ..Default::default()
        };
        assert_eq!(s.query_stats(&from_ts).unwrap().request_count, 1);
        let by_model = UsageFilter {
            model: Some("glm-5.2".into()),
            ..Default::default()
        };
        assert_eq!(s.query_stats(&by_model).unwrap().request_count, 1);
    }

    #[test]
    fn logs_ordered_desc_and_paged() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "d", 1, 0, 1.0),
            rec("b", "2026-07-14", "glm-5.2", "d", 2, 0, 2.0),
        ])
        .unwrap();
        let q = LogsQuery {
            filter: UsageFilter::default(),
            limit: 10,
            offset: 0,
        };
        let logs = s.query_logs(&q).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].uuid, "b", "ORDER BY timestamp DESC");
        let q2 = LogsQuery {
            filter: UsageFilter::default(),
            limit: 10,
            offset: 1,
        };
        assert_eq!(s.query_logs(&q2).unwrap().len(), 1);
    }

    #[test]
    fn models_breakdown_groups_by_model() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "d", 100, 0, 1.0),
            rec("b", "2026-07-13", "gpt-4o", "d", 50, 0, 2.0),
        ])
        .unwrap();
        let models = s.query_models(&UsageFilter::default()).unwrap();
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn ingest_recomputes_daily_rollup() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0),
            rec("b", "2026-07-13", "glm-5.2", "dev1", 200, 100, 2.0),
        ])
        .unwrap();
        // Rollup cache for the (day, model, device) bucket must reflect both rows.
        // (Lock taken AFTER ingest returns — ingest takes the same lock.)
        let conn = s.conn.lock().unwrap();
        let (cnt, tokens): (i64, i64) = conn.query_row(
            "SELECT request_count, input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens
             FROM daily_rollups WHERE day=?1 AND model=?2 AND device_id=?3",
            params!("2026-07-13", "glm-5.2", "dev1"),
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        drop(conn);
        assert_eq!(cnt, 2);
        assert_eq!(tokens, 450);
    }

    #[test]
    fn rebill_top_ups_zero_cost_rows() {
        let s = mem();
        // Zero-cost row for a model the seed book knows.
        s.ingest(&[rec("z", "2026-07-13", "glm-5.2", "d", 1000, 500, 0.0)])
            .unwrap();
        let book = s.load_pricing_book().unwrap();
        let n = s.rebill_zero_cost(&book).unwrap();
        assert_eq!(n, 1, "the zero-cost glm-5.2 row should be rebilled");
        let logs = s
            .query_logs(&LogsQuery {
                filter: UsageFilter::default(),
                limit: 10,
                offset: 0,
            })
            .unwrap();
        let z = logs.iter().find(|r| r.uuid == "z").unwrap();
        assert!(z.total_cost_usd > 0.0);
    }

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
        // device row + usage_records + ledger (+ rollup recompute leaves none).
        assert!(deleted >= 3, "expected several rows deleted, got {deleted}");

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

    #[test]
    fn pricing_crud_upsert_load_delete() {
        let s = mem();
        let entry = PricingEntry {
            model_key: "custom-model".into(),
            display_name: "Custom".into(),
            input_per_million: 1.0,
            output_per_million: 2.0,
            cache_read_per_million: 0.1,
            cache_creation_per_million: 1.25,
            is_builtin: false,
        };
        s.upsert_pricing(&entry).unwrap();
        let all = s.list_pricing().unwrap();
        assert!(all
            .iter()
            .any(|e| e.model_key == "custom-model" && !e.is_builtin));
        assert!(s
            .load_pricing_book()
            .unwrap()
            .resolve("custom-model")
            .is_some());
        s.delete_pricing("custom-model").unwrap();
        assert!(!s
            .list_pricing()
            .unwrap()
            .iter()
            .any(|e| e.model_key == "custom-model"));
    }

    #[test]
    fn turn_durations_ingest_and_aggregate() {
        let s = mem();
        s.ingest_turn_durations(&[
            TurnDuration {
                uuid: "t1".into(),
                timestamp: "2026-07-13T10:00:00Z".into(),
                day: "2026-07-13".into(),
                device_id: "d".into(),
                duration_ms: 100_000,
            },
            TurnDuration {
                uuid: "t2".into(),
                timestamp: "2026-07-13T11:00:00Z".into(),
                day: "2026-07-13".into(),
                device_id: "d".into(),
                duration_ms: 200_000,
            },
        ])
        .unwrap();
        // Same uuid dedupes (INSERT OR IGNORE).
        s.ingest_turn_durations(&[TurnDuration {
            uuid: "t1".into(),
            timestamp: "2026-07-13T10:00:00Z".into(),
            day: "2026-07-13".into(),
            device_id: "d".into(),
            duration_ms: 999_999,
        }])
        .unwrap();
        let stats = s.query_stats(&UsageFilter::default()).unwrap();
        assert_eq!(stats.turn_count, 2);
        assert!((stats.avg_turn_duration_ms - 150_000.0).abs() < 1e-9);
    }

    // ---- incremental scan cursors ----

    #[test]
    fn scan_progress_save_load_roundtrip() {
        let s = mem();
        let mut delta = ScanProgressDelta::new();
        delta.insert(
            "C:/a.jsonl".into(),
            FileCursor {
                last_modified: 1_000,
                last_line_offset: 5,
            },
        );
        delta.insert(
            "C:/b.jsonl".into(),
            FileCursor {
                last_modified: 2_000,
                last_line_offset: 10,
            },
        );
        s.save_scan_progress(&delta).unwrap();
        let loaded = s.load_scan_progress().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("C:/a.jsonl").unwrap().last_line_offset, 5);
        assert_eq!(loaded.get("C:/b.jsonl").unwrap().last_modified, 2_000);
    }

    #[test]
    fn scan_progress_upsert_overwrites_on_conflict() {
        let s = mem();
        let mut delta = ScanProgressDelta::new();
        delta.insert(
            "C:/a.jsonl".into(),
            FileCursor {
                last_modified: 1,
                last_line_offset: 5,
            },
        );
        s.save_scan_progress(&delta).unwrap();
        // Same path, advanced cursor — UPSERT must overwrite, not duplicate.
        delta.insert(
            "C:/a.jsonl".into(),
            FileCursor {
                last_modified: 2,
                last_line_offset: 10,
            },
        );
        s.save_scan_progress(&delta).unwrap();
        let loaded = s.load_scan_progress().unwrap();
        assert_eq!(loaded.len(), 1, "upsert overwrites, not inserts");
        let c = loaded.get("C:/a.jsonl").unwrap();
        assert_eq!(c.last_modified, 2);
        assert_eq!(c.last_line_offset, 10);
    }

    #[test]
    fn scan_progress_load_empty_on_fresh_db() {
        let s = mem();
        assert!(
            s.load_scan_progress().unwrap().is_empty(),
            "fresh DB has no cursors ⇒ first collect is a full scan"
        );
    }

    #[test]
    fn scan_progress_save_empty_delta_is_noop() {
        let s = mem();
        let delta = ScanProgressDelta::new();
        s.save_scan_progress(&delta).unwrap();
        assert!(s.load_scan_progress().unwrap().is_empty());
    }

    /// Helper: insert one session row with a given last_active_at.
    fn seed_session(store: &Store, id: &str, device: &str, last_active: &str) {
        store
            .upsert_session(
                device,
                &SessionSystemData {
                    id: id.into(),
                    source: "claude_code".into(),
                    project_dir: "/proj".into(),
                    title_orig: "Title".into(),
                    started_at: "2026-08-01T00:00:00.000Z".into(),
                    last_active_at: last_active.into(),
                },
            )
            .unwrap();
    }

    #[test]
    fn query_sessions_time_range_filters_last_active_at() {
        let s = mem();
        seed_session(&s, "old", "dev", "2026-08-01T10:00:00.000Z");
        seed_session(&s, "mid", "dev", "2026-08-15T10:00:00.000Z");
        seed_session(&s, "new", "dev", "2026-08-31T10:00:00.000Z");

        // from_ts narrows to sessions at or after Aug 10.
        let from = SessionFilter {
            from_ts: Some("2026-08-10T00:00:00.000Z".into()),
            ..Default::default()
        };
        let ids: Vec<String> = s
            .query_sessions(Some(&from))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["new", "mid"], "from_ts excludes early sessions");

        // to_ts narrows to sessions at or before Aug 20.
        let to = SessionFilter {
            to_ts: Some("2026-08-20T23:59:59.999Z".into()),
            ..Default::default()
        };
        let ids: Vec<String> = s
            .query_sessions(Some(&to))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["mid", "old"], "to_ts excludes late sessions");

        // both bounds → only "mid".
        let both = SessionFilter {
            from_ts: Some("2026-08-10T00:00:00.000Z".into()),
            to_ts: Some("2026-08-20T23:59:59.999Z".into()),
            ..Default::default()
        };
        let ids: Vec<String> = s
            .query_sessions(Some(&both))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["mid"], "from_ts + to_ts intersect to one session");
    }
}
