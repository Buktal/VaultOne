//! Pricing table CRUD + seed.

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
}
