//! Byte-stable, tolerant JSONL file I/O — the shared primitive the derived
//! snapshot modules (`artifact`, `session_snapshot`) build on.
//!
//! Two shapes: a full **rewrite** (truncate + write every row on its own line,
//! removing the file when there are no rows) and a tolerant **read** (skip
//! blank/unparseable lines). The rewrite is byte-stable by contract: the caller
//! supplies rows in a deterministic order and serde emits fields in declaration
//! order, so the same rows always serialize to the same bytes (no git churn once
//! a file settles). An append helper survives for tests that stand a file up
//! directly.

use std::path::Path;

use crate::error::AppResult;

/// Full rewrite of one JSONL file from its rows: truncate and write every row as
/// its own JSON line. Byte-stable by construction — the caller supplies the rows
/// in a deterministic order and serde emits fields in declaration order, so the
/// same rows always serialize to the same bytes. An empty row set means the file
/// should not exist, so it is removed rather than left empty.
pub(crate) fn rewrite_day_file<T: serde::Serialize>(path: &Path, rows: &[T]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if rows.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let mut out = String::new();
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Read every row from one JSONL file. Blank and unparseable lines are skipped —
/// a partially-corrupt peer file must not abort a read.
pub(crate) fn read_jsonl_file_of<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<Vec<T>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(r) = serde_json::from_str::<T>(line) {
            out.push(r);
        }
    }
    Ok(out)
}

/// Open once in append mode, serialize + writeln each row. Test-fixture only:
/// production writes derived snapshots via the full-rewrite path
/// ([`rewrite_day_file`]); this append helper survives for tests that stand a
/// file up directly.
#[cfg(test)]
pub(crate) fn write_jsonl_day<T: serde::Serialize>(path: &Path, rows: &[&T]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for r in rows {
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}
