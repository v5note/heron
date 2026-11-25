//! Favourited open-source skills (「来源与市场 → 我的收藏」).
//!
//! A favourite is just a saved (name, url, description) triple: the url is what feeds the
//! installer, so the row itself holds no content. `url` is unique — re-saving updates in place.

use heron_core::Result;
use rusqlite::params;

use crate::db::{now_iso, WriteTx};
use crate::models::FavoriteRow;
use crate::sql_err;

const COLUMNS: &str = "id, name, url, description, market, created_at";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<FavoriteRow> {
    Ok(FavoriteRow {
        id: r.get(0)?,
        name: r.get(1)?,
        url: r.get(2)?,
        description: r.get(3)?,
        market: r.get(4)?,
        created_at: r.get(5)?,
    })
}

/// Insert (or update, keyed on `url`) and return the stored row.
pub fn save(tx: &WriteTx<'_>, fav: &FavoriteRow) -> Result<FavoriteRow> {
    tx.conn()
        .execute(
            "INSERT INTO favorite (name, url, description, market, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(url) DO UPDATE SET
                name = excluded.name, description = excluded.description,
                market = excluded.market",
            params![fav.name, fav.url, fav.description, fav.market, now_iso()],
        )
        .map_err(sql_err)?;
    let created = get(tx.conn(), &fav.url)?.ok_or_else(|| {
        crate::db::db_msg("favorite.save", rusqlite::Error::ExecuteReturnedResults)
    })?;
    Ok(created)
}

pub fn get(conn: &rusqlite::Connection, url: &str) -> Result<Option<FavoriteRow>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM favorite WHERE url = ?1"),
        [url],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

/// Newest first.
pub fn list(conn: &rusqlite::Connection) -> Result<Vec<FavoriteRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM favorite ORDER BY created_at DESC, id DESC"
        ))
        .map_err(sql_err)?;
    let rows = stmt.query_map([], row_from).map_err(sql_err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_err)
}

pub fn remove(tx: &WriteTx<'_>, id: i64) -> Result<()> {
    tx.conn()
        .execute("DELETE FROM favorite WHERE id = ?1", params![id])
        .map_err(sql_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;

    fn fav(name: &str, url: &str) -> FavoriteRow {
        FavoriteRow {
            id: 0,
            name: name.to_owned(),
            url: url.to_owned(),
            description: "d".to_owned(),
            market: Some("github".to_owned()),
            created_at: String::new(),
        }
    }

    #[test]
    fn save_then_list_then_remove() {
        let mut db = Db::open_in_memory().unwrap();
        let saved = db
            .write(|tx| save(tx, &fav("WeChat MP", "https://github.com/o/wechat-mp")))
            .unwrap();
        assert!(saved.id > 0);
        assert_eq!(list(db.conn()).unwrap().len(), 1);

        // Same url updates in place rather than duplicating.
        db.write(|tx| {
            save(
                tx,
                &fav("WeChat MP renamed", "https://github.com/o/wechat-mp"),
            )
        })
        .unwrap();
        let rows = list(db.conn()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "WeChat MP renamed");

        let id = rows[0].id;
        db.write(|tx| remove(tx, id)).unwrap();
        assert!(list(db.conn()).unwrap().is_empty());
    }
}
