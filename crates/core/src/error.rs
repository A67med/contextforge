use std::fmt;

/// Errors produced by the ContextForge core.
///
/// Variants are deliberately coarse: callers (CLI, MCP server, desktop app) map them to user-facing
/// messages, so every variant carries a message that is safe to show to a user or an agent.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("not permitted: {0}")]
    Forbidden(String),
    #[error("git error: {0}")]
    Git(String),
}

impl Error {
    pub fn invalid(msg: impl fmt::Display) -> Self {
        Error::Invalid(msg.to_string())
    }
    pub fn not_found(msg: impl fmt::Display) -> Self {
        Error::NotFound(msg.to_string())
    }
    /// A short machine-readable code, useful for MCP error payloads and the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Db(e) if is_busy(e) => "busy",
            Error::Db(_) => "database",
            Error::Io(_) => "io",
            Error::Json(_) => "json",
            Error::NotFound(_) => "not_found",
            Error::Invalid(_) => "invalid_input",
            Error::Conflict(_) => "conflict",
            Error::Forbidden(_) => "forbidden",
            Error::Git(_) => "git",
        }
    }
}

/// True when SQLite reported lock contention that outlasted the busy timeout.
pub fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::DatabaseBusy || f.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
