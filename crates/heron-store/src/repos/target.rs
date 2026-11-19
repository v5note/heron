use heron_core::{ErrorCode, Result};
use rusqlite::{params, OptionalExtension};

use crate::db::WriteTx;
use crate::models::TargetRow;
use crate::sql_err;

const COLUMNS: &str = "id, kind, scope, project_path, path, enabled, is_default, discover";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<TargetRow> {
    Ok(TargetRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        scope: r.get(2)?,
        project_path: r.get(3)?,
        path: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        is_default: r.get::<_, i64>(6)? != 0,
        discover: r.get(7)?,
    })
}

pub fn upsert(tx: &WriteTx<'_>, row: &TargetRow) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO target (id, kind, scope, project_path, path, enabled, is_default, discover)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind, scope = excluded.scope, project_path = excluded.project_path,
                path = excluded.path, enabled = excluded.enabled, discover = excluded.discover",
            params![
                row.id,
                row.kind,
                row.scope,
                row.project_path,
                row.path,
                row.enabled as i64,
                row.is_default as i64,
                row.discover,
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn list(conn: &rusqlite::Connection) -> Result<Vec<TargetRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM target ORDER BY is_default DESC, kind, scope, path"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn get(conn: &rusqlite::Connection, id: &str) -> Result<Option<TargetRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM target WHERE id = ?1"),
        [id],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

/// The default landing target (decision D10: `~/.heron/skills`).
pub fn get_default(conn: &rusqlite::Connection) -> Result<Option<TargetRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM target WHERE is_default = 1 AND enabled = 1 LIMIT 1"),
        [],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

pub fn set_default(tx: &WriteTx<'_>, id: &str) -> Result<()> {
    if get(tx.conn(), id)?.is_none() {
        return Err(ErrorCode::TargetNotFound
            .msg()
            .with("target", id.to_owned())
            .into());
    }
    tx.conn()
        .execute("UPDATE target SET is_default = 0 WHERE is_default = 1", [])
        .map_err(sql_err)?;
    tx.conn()
        .execute("UPDATE target SET is_default = 1 WHERE id = ?1", [id])
        .map_err(sql_err)?;
    Ok(())
}

pub fn remove(tx: &WriteTx<'_>, id: &str) -> Result<()> {
    let changed = tx
        .conn()
        .execute("DELETE FROM target WHERE id = ?1", [id])
        .map_err(sql_err)?;
    if changed == 0 {
        return Err(ErrorCode::TargetNotFound
            .msg()
            .with("target", id.to_owned())
            .into());
    }
    Ok(())
}
