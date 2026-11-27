//! Saved market/remote favourites, owned by the service layer (ADR-0010).

use heron_core::ErrorCode;
use heron_store::models::FavoriteRow;
use heron_store::repos::favorite;

use crate::{Ctx, HeronError, Result};

pub fn list(ctx: &Ctx) -> Result<Vec<FavoriteRow>> {
    favorite::list(ctx.db.conn())
}

pub fn add(
    ctx: &mut Ctx,
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
    let row = FavoriteRow {
        id: 0,
        name,
        url,
        description: description.trim().to_owned(),
        market: market
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty()),
        created_at: String::new(),
    };
    ctx.db.write(|tx| favorite::save(tx, &row))
}

pub fn remove(ctx: &mut Ctx, id: i64) -> Result<()> {
    ctx.db.write(|tx| favorite::remove(tx, id))?;
    Ok(())
}
