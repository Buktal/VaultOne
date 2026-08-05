//! Pricing table CRUD + seed + zero-cost rebill.

use super::store_dirty_days::mark_days_dirty;
use super::*;

impl super::Store {
    /// Seed the pricing table from the built-in book if it is empty.
    pub(super) fn ensure_pricing_seed(&self) -> AppResult<()> {
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

    /// Rebill zero-cost rows whose model now has a price (freeze + top-up
    /// zero-cost only). Returns the number of rows rebilled. Each rebilled row's
    /// day is flagged dirty IN the same transaction — the store is the single
    /// source of truth and `dirty_days` is the ONLY channel into the Artifact,
    /// so a rebill that skipped the flag would silently never reach git (same-tx
    /// rationale as [`Store::ingest_marking_dirty`]).
    pub fn rebill_zero_cost(&self, book: &PricingBook) -> AppResult<usize> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare(
            "SELECT uuid, day, device_id, pricing_model, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens
             FROM usage_records
             WHERE CAST(total_cost_usd AS REAL) <= 0",
        )?;
        let candidates: Vec<(String, String, String, String, TokenCounts)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    TokenCounts {
                        input: r.get::<_, i64>(4)? as u32,
                        output: r.get::<_, i64>(5)? as u32,
                        cache_creation: r.get::<_, i64>(6)? as u32,
                        cache_read: r.get::<_, i64>(7)? as u32,
                    },
                ))
            })?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        let mut dirty = std::collections::BTreeSet::new();
        let mut rebilled = 0usize;
        for (uuid, day, device, model, tokens) in candidates {
            let Some(rate) = book.resolve(&model) else {
                continue;
            };
            let cost = crate::pricing::CostCalculator::calc(tokens, Some(rate));
            if cost.total_usd <= rust_decimal::Decimal::ZERO {
                continue;
            }
            // uuid is no longer unique across devices, so scope the update by both.
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
            dirty.insert(day);
            rebilled += 1;
        }
        mark_days_dirty(&tx, &dirty)?;
        tx.commit()?;
        Ok(rebilled)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testutil::*;

    #[test]
    fn open_seeds_builtin_pricing() {
        let s = mem();
        let entries = s.list_pricing().unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.model_key == "glm-5.2"));
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

    /// Rebill mutates stored costs — a change that must reach git. It flags the
    /// affected day dirty in the same transaction (the only channel into the
    /// Artifact), so the next push materializes the rebilled amounts.
    #[test]
    fn rebill_flags_rebilled_day_dirty() {
        let s = mem();
        s.ingest(&[rec("z", "2026-07-13", "glm-5.2", "d", 1000, 500, 0.0)])
            .unwrap();
        let book = s.load_pricing_book().unwrap();
        s.rebill_zero_cost(&book).unwrap();
        assert_eq!(
            s.dirty_days().unwrap(),
            vec!["2026-07-13".to_string()],
            "rebilled day is flagged for the next push"
        );
    }
}
