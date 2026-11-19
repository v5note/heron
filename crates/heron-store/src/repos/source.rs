use heron_core::Result;
use rusqlite::{params, OptionalExtension};

use crate::db::WriteTx;
use crate::models::SourceRow;
use crate::sql_err;

const COLUMNS: &str = "id, name, kind, adapter, url, ref, subpath, path, priority, trusted, enabled, capability, last_synced_at, last_commit, last_probe_ok";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        adapter: r.get(3)?,
        url: r.get(4)?,
        ref_name: r.get(5)?,
        subpath: r.get(6)?,
        path: r.get(7)?,
        priority: r.get(8)?,
        trusted: r.get::<_, i64>(9)? != 0,
        enabled: r.get::<_, i64>(10)? != 0,
        capability: r.get(11)?,
        last_synced_at: r.get(12)?,
        last_commit: r.get(13)?,
        last_probe_ok: r.get(14)?,
    })
}

pub fn upsert(tx: &WriteTx<'_>, row: &SourceRow) -> Result<()> {
    tx.conn()
        .execute(
            "INSERT INTO source (id, name, kind, adapter, url, ref, subpath, path, priority, trusted,
                                 enabled, capability, last_synced_at, last_commit, last_probe_ok)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, kind = excluded.kind, adapter = excluded.adapter,
                url = excluded.url, ref = excluded.ref, subpath = excluded.subpath,
                path = excluded.path, priority = excluded.priority, trusted = excluded.trusted,
                enabled = excluded.enabled, capability = excluded.capability",
            params![
                row.id,
                row.name,
                row.kind,
                row.adapter,
                row.url,
                row.ref_name,
                row.subpath,
                row.path,
                row.priority,
                row.trusted as i64,
                row.enabled as i64,
                row.capability,
                row.last_synced_at,
                row.last_commit,
                row.last_probe_ok,
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn list(conn: &rusqlite::Connection) -> Result<Vec<SourceRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM source ORDER BY priority, id"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn get(conn: &rusqlite::Connection, id: &str) -> Result<Option<SourceRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM source WHERE id = ?1"),
        [id],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

pub fn set_enabled(tx: &WriteTx<'_>, id: &str, enabled: bool) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE source SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn set_probe(tx: &WriteTx<'_>, id: &str, ok: bool, at: &str) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE source SET last_probe_ok = ?2 WHERE id = ?1",
            params![
                id,
                if ok {
                    at.to_owned()
                } else {
                    format!("failed:{at}")
                }
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use heron_core::ids::BUILTIN_MARKETS;

    fn builtin(id: &str, name: &str, adapter: &str, enabled: bool) -> SourceRow {
        SourceRow {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: "market".to_owned(),
            adapter: Some(adapter.to_owned()),
            url: None,
            ref_name: None,
            subpath: None,
            path: None,
            priority: 0,
            trusted: true,
            enabled,
            capability: Some("indexed".to_owned()),
            last_synced_at: None,
            last_commit: None,
            last_probe_ok: None,
        }
    }

    #[test]
    fn builtin_markets_roundtrip() {
        let mut db = Db::open_in_memory().unwrap();
        db.write(|tx| {
            for (index, (id, name, enabled)) in BUILTIN_MARKETS.iter().enumerate() {
                let mut row = builtin(id, name, id, *enabled);
                row.priority = index as i64;
                upsert(tx, &row)?;
            }
            Ok(())
        })
        .unwrap();

        let rows = list(db.conn()).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].id, "skills_sh");
        let disabled: Vec<_> = rows
            .iter()
            .filter(|r| !r.enabled)
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            disabled,
            vec!["skillsmp"],
            "decision D1: only SkillsMP off by default"
        );
        assert_eq!(
            get(db.conn(), "clawhub")
                .unwrap()
                .unwrap()
                .adapter
                .as_deref(),
            Some("clawhub")
        );
        assert!(get(db.conn(), "nope").unwrap().is_none());
    }

    #[test]
    fn upsert_updates_instead_of_duplicating() {
        let mut db = Db::open_in_memory().unwrap();
        db.write(|tx| upsert(tx, &builtin("a", "A", "a", true)))
            .unwrap();
        let mut updated = builtin("a", "A renamed", "a", false);
        updated.priority = 42;
        db.write(|tx| upsert(tx, &updated)).unwrap();
        let rows = list(db.conn()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "A renamed");
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].priority, 42);
    }
}
