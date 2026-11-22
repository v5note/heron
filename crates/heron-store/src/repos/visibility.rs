//! The visibility cache (design §7: "可见性判定缓存"). `verify` writes one row per projection;
//! the next `verify` reads them back to tell a first sighting from a confirmed landing.

use heron_core::Result;
use rusqlite::params;

use crate::db::WriteTx;
use crate::models::VisibilityRow;
use crate::sql_err;

pub fn list(conn: &rusqlite::Connection) -> Result<Vec<VisibilityRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT projection_id, client_id, state, shadowed_by, checked_at FROM visibility",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(VisibilityRow {
                projection_id: r.get(0)?,
                client_id: r.get(1)?,
                state: r.get(2)?,
                shadowed_by: r.get(3)?,
                checked_at: r.get(4)?,
            })
        })
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn upsert(tx: &WriteTx<'_>, row: &VisibilityRow) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO visibility (projection_id, client_id, state, shadowed_by, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(projection_id) DO UPDATE SET
               client_id = excluded.client_id,
               state = excluded.state,
               shadowed_by = excluded.shadowed_by,
               checked_at = excluded.checked_at",
            params![
                row.projection_id,
                row.client_id,
                row.state,
                row.shadowed_by,
                row.checked_at
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}
