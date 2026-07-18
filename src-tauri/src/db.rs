//! SQLite Local Store (ADR-0004 / 0005 / 0007 / 0008 / 0009).
//!
//! Owns the schema, the dedup ledger, daily rollups cache, pricing table and
//! device registry. Exposes typed read methods (stats / trend / logs / models)
//! and write methods (ingest, pricing CRUD, rebill) — the JS layer never sees
//! SQL (ADR-0007 query boundary; ADR-0008 typed commands).
//!
//! Cost columns are `rust_decimal::Decimal` stored as TEXT (ADR-0009); sums over
//! them read back as REAL for display (f64 is display-only — JS never recomputes
//! cost).

use std::sync::Mutex;

use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::{
    LogsQuery, ModelStatsRow, PricingEntry, TokenCounts, TrendPoint, UsageFilter, UsageLogRow,
    UsageRecord, UsageStats,
};
use crate::pricing::{ModelPricing, PricingBook};

/// Schema DDL (ADR-0002 / 0004 / 0009). `IF NOT EXISTS` ⇒ idempotent migration.
pub const SCHEMA: &str = include_str!("db_schema.sql");

/// Thread-safe wrapper over a single SQLite connection.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (or create) `vaultone.db` and ensure the schema + seed pricing.
    pub fn open(path: &std::path::Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.ensure_pricing_seed()?;
        Ok(store)
    }

    /// Seed the pricing table from the built-in book if it is empty (ADR-0006).
    fn ensure_pricing_seed(&self) -> AppResult<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM model_pricing", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }
        let now = now_iso();
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
                now_iso(),
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

    /// Insert a batch of records, deduping by uuid via the ledger (ADR-0005).
    /// Returns the newly imported rows (in order). Recomputes affected rollups.
    pub fn ingest(&self, records: &[UsageRecord]) -> AppResult<Vec<UsageRecord>> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        // Batch existence check: which uuids are already in the ledger?
        let uuids: Vec<String> = records.iter().map(|r| r.uuid.clone()).collect();
        let placeholders = (0..uuids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT uuid FROM ledger WHERE uuid IN ({placeholders})");
        let known: std::collections::HashSet<String> = {
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(uuids.iter()), |r| r.get::<_, String>(0))?;
            rows.filter_map(Result::ok).collect()
        };

        let now = now_iso();
        let mut new_days: std::collections::HashSet<(String, String, String)> = Default::default();
        let mut inserted: Vec<UsageRecord> = Vec::new();
        for r in records {
            if known.contains(&r.uuid) {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO usage_records
                 (uuid, timestamp, day, model, pricing_model, source, device_id,
                  input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                  server_tool_use, input_cost_usd, output_cost_usd, cache_read_cost_usd,
                  cache_creation_cost_usd, total_cost_usd)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    r.uuid,
                    r.timestamp,
                    r.day,
                    r.model,
                    r.pricing_model,
                    r.source,
                    r.device_id,
                    r.tokens.input as i64,
                    r.tokens.output as i64,
                    r.tokens.cache_creation as i64,
                    r.tokens.cache_read as i64,
                    serde_json::to_string(&r.server_tool_use).unwrap_or_else(|_| "{}".into()),
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

    /// Rebill zero-cost rows whose model now has a price (ADR-0009: freeze +
    /// top-up zero-cost only). Returns the number of rows rebilled.
    pub fn rebill_zero_cost(&self, book: &PricingBook) -> AppResult<usize> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare(
            "SELECT uuid, pricing_model, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens
             FROM usage_records
             WHERE CAST(total_cost_usd AS REAL) <= 0",
        )?;
        let candidates: Vec<(String, String, TokenCounts)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    TokenCounts {
                        input: r.get::<_, i64>(2)? as u32,
                        output: r.get::<_, i64>(3)? as u32,
                        cache_creation: r.get::<_, i64>(4)? as u32,
                        cache_read: r.get::<_, i64>(5)? as u32,
                    },
                ))
            })?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        let mut rebilled = 0usize;
        let mut affected: std::collections::HashSet<(String, String, String)> = Default::default();
        for (uuid, model, tokens) in candidates {
            let Some(rate) = book.resolve(&model) else {
                continue;
            };
            let cost = crate::pricing::CostCalculator::calc(tokens, Some(rate));
            if cost.total_usd <= rust_decimal::Decimal::ZERO {
                continue;
            }
            // fetch day/device for rollup recompute
            let (day, device): (String, String) = tx.query_row(
                "SELECT day, device_id FROM usage_records WHERE uuid = ?1",
                params![uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            tx.execute(
                "UPDATE usage_records SET
                   input_cost_usd=?1, output_cost_usd=?2, cache_read_cost_usd=?3,
                   cache_creation_cost_usd=?4, total_cost_usd=?5
                 WHERE uuid=?6",
                params![
                    cost.input_usd.to_string(),
                    cost.output_usd.to_string(),
                    cost.cache_read_usd.to_string(),
                    cost.cache_creation_usd.to_string(),
                    cost.total_usd.to_string(),
                    uuid,
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

    /// Register/refresh a device in the registry (ADR-0002).
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
            params![device_id, display_name, is_self as i64, now_iso()],
        )?;
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

    // ---------------- Reads (dashboard) ----------------

    /// Aggregate stats over a filter (BLUEPRINT 使用统计).
    pub fn query_stats(&self, filter: &UsageFilter) -> AppResult<UsageStats> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter);
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
        Ok(s)
    }

    /// Per-model breakdown over a filter.
    pub fn query_models(&self, filter: &UsageFilter) -> AppResult<Vec<ModelStatsRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter);
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

    /// Daily trend points over a filter (BLUEPRINT 使用趋势).
    pub fn query_trend(&self, filter: &UsageFilter) -> AppResult<Vec<TrendPoint>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter);
        let sql = format!(
            "SELECT day,
                COALESCE(SUM(input_tokens),0),
                COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_creation_tokens),0),
                COALESCE(SUM(cache_read_tokens),0),
                COALESCE(SUM(CAST(total_cost_usd AS REAL)),0)
             FROM usage_records {clause}
             GROUP BY day ORDER BY day"
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

    /// Request-log rows (BLUEPRINT 请求日志; ADR-0003 columns).
    pub fn query_logs(&self, q: &LogsQuery) -> AppResult<Vec<UsageLogRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(&q.filter);
        let limit = q.limit.clamp(1, 1000) as i64;
        let offset = q.offset as i64;
        let sql = format!(
            "SELECT uuid, timestamp, model, source, device_id,
                    input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                    CAST(total_cost_usd AS REAL)
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
                total_cost_usd: r.get(9)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Total row count (for paging display).
    pub fn count_logs(&self, filter: &UsageFilter) -> AppResult<u32> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (clause, params_vec) = build_where(filter);
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

/// Build a `WHERE` clause + bound params for a `UsageFilter` (day range, model,
/// source, device scope). Returns `("WHERE ...", vec![...])` or `("", [])`.
fn build_where(filter: &UsageFilter) -> (String, Vec<SqlValue>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(d) = &filter.from_day {
        if !d.is_empty() {
            conds.push("day >= ?".into());
            params.push(SqlValue::Text(d.clone()));
        }
    }
    if let Some(d) = &filter.to_day {
        if !d.is_empty() {
            conds.push("day <= ?".into());
            params.push(SqlValue::Text(d.clone()));
        }
    }
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

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerToolUse;
    use crate::pricing;
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
            pricing_model: pricing::normalize_key(model),
            source: "claude_code".into(),
            device_id: device.into(),
            tokens: TokenCounts {
                input,
                output,
                cache_creation: 0,
                cache_read: 0,
            },
            server_tool_use: ServerToolUse::default(),
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
    fn ingest_inserts_then_dedups_same_uuid() {
        let s = mem();
        let r = rec("u1", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0);
        assert_eq!(s.ingest(std::slice::from_ref(&r)).unwrap().len(), 1);
        assert_eq!(s.ingest(&[r]).unwrap().len(), 0, "same uuid must dedupe");
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

        let trend = s.query_trend(&UsageFilter::default()).unwrap();
        assert_eq!(trend.len(), 2);
        assert_eq!(trend[0].day, "2026-07-13");
        assert_eq!(trend[0].total_tokens, 450);
    }

    #[test]
    fn filters_by_day_range_and_model() {
        let s = mem();
        s.ingest(&[
            rec("a", "2026-07-13", "glm-5.2", "d", 10, 0, 1.0),
            rec("b", "2026-07-14", "gpt-4o", "d", 20, 0, 2.0),
        ])
        .unwrap();
        let by_day = UsageFilter {
            from_day: Some("2026-07-14".into()),
            ..Default::default()
        };
        assert_eq!(s.query_stats(&by_day).unwrap().request_count, 1);
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
}
