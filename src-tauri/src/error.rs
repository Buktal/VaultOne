//! Typed error channel (ADR-0008).
//!
//! Every Tauri command returns `Result<T, AppError>`. `AppError` derives
//! `specta::Type` and is serialized as a tagged enum, so the frontend receives
//! a discriminated union it can narrow on (`{ type: "Db", data: "..." }`).

/// The single error type crossing the Rust→JS boundary.
///
/// Variants are kept coarse and serializable-friendly: low-level causes
/// (io / rusqlite / git2) are stringified into `Internal` rather than leaked
/// across the boundary, so the contract stays stable and specta-friendly.
#[derive(Debug, thiserror::Error, serde::Serialize, specta::Type)]
#[serde(tag = "type", content = "data")]
pub enum AppError {
    /// The local data dir / config could not be created or read (ADR-0004).
    #[error("config error: {0}")]
    Config(String),
    /// SQLite Local Store error (ADR-0004 / 0009).
    #[error("db error: {0}")]
    Db(String),
    /// Provider failed to discover/parse Source logs (ADR-0001).
    #[error("provider error: {0}")]
    Provider(String),
    /// Pricing lookup / cost calc error (ADR-0009).
    #[error("pricing error: {0}")]
    Pricing(String),
    /// Sync (git2 / network) error — only raised in Synced mode (ADR-0010).
    #[error("sync error: {0}")]
    Sync(String),
    /// Catch-all for anything not covered above.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Config(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("serde: {e}"))
    }
}

impl From<rust_decimal::Error> for AppError {
    fn from(e: rust_decimal::Error) -> Self {
        Self::Pricing(e.to_string())
    }
}

impl From<git2::Error> for AppError {
    fn from(e: git2::Error) -> Self {
        Self::Sync(e.message().to_string())
    }
}

/// `Result` alias used throughout the backend.
pub type AppResult<T> = Result<T, AppError>;
