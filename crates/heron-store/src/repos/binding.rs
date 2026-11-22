//! `source_binding`: where a skill came from.
//!
//! Adoption records provenance here (including the upstream a third-party installer left behind in
//! its own lockfile). It is deliberately *only* an index: the baseline a three-way merge needs lives
//! in the skill's `heron.skill.json`, so a binding never makes `upgrade` believe it can merge
//! against a revision Heron has never fetched.

use heron_core::Result;
use rusqlite::params;

use crate::db::WriteTx;
use crate::models::SourceBindingRow;
use crate::sql_err;

const COLUMNS: &str =
    "slug, kind, url, ref, subpath, pinned_commit, remote_version, content_hash, is_primary";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceBindingRow> {
    Ok(SourceBindingRow {
        slug: r.get(0)?,
        kind: r.get(1)?,
        url: r.get(2)?,
        ref_name: r.get(3)?,
        subpath: r.get(4)?,
        pinned_commit: r.get(5)?,
        remote_version: r.get(6)?,
        content_hash: r.get(7)?,
        is_primary: r.get::<_, i64>(8)? != 0,
    })
}

/// Replace the skill's primary binding. A `git` binding carries a fetchable `url`; a `local` one
/// only records that the content arrived from outside the store.
pub fn replace_primary(tx: &WriteTx<'_>, row: &SourceBindingRow) -> Result<()> {
    tx.conn()
        .execute(
            "DELETE FROM source_binding WHERE slug = ?1 AND is_primary = 1",
            params![row.slug],
        )
        .map_err(sql_err)?;
    tx.conn()
        .execute(
            "INSERT INTO source_binding (slug, kind, url, ref, subpath, pinned_commit,
                                         remote_version, content_hash, is_primary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            params![
                row.slug,
                row.kind,
                row.url,
                row.ref_name,
                row.subpath,
                row.pinned_commit,
                row.remote_version,
                row.content_hash,
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

pub fn get(conn: &rusqlite::Connection, slug: &str) -> Result<Vec<SourceBindingRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM source_binding WHERE slug = ?1 ORDER BY is_primary DESC, id"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([slug], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}
