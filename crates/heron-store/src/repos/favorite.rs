//! Favourited open-source skills (「来源与市场 → 我的收藏」).
//!
//! A favourite is just a saved (name, url, description) triple: the url is what feeds the
//! installer, so the row itself holds no content. `url` is unique — re-saving updates in place.

use heron_core::{ErrorCode, HeronError, Result};
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

pub fn get_by_id(conn: &rusqlite::Connection, id: i64) -> Result<Option<FavoriteRow>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM favorite WHERE id = ?1"),
        params![id],
        row_from,
    )
    .optional()
    .map_err(sql_err)
}

/// Update an existing favourite in place by `id`, preserving `created_at`.
pub fn update(tx: &WriteTx<'_>, fav: &FavoriteRow) -> Result<FavoriteRow> {
    if let Some(existing) = get(tx.conn(), &fav.url)? {
        if existing.id != fav.id {
            return Err(HeronError::from(
                ErrorCode::Internal
                    .msg()
                    .with("detail", "favorite_url_exists"),
            ));
        }
    }
    let changed = tx
        .conn()
        .execute(
            "UPDATE favorite
             SET name = ?2, url = ?3, description = ?4, market = ?5
             WHERE id = ?1",
            params![fav.id, fav.name, fav.url, fav.description, fav.market],
        )
        .map_err(sql_err)?;
    if changed == 0 {
        return Err(HeronError::from(
            ErrorCode::Internal
                .msg()
                .with("detail", "favorite_not_found"),
        ));
    }
    get_by_id(tx.conn(), fav.id)?
        .ok_or_else(|| crate::db::db_msg("favorite.update", rusqlite::Error::ExecuteReturnedResults))
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

    #[test]
    fn update_modifies_in_place_and_validates_conflicts() {
        let mut db = Db::open_in_memory().unwrap();
        let first = db
            .write(|tx| save(tx, &fav("Alpha", "https://github.com/o/alpha")))
            .unwrap();
        let second = db
            .write(|tx| save(tx, &fav("Beta", "https://github.com/o/beta")))
            .unwrap();

        let edited = FavoriteRow {
            id: first.id,
            name: "Alpha edited".to_owned(),
            url: "https://gitee.com/o/alpha-v2".to_owned(),
            description: "new desc".to_owned(),
            market: Some("gitee".to_owned()),
            created_at: "ignored".to_owned(),
        };
        let updated = db.write(|tx| update(tx, &edited)).unwrap();
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.created_at, first.created_at);
        assert_eq!(updated.name, "Alpha edited");
        assert_eq!(updated.url, "https://gitee.com/o/alpha-v2");
        assert_eq!(updated.description, "new desc");
        assert_eq!(updated.market.as_deref(), Some("gitee"));
        assert_eq!(list(db.conn()).unwrap().len(), 2);

        // Updating to another row's URL returns favorite_url_exists without modifying either row.
        let conflict = FavoriteRow {
            id: first.id,
            name: "Conflict".to_owned(),
            url: second.url.clone(),
            description: "x".to_owned(),
            market: None,
            created_at: String::new(),
        };
        let err = db.write(|tx| update(tx, &conflict)).unwrap_err();
        assert_eq!(err.code(), "E_INTERNAL");
        assert_eq!(
            err.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_url_exists")
        );
        assert_eq!(get_by_id(db.conn(), first.id).unwrap(), Some(updated));
        assert_eq!(get_by_id(db.conn(), second.id).unwrap(), Some(second));

        // Updating a nonexistent id returns favorite_not_found.
        let missing = FavoriteRow {
            id: 999_999,
            ..fav("Missing", "https://github.com/o/missing")
        };
        let err = db.write(|tx| update(tx, &missing)).unwrap_err();
        assert_eq!(err.code(), "E_INTERNAL");
        assert_eq!(
            err.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_not_found")
        );
    }
}
