//! Saved market/remote favourites, owned by the service layer (ADR-0010).

use heron_core::ErrorCode;
use heron_store::models::FavoriteRow;
use heron_store::repos::favorite;

use crate::{Ctx, HeronError, Result};

pub fn list(ctx: &Ctx) -> Result<Vec<FavoriteRow>> {
    favorite::list(ctx.db.conn())
}

fn build_row(
    id: i64,
    name: &str,
    url: &str,
    description: &str,
    market: Option<&str>,
) -> Result<FavoriteRow> {
    let name = name.trim().to_owned();
    let url = url.trim().to_owned();
    if name.is_empty() {
        return Err(HeronError::from(
            ErrorCode::Internal
                .msg()
                .with("detail", "favorite_name_required"),
        ));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(HeronError::from(
            ErrorCode::Internal
                .msg()
                .with("detail", "favorite_url_required"),
        ));
    }
    Ok(FavoriteRow {
        id,
        name,
        url,
        description: description.trim().to_owned(),
        market: market
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty()),
        created_at: String::new(),
    })
}

pub fn add(
    ctx: &mut Ctx,
    name: &str,
    url: &str,
    description: &str,
    market: Option<&str>,
) -> Result<FavoriteRow> {
    let row = build_row(0, name, url, description, market)?;
    ctx.db.write(|tx| favorite::save(tx, &row))
}

pub fn update(
    ctx: &mut Ctx,
    id: i64,
    name: &str,
    url: &str,
    description: &str,
    market: Option<&str>,
) -> Result<FavoriteRow> {
    let row = build_row(id, name, url, description, market)?;
    ctx.db.write(|tx| favorite::update(tx, &row))
}

pub fn remove(ctx: &mut Ctx, id: i64) -> Result<()> {
    ctx.db.write(|tx| favorite::remove(tx, id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron_platform::dirs::Dirs;
    use heron_store::Db;
    use tempfile::TempDir;

    fn ctx() -> (TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs =
            Dirs::resolve_with_override("heron-test", Some(&tmp.path().join("data"))).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        (tmp, Ctx::new(dirs, db, "test"))
    }

    #[test]
    fn update_trims_and_persists() {
        let (_tmp, mut ctx) = ctx();
        let created = add(
            &mut ctx,
            "WeChat MP",
            "https://github.com/o/wechat-mp",
            "initial",
            Some("github"),
        )
        .unwrap();

        let updated = update(
            &mut ctx,
            created.id,
            "  WeChat MP v2  ",
            "  https://gitee.com/o/wechat-mp  ",
            "  updated desc  ",
            Some("  "),
        )
        .unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "WeChat MP v2");
        assert_eq!(updated.url, "https://gitee.com/o/wechat-mp");
        assert_eq!(updated.description, "updated desc");
        assert_eq!(updated.market, None);
        assert_eq!(updated.created_at, created.created_at);

        let rows = list(&ctx).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], updated);
    }

    #[test]
    fn update_rejects_invalid_fields_duplicates_and_missing_id() {
        let (_tmp, mut ctx) = ctx();
        let first = add(
            &mut ctx,
            "First",
            "https://github.com/o/first",
            "",
            None,
        )
        .unwrap();
        let second = add(
            &mut ctx,
            "Second",
            "https://github.com/o/second",
            "",
            None,
        )
        .unwrap();

        let empty_name = update(
            &mut ctx,
            first.id,
            "   ",
            "https://github.com/o/first",
            "",
            None,
        )
        .unwrap_err();
        assert_eq!(
            empty_name.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_name_required")
        );

        let bad_url = update(&mut ctx, first.id, "First", "ftp://example.com", "", None)
            .unwrap_err();
        assert_eq!(
            bad_url.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_url_required")
        );

        let dup = update(
            &mut ctx,
            second.id,
            "Second",
            "https://github.com/o/first",
            "",
            None,
        )
        .unwrap_err();
        assert_eq!(
            dup.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_url_exists")
        );

        let missing = update(
            &mut ctx,
            999_999,
            "Ghost",
            "https://github.com/o/ghost",
            "",
            None,
        )
        .unwrap_err();
        assert_eq!(
            missing.as_msg().params.get("detail").map(String::as_str),
            Some("favorite_not_found")
        );
    }
}
