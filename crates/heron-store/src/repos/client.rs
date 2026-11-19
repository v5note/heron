use heron_core::Result;
use rusqlite::params;

use crate::db::WriteTx;
use crate::models::ClientRow;
use crate::repos::{json_list, parse_list};
use crate::sql_err;

pub fn upsert(tx: &WriteTx<'_>, row: &ClientRow) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO client (id, display_name, global_paths, project_paths, link_modes,
                                 global_commands, project_commands, priority,
                                 native_scan, verified_at, verified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                global_paths = excluded.global_paths,
                project_paths = excluded.project_paths,
                link_modes = excluded.link_modes,
                global_commands = excluded.global_commands,
                project_commands = excluded.project_commands,
                priority = excluded.priority,
                native_scan = excluded.native_scan,
                verified_at = excluded.verified_at,
                verified_by = excluded.verified_by",
            params![
                row.id,
                row.display_name,
                json_list(&row.global_paths),
                json_list(&row.project_paths),
                json_list(&row.link_modes),
                json_list(&row.global_commands),
                json_list(&row.project_commands),
                row.priority,
                row.native_scan,
                row.verified_at,
                row.verified_by,
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn list(conn: &rusqlite::Connection) -> Result<Vec<ClientRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, display_name, global_paths, project_paths, link_modes,
                    global_commands, project_commands, priority, native_scan, verified_at, verified_by
             FROM client ORDER BY priority, id",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ClientRow {
                id: r.get(0)?,
                display_name: r.get(1)?,
                global_paths: parse_list(Some(r.get(2)?)),
                project_paths: parse_list(Some(r.get(3)?)),
                link_modes: parse_list(Some(r.get(4)?)),
                global_commands: parse_list(Some(r.get(5)?)),
                project_commands: parse_list(Some(r.get(6)?)),
                priority: r.get(7)?,
                native_scan: r.get(8)?,
                verified_at: r.get(9)?,
                verified_by: r.get(10)?,
            })
        })
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn get(conn: &rusqlite::Connection, id: &str) -> Result<Option<ClientRow>> {
    Ok(list(conn)?.into_iter().find(|c| c.id == id))
}
