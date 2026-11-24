use heron_core::Result;
use rusqlite::params;

use crate::db::WriteTx;
use crate::models::ClientRow;
use crate::repos::{json_list, parse_list};
use crate::sql_err;

/// Writes the catalogue fields only. Presence has its own path ([`set_presence`]) so that
/// re-bootstrapping the catalogue can never reset a detection verdict, and a detection write can
/// never reset the catalogue. Do not add the presence columns to this statement.
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
                    global_commands, project_commands, priority, native_scan, verified_at, verified_by,
                    detected, detected_at, detect_evidence
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
                detected: r.get(11)?,
                detected_at: r.get(12)?,
                detect_evidence: parse_list(Some(r.get(13)?)),
            })
        })
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn get(conn: &rusqlite::Connection, id: &str) -> Result<Option<ClientRow>> {
    Ok(list(conn)?.into_iter().find(|c| c.id == id))
}

/// Narrow write of the presence verdict for one client. Deliberately touches only the three
/// presence columns so it cannot clobber catalogue fields (see [`upsert`]'s comment).
pub fn set_presence(
    tx: &WriteTx<'_>,
    id: &str,
    detected: Option<bool>,
    at: &str,
    evidence: &[String],
) -> Result<usize> {
    tx.conn()
        .execute(
            "UPDATE client SET detected = ?2, detected_at = ?3, detect_evidence = ?4 WHERE id = ?1",
            params![id, detected.map(|d| d as i64), at, json_list(evidence),],
        )
        .map_err(sql_err)
}
