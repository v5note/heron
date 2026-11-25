use heron_core::{ErrorCode, Result};
use rusqlite::{params, OptionalExtension};

use crate::db::{now_iso, WriteTx};
use crate::models::SkillRow;
use crate::repos::{json_list, parse_list};
use crate::sql_err;

const COLUMNS: &str =
    "slug, display_name, description, tags, status, origin, storage_path, subpath, created_at, updated_at";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<SkillRow> {
    Ok(SkillRow {
        slug: r.get(0)?,
        display_name: r.get(1)?,
        description: r.get(2)?,
        tags: parse_list(Some(r.get(3)?)),
        status: r.get(4)?,
        origin: r.get(5)?,
        storage_path: r.get(6)?,
        subpath: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn insert(tx: &WriteTx<'_>, row: &SkillRow) -> Result<()> {
    let now = now_iso();
    tx.conn()
        .execute(
            "INSERT INTO skill (slug, display_name, description, tags, status, origin, storage_path, subpath, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.slug,
                row.display_name,
                row.description,
                json_list(&row.tags),
                row.status,
                row.origin,
                row.storage_path,
                row.subpath,
                if row.created_at.is_empty() { &now } else { &row.created_at },
                if row.updated_at.is_empty() { &now } else { &row.updated_at },
            ],
        )
        .map_err(sql_err)?;
    Ok(())
}

/// Point a skill at its authoritative directory. `None` restores the store default.
pub fn set_storage_path(tx: &WriteTx<'_>, slug: &str, path: Option<&str>) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE skill SET storage_path = ?2, updated_at = ?3 WHERE slug = ?1",
            params![slug, path, now_iso()],
        )
        .map_err(sql_err)?;
    Ok(())
}

/// Fill in a description an older adopt left blank. The `description = ''` guard is what makes
/// this safe to run repeatedly: a value the library already owns is never overwritten.
pub fn fill_description(tx: &WriteTx<'_>, slug: &str, description: &str) -> Result<bool> {
    let changed = tx
        .conn()
        .execute(
            "UPDATE skill SET description = ?2, updated_at = ?3 WHERE slug = ?1 AND description = ''",
            params![slug, description, now_iso()],
        )
        .map_err(sql_err)?;
    Ok(changed > 0)
}

pub fn get(conn: &rusqlite::Connection, slug: &str) -> Result<Option<SkillRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM skill WHERE slug = ?1"),
        [slug],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

/// Case-insensitive lookup, matching the uniqueness rule of `Slug`.
pub fn get_ci(conn: &rusqlite::Connection, slug: &str) -> Result<Option<SkillRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM skill WHERE lower(slug) = lower(?1)"),
        [slug.trim()],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

pub fn list(conn: &rusqlite::Connection, include_trashed: bool) -> Result<Vec<SkillRow>> {
    let sql = if include_trashed {
        format!("SELECT {COLUMNS} FROM skill ORDER BY slug")
    } else {
        format!("SELECT {COLUMNS} FROM skill WHERE status = 'active' ORDER BY slug")
    };
    let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

/// Drop the index row outright. `version` and `source_binding` cascade with it, which is what
/// frees the slug for a fresh install. `projection` deliberately has **no** cascade: the caller
/// must take the client links back first, so deleting can never leave a dangling pointer behind.
pub fn delete(tx: &WriteTx<'_>, slug: &str) -> Result<bool> {
    let changed = tx
        .conn()
        .execute("DELETE FROM skill WHERE slug = ?1", params![slug])
        .map_err(sql_err)?;
    Ok(changed > 0)
}

pub fn set_status(tx: &WriteTx<'_>, slug: &str, status: &str) -> Result<()> {
    let changed = tx
        .conn()
        .execute(
            "UPDATE skill SET status = ?2, updated_at = ?3 WHERE slug = ?1",
            params![slug, status, now_iso()],
        )
        .map_err(sql_err)?;
    if changed == 0 {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    Ok(())
}

pub fn touch(tx: &WriteTx<'_>, slug: &str) -> Result<()> {
    tx.conn()
        .execute(
            "UPDATE skill SET updated_at = ?2 WHERE slug = ?1",
            params![slug, now_iso()],
        )
        .map_err(sql_err)?;
    Ok(())
}
