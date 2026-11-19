//! SQLite index.
//!
//! The index is always derivable from the store directory plus the file system
//! (implementation doc §10.9), so it is safe to delete and rebuild. Migrations are
//! append-only and run under a process lock (§2.2).

pub mod db;
pub mod models;
pub mod repos;

pub use db::{to_msg, Db, WriteTx};
pub use heron_core::{HeronError, Msg, Result};

/// Convert a `rusqlite` error into a Heron error carrying `E_DB_CORRUPT`.
pub fn sql_err(e: rusqlite::Error) -> HeronError {
    HeronError::Msg(
        heron_core::ErrorCode::DbCorrupt
            .msg()
            .with("detail", e.to_string()),
    )
}
