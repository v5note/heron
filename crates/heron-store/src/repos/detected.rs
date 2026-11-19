//! The `detected` table holds everything the scanner saw that Heron does not own yet.

use std::collections::HashSet;

use heron_core::Result;
use rusqlite::params;

use crate::db::{now_iso, WriteTx};
use crate::models::{DetectedRow, DetectedUpsert};
use crate::sql_err;

const COLUMNS: &str = "path, real_path, client_id, scope, project_path, observed_slug, content_hash, frontmatter_ok, kind, last_seen_at, adopt_state, mtime_ns, size_bytes";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<DetectedRow> {
    Ok(DetectedRow {
        path: r.get(0)?,
        real_path: r.get(1)?,
        client_id: r.get(2)?,
        scope: r.get(3)?,
        project_path: r.get(4)?,
        observed_slug: r.get(5)?,
        content_hash: r.get(6)?,
        frontmatter_ok: r.get::<_, Option<i64>>(7)?.map(|v| v != 0),
        kind: r.get(8)?,
        last_seen_at: r.get(9)?,
        adopt_state: r.get(10)?,
        mtime_ns: r.get(11)?,
        size_bytes: r.get(12)?,
    })
}

pub fn upsert(tx: &WriteTx<'_>, row: &DetectedUpsert) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO detected (path, real_path, client_id, scope, project_path, observed_slug,
                                   content_hash, frontmatter_ok, kind, last_seen_at, adopt_state,
                                   mtime_ns, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'candidate', ?11, ?12)
             ON CONFLICT(path) DO UPDATE SET
                real_path = excluded.real_path,
                client_id = excluded.client_id,
                scope = excluded.scope,
                project_path = excluded.project_path,
                observed_slug = excluded.observed_slug,
                content_hash = excluded.content_hash,
                frontmatter_ok = excluded.frontmatter_ok,
                kind = excluded.kind,
                last_seen_at = excluded.last_seen_at,
                mtime_ns = excluded.mtime_ns,
                size_bytes = excluded.size_bytes",
            params![
                row.path,
                row.real_path,
                row.client_id,
                row.scope,
                row.project_path,
                row.observed_slug,
                row.content_hash,
                row.frontmatter_ok.map(|b| b as i64),
                row.kind,
                now_iso(),
                row.mtime_ns,
                row.size_bytes,
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn list_by_state(conn: &rusqlite::Connection, state: &str) -> Result<Vec<DetectedRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM detected WHERE adopt_state = ?1 ORDER BY path"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([state], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn list_all(conn: &rusqlite::Connection) -> Result<Vec<DetectedRow>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLUMNS} FROM detected ORDER BY path"))
        .map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn count_by_kind(conn: &rusqlite::Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare("SELECT kind, COUNT(*) FROM detected GROUP BY kind ORDER BY kind")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

/// Drop rows that were not seen during the scan identified by `seen` (deleted skills).
/// Rows the user explicitly ignored are kept so the decision survives rescans.
pub fn prune_not_in(tx: &WriteTx<'_>, seen: &HashSet<String>) -> Result<usize> {
    let mut removed = 0usize;
    let mut stmt = tx
        .conn()
        .prepare("SELECT path FROM detected WHERE adopt_state <> 'ignored'")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(sql_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)?;
    for path in rows {
        if !seen.contains(&path) {
            removed += tx
                .conn()
                .execute("DELETE FROM detected WHERE path = ?1", [&path])
                .map_err(sql_err)?;
        }
    }
    Ok(removed)
}

pub fn set_state(tx: &WriteTx<'_>, path: &str, state: &str) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE detected SET adopt_state = ?2 WHERE path = ?1",
            params![path, state],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn get(conn: &rusqlite::Connection, path: &str) -> Result<Option<DetectedRow>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM detected WHERE path = ?1"),
        [path],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}
