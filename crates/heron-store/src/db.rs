//! Connection handling, PRAGMAs and migrations.

use std::path::Path;
use std::time::{Duration, Instant};

use heron_core::{ErrorCode, HeronError, Msg, Result};
use rusqlite::{Connection, OpenFlags};

/// (name, sql). Append-only: never edit a released entry.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("migrations/0001_init.sql")),
    (
        "0002_client_commands",
        include_str!("migrations/0002_client_commands.sql"),
    ),
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub struct Db {
    conn: Connection,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("schema_version", &SCHEMA_VERSION)
            .finish()
    }
}

impl Db {
    /// Open (creating if needed) the index at `path` and bring it to the latest schema.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ErrorCode::Permission
                    .msg()
                    .with("path", parent.display().to_string())
                    .with("detail", e.to_string())
            })?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| db_msg("open", e))?;
        Self::configure(&conn)?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// In-memory database, used by tests and by `heron doctor --rebuild-index` dry runs.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| db_msg("open_memory", e))?;
        Self::configure(&conn)?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn configure(conn: &Connection) -> Result<()> {
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| db_msg("busy_timeout", e))?;
        // WAL is a file property; harmless (and not persisted) on :memory:.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| db_msg("synchronous", e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| db_msg("foreign_keys", e))?;
        conn.pragma_update(None, "temp_store", "MEMORY")
            .map_err(|e| db_msg("temp_store", e))?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        let current: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| db_msg("read_user_version", e))?;
        if current > SCHEMA_VERSION {
            return Err(ErrorCode::DbCorrupt
                .msg()
                .with("detail", "schema_newer_than_binary")
                .with("found", current.to_string())
                .with("supported", SCHEMA_VERSION.to_string())
                .into());
        }
        for (index, (name, sql)) in MIGRATIONS.iter().enumerate() {
            let version = (index + 1) as i64;
            if version <= current {
                continue;
            }
            let tx = self.conn.transaction().map_err(|e| db_msg(name, e))?;
            tx.execute_batch(sql).map_err(|e| db_msg(name, e))?;
            tx.pragma_update(None, "user_version", version)
                .map_err(|e| db_msg(name, e))?;
            tx.commit().map_err(|e| db_msg(name, e))?;
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Run `f` inside an IMMEDIATE transaction; rolls back on error.
    pub fn write<T>(&mut self, f: impl FnOnce(&WriteTx<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| db_msg("begin", e))?;
        let out = {
            let wrapper = WriteTx { tx };
            let value = f(&wrapper)?;
            wrapper.tx.commit().map_err(|e| db_msg("commit", e))?;
            value
        };
        Ok(out)
    }

    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&self.conn)
    }

    /// `PRAGMA quick_check` — used by `doctor` and on startup after a crash.
    pub fn integrity_ok(&self) -> bool {
        let result: std::result::Result<String, _> =
            self.conn
                .query_row("PRAGMA quick_check(1)", [], |r| r.get(0));
        matches!(result, Ok(ref s) if s == "ok")
    }

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT v FROM kv WHERE k = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(|e| db_msg("kv_get", e))
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                rusqlite::params![key, value],
            )
            .map_err(|e| db_msg("kv_set", e))?;
        Ok(())
    }
}

use rusqlite::OptionalExtension;

pub struct WriteTx<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl<'a> WriteTx<'a> {
    pub fn conn(&self) -> &Connection {
        &self.tx
    }
}

/// Monotonic timer used by the scanner's "reuse cached hash" optimization: a path whose
/// mtime is older than the last scan start cannot have changed during that scan.
pub struct ScanClock {
    started: Instant,
}

impl ScanClock {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
        }
    }
    pub fn started(&self) -> Instant {
        self.started
    }
}

pub fn db_msg(stage: &str, e: rusqlite::Error) -> HeronError {
    HeronError::Msg(
        ErrorCode::DbCorrupt
            .msg()
            .with("detail", format!("{stage}: {e}")),
    )
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn to_msg(e: rusqlite::Error) -> Msg {
    ErrorCode::DbCorrupt.msg().with("detail", e.to_string())
}
