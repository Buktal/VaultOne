//! Dirty-day tracking: the push path's per-day Artifact recompute driver.
//!
//! Stores the day-buckets holding un-pushed local changes and the per-day
//! source rows the push path re-exports. Also hosts the shared
//! `mark_days_dirty` helper used by the collect-side ingest path.

use super::*;
use super::schema;

impl super::Store {
    // ---------------- Dirty days (sync recompute driver) ----------------

    /// The day-buckets holding un-pushed local changes, in deterministic order
    /// (sorted). Drives the push path's per-day Artifact recompute. Read-only —
    /// it does NOT clear: clearing happens only after a push lands (see
    /// [`Self::clear_dirty_days_if_unchanged`]), so a failed push leaves the
    /// days dirty for the next retry. Pure local state: this makes no claim
    /// about the git worktree and never reads it.
    pub fn dirty_days(&self) -> AppResult<Vec<String>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare("SELECT day FROM dirty_days ORDER BY day")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Clear the dirty flag for each day whose store contents still match the
    /// recompute-time snapshot — i.e. exactly the rows the last push committed.
    /// A collect that raced in a new row since (row count grew) keeps the day
    /// dirty so the next push carries that row up; a blind delete would silently
    /// strand it (the exact "miss git" failure the same-tx marking prevents on
    /// the write side). The check and the delete run in ONE transaction, so the
    /// flag can never be dropped after new rows land between the two. Row counts
    /// suffice: per-device rows are INSERT-only (a count mismatch exactly means
    /// "new row since the snapshot"); `forget_device` wipes a whole device, not
    /// one day, so it never hides a mismatch.
    pub fn clear_dirty_days_if_unchanged(
        &self,
        snapshots: &[(String, usize, usize)],
        device_id: &str,
    ) -> AppResult<()> {
        if snapshots.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        for (day, expected_usage, expected_turns) in snapshots {
            let usage: i64 = tx.query_row(
                "SELECT COUNT(*) FROM usage_records WHERE day = ?1 AND device_id = ?2",
                params![day, device_id],
                |r| r.get(0),
            )?;
            let turns: i64 = tx.query_row(
                "SELECT COUNT(*) FROM turn_durations WHERE day = ?1 AND device_id = ?2",
                params![day, device_id],
                |r| r.get(0),
            )?;
            if usage == *expected_usage as i64 && turns == *expected_turns as i64 {
                tx.execute("DELETE FROM dirty_days WHERE day = ?1", params![day])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every usage row for one (day, device), in uuid order — the source for the
    /// push path's per-day Artifact recompute. `ORDER BY uuid` (not collect
    /// order) is what makes the rewrite byte-stable across pushes: the same
    /// store yields the same file bytes every time, so git sees no churn once a
    /// day is settled.
    pub fn usage_for_day_device(&self, day: &str, device_id: &str) -> AppResult<Vec<UsageRecord>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        // Column list derives from the schema constant (same single source of
        // truth as `ingest_impl`'s INSERT) — column order is the field order
        // `row_to_usage_record` reads positionally.
        let select_sql = format!(
            "SELECT {} FROM usage_records WHERE day = ? AND device_id = ? ORDER BY uuid",
            schema::USAGE_RECORDS_COLNAMES
        );
        let mut stmt = conn.prepare(&select_sql)?;
        let rows = stmt.query_map(params![day, device_id], row_to_usage_record)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Every turn duration for one (day, device), in uuid order — the source for
    /// the turns Artifact recompute. Same byte-stability rationale as
    /// [`Self::usage_for_day_device`].
    pub fn turns_for_day_device(&self, day: &str, device_id: &str) -> AppResult<Vec<TurnDuration>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT uuid, timestamp, day, device_id, duration_ms
             FROM turn_durations WHERE day = ? AND device_id = ? ORDER BY uuid",
        )?;
        let rows = stmt.query_map(params![day, device_id], |r| {
            Ok(TurnDuration {
                uuid: r.get(0)?,
                timestamp: r.get(1)?,
                day: r.get(2)?,
                device_id: r.get(3)?,
                duration_ms: r.get::<_, i64>(4)? as u32,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Rebill zero-cost rows whose model now has a price (freeze +
    /// top-up zero-cost only). Returns the number of rows rebilled. Each
    /// rebilled row's day is flagged dirty IN the same transaction — the store
    /// is the single source of truth and `dirty_days` is the ONLY channel into
    /// the Artifact, so a rebill that skipped the flag would silently never
    /// reach git (same-tx rationale as [`Self::ingest_marking_dirty`]).
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

/// Reconstruct a full [`UsageRecord`] (with nested token / cost structs) from a
/// `usage_records` row — the inverse of `Store::ingest_impl`'s insert. Used by
/// the push path's per-day recompute to serialize the day's full content.
fn row_to_usage_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageRecord> {
    use std::str::FromStr;
    let dec =
        |s: String| rust_decimal::Decimal::from_str(&s).unwrap_or(rust_decimal::Decimal::ZERO);
    let total = dec(r.get::<_, String>(20)?);
    Ok(UsageRecord {
        uuid: r.get(0)?,
        timestamp: r.get(1)?,
        day: r.get(2)?,
        model: r.get(3)?,
        pricing_model: r.get(4)?,
        source: r.get(5)?,
        session_id: r.get(6)?,
        device_id: r.get(7)?,
        tokens: TokenCounts {
            input: r.get::<_, i64>(8)? as u32,
            output: r.get::<_, i64>(9)? as u32,
            cache_creation: r.get::<_, i64>(10)? as u32,
            cache_read: r.get::<_, i64>(11)? as u32,
        },
        server_tool_use: serde_json::from_str(&r.get::<_, String>(12)?)
            .unwrap_or(crate::model::ServerToolUse::default()),
        stop_reason: r.get(13)?,
        service_tier: r.get(14)?,
        iterations: r.get::<_, i64>(15)? as u32,
        cost: crate::model::CostBreakdown {
            input_usd: dec(r.get::<_, String>(16)?),
            output_usd: dec(r.get::<_, String>(17)?),
            cache_read_usd: dec(r.get::<_, String>(18)?),
            cache_creation_usd: dec(r.get::<_, String>(19)?),
            total_usd: total,
        },
    })
}

/// Flag each day in `days` as dirty, within `tx` so the flag lands atomically
/// with the row writes that made them dirty (a separate transaction could leave
/// a written row whose day is never flagged, silently dropping it from the next
/// push). `INSERT OR IGNORE` keeps it idempotent across collects.
pub(super) fn mark_days_dirty(
    tx: &rusqlite::Transaction,
    days: &std::collections::BTreeSet<String>,
) -> AppResult<()> {
    if days.is_empty() {
        return Ok(());
    }
    let mut stmt = tx.prepare("INSERT OR IGNORE INTO dirty_days(day) VALUES (?1)")?;
    for day in days {
        stmt.execute(params![day])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testutil::*;

    // ---- dirty_days (sync recompute driver) ----

    /// The local-collect ingest flags each newly inserted row's day dirty, in
    /// the same transaction as the write. A collect that ingests rows on D1 and
    /// D2 leaves exactly {D1, D2} dirty (deduped, sorted).
    #[test]
    fn ingest_marking_dirty_flags_days_of_new_rows() {
        let s = mem();
        s.ingest_marking_dirty(&[
            rec("a", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0),
            rec("b", "2026-07-14", "glm-5.2", "dev1", 200, 0, 2.0),
            rec("c", "2026-07-13", "gpt-4o", "dev1", 10, 0, 0.0),
        ])
        .unwrap();
        assert_eq!(
            s.dirty_days().unwrap(),
            vec!["2026-07-13".to_string(), "2026-07-14".to_string()],
            "D1 (two rows) + D2, deduped and sorted"
        );
    }

    /// The pull ingest path must NOT flag days dirty — imported rows are already
    /// on git, so flagging them would only cause spurious recomputes and muddy
    /// the "local dirtiness" invariant.
    #[test]
    fn pull_ingest_does_not_flag_days_dirty() {
        let s = mem();
        s.ingest(&[rec("a", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0)])
            .unwrap();
        assert!(
            s.dirty_days().unwrap().is_empty(),
            "pull-path ingest must not flag days dirty"
        );
    }

    /// Re-ingesting already-known rows (uuid dedup) writes nothing new, so it
    /// must not flag any day dirty — otherwise a retried collect would re-dirty
    /// settled days forever. (Clearing first proves the second ingest adds nil.)
    #[test]
    fn deduped_reingest_does_not_flag_dirty() {
        let s = mem();
        let r = rec("a", "2026-07-13", "glm-5.2", "dev1", 100, 50, 1.0);
        s.ingest_marking_dirty(std::slice::from_ref(&r)).unwrap();
        // Snapshot (1 usage row, 0 turns) still matches ⇒ cleared.
        s.clear_dirty_days_if_unchanged(&[("2026-07-13".into(), 1, 0)], "dev1")
            .unwrap();
        // Same uuid again ⇒ no new row ⇒ no dirty flag.
        s.ingest_marking_dirty(std::slice::from_ref(&r)).unwrap();
        assert!(s.dirty_days().unwrap().is_empty());
    }

    /// Regression (review): a day whose store grew between the recompute
    /// snapshot and the clear must NOT be cleared — the new row has not reached
    /// git, and clearing would strand it forever. The blind
    /// `DELETE WHERE day IN (...)` this snapshot API replaces could not tell
    /// the two apart.
    #[test]
    fn clear_keeps_day_dirty_when_rows_grew_since_snapshot() {
        let s = mem();
        s.ingest_marking_dirty(std::slice::from_ref(&rec(
            "a",
            "2026-07-13",
            "glm-5.2",
            "dev1",
            100,
            50,
            1.0,
        )))
        .unwrap();
        // Snapshot taken at recompute time: 1 usage row for the day. A
        // concurrent collect lands a second row for the SAME day before the
        // push's clear runs.
        s.ingest_marking_dirty(std::slice::from_ref(&rec(
            "b",
            "2026-07-13",
            "glm-5.2",
            "dev1",
            10,
            20,
            2.0,
        )))
        .unwrap();
        s.clear_dirty_days_if_unchanged(&[("2026-07-13".into(), 1, 0)], "dev1")
            .unwrap();
        assert_eq!(
            s.dirty_days().unwrap(),
            vec!["2026-07-13".to_string()],
            "day with a post-snapshot row stays dirty"
        );
    }

    /// Turn ingest marks its days dirty on the collect path too (one shared
    /// dirty_days set serves both grains).
    #[test]
    fn turn_ingest_marking_dirty_flags_days() {
        let s = mem();
        s.ingest_turn_durations_marking_dirty(&[
            TurnDuration {
                uuid: "t1".into(),
                timestamp: "2026-07-13T10:00:00Z".into(),
                day: "2026-07-13".into(),
                device_id: "d".into(),
                duration_ms: 100_000,
            },
            TurnDuration {
                uuid: "t2".into(),
                timestamp: "2026-07-14T11:00:00Z".into(),
                day: "2026-07-14".into(),
                device_id: "d".into(),
                duration_ms: 200_000,
            },
        ])
        .unwrap();
        assert_eq!(
            s.dirty_days().unwrap(),
            vec!["2026-07-13".to_string(), "2026-07-14".to_string()]
        );
        // Pull-path turn ingest does not flag. (Snapshots still match — the
        // turn count per day is 1.)
        s.clear_dirty_days_if_unchanged(
            &[("2026-07-13".into(), 0, 1), ("2026-07-14".into(), 0, 1)],
            "d",
        )
        .unwrap();
        s.ingest_turn_durations(&[TurnDuration {
            uuid: "t3".into(),
            timestamp: "2026-07-15T10:00:00Z".into(),
            day: "2026-07-15".into(),
            device_id: "d".into(),
            duration_ms: 1,
        }])
        .unwrap();
        assert!(
            s.dirty_days().unwrap().is_empty(),
            "pull turn ingest no flag"
        );
    }

    /// dirty_days accumulates across separate collects (a day stays dirty until
    /// the push path clears it).
    #[test]
    fn dirty_days_accumulate_across_collects() {
        let s = mem();
        s.ingest_marking_dirty(&[rec("a", "2026-07-13", "glm-5.2", "d", 1, 0, 0.0)])
            .unwrap();
        s.ingest_marking_dirty(&[rec("b", "2026-07-14", "glm-5.2", "d", 1, 0, 0.0)])
            .unwrap();
        assert_eq!(
            s.dirty_days().unwrap(),
            vec!["2026-07-13".to_string(), "2026-07-14".to_string()]
        );
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
