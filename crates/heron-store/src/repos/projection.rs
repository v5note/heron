use heron_core::Result;
use rusqlite::params;

use crate::db::{now_iso, WriteTx};
use crate::models::ProjectionRow;
use crate::sql_err;

const COLUMNS: &str = "id, slug, target_id, link_name, path, mode, version, content_hash, inode_kind, state, created_at";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectionRow> {
    Ok(ProjectionRow {
        id: r.get(0)?,
        slug: r.get(1)?,
        target_id: r.get(2)?,
        link_name: r.get(3)?,
        path: r.get(4)?,
        mode: r.get(5)?,
        version: r.get(6)?,
        content_hash: r.get(7)?,
        inode_kind: r.get(8)?,
        state: r.get(9)?,
        created_at: r.get(10)?,
    })
}

pub fn insert(tx: &WriteTx<'_>, row: &ProjectionRow) -> Result<i64> {
    let created = if row.created_at.is_empty() {
        now_iso()
    } else {
        row.created_at.clone()
    };
    let state = if row.state.is_empty() {
        "ok"
    } else {
        row.state.as_str()
    };
    tx.conn()
        .execute(
            "INSERT INTO projection (slug, target_id, link_name, path, mode, version, content_hash, inode_kind, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.slug,
                row.target_id,
                row.link_name,
                row.path,
                row.mode,
                row.version,
                row.content_hash,
                row.inode_kind,
                state,
                created,
            ],
        )
        .map_err(sql_err)?;
    Ok(tx.conn().last_insert_rowid())
}

pub fn list_for_skill(conn: &rusqlite::Connection, slug: &str) -> Result<Vec<ProjectionRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM projection WHERE slug = ?1 ORDER BY target_id"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([slug], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn list_all(conn: &rusqlite::Connection) -> Result<Vec<ProjectionRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM projection ORDER BY slug, target_id"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn set_state(tx: &WriteTx<'_>, id: i64, state: &str) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE projection SET state = ?2 WHERE id = ?1",
            params![id, state],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn delete(tx: &WriteTx<'_>, id: i64) -> Result<()> {
    tx.conn()
        .execute("DELETE FROM projection WHERE id = ?1", [id])
        .map_err(sql_err)?;
    Ok(())
}
