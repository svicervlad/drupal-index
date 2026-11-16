//! Error types for `drupal-index`.

use std::path::PathBuf;
use thiserror::Error;

/// Result alias for `drupal-index` operations.
pub type Result<T> = std::result::Result<T, IndexError>;

/// Errors returned by `drupal-index`.
#[derive(Debug, Error)]
pub enum IndexError {
    /// Filesystem I/O failure.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path associated with the failing operation.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// `SQLite` database error.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    /// Git CLI invocation failure.
    #[error("git command failed: {0}")]
    Git(String),

    /// Composer CLI invocation failure.
    #[error("composer command failed: {0}")]
    Composer(String),

    /// Tree-sitter parser configuration error.
    #[error("tree-sitter setup failed: {0}")]
    Parser(String),

    /// Invalid user input or CLI argument.
    #[error("invalid argument: {0}")]
    InvalidArg(String),
}
