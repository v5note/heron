//! Audit trail. Every mutating operation registers a row here so the GUI can offer "undo".

use heron_core::Result;
use rusqlite::params;

use crate::db::{now_iso, WriteTx};
use crate::models::EventRow;
use crate::sql_err;

pub fn record(
    tx: &WriteTx<'_>,
    actor: &str,
    op: &str,
    subject: Option<&str>,
    detail: Option<&str>,
    undoable: bool,
) -> Result<i64> {
    tx.conn()
        .execute(
            "INSERT INTO event (ts, actor, op, subject, detail, undoable) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_iso(), actor, op, subject, detail, undoable as i64],
        )
        .map_err(sql_err)?;
    Ok(tx.conn().last_insert_rowid())
}

pub fn push_undo_step(tx: &WriteTx<'_>, event_id: i64, seq: i64, action_json: &str) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO undo_step (event_id, seq, action) VALUES (?1, ?2, ?3)",
            params![event_id, seq, action_json],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn undo_steps(conn: &rusqlite::Connection, event_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT action FROM undo_step WHERE event_id = ?1 ORDER BY seq DESC")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([event_id], |r| r.get::<_, String>(0))
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn finish(tx: &WriteTx<'_>, event_id: i64, undoable: bool) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE event SET undoable = ?2 WHERE id = ?1",
            params![event_id, undoable as i64],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn pending_undoable(conn: &rusqlite::Connection) -> Result<Vec<EventRow>> {
    let mut stmt = conn
        .prepare("SELECT id, ts, actor, op, subject, detail, undoable FROM event WHERE undoable = 1 ORDER BY id DESC")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(EventRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                actor: r.get(2)?,
                op: r.get(3)?,
                subject: r.get(4)?,
                detail: r.get(5)?,
                undoable: r.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn recent(conn: &rusqlite::Connection, limit: usize) -> Result<Vec<EventRow>> {
    let mut stmt = conn
        .prepare("SELECT id, ts, actor, op, subject, detail, undoable FROM event ORDER BY id DESC LIMIT ?1")
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok(EventRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                actor: r.get(2)?,
                op: r.get(3)?,
                subject: r.get(4)?,
                detail: r.get(5)?,
                undoable: r.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}
