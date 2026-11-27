//! Built-in market catalogue state and search/plan entry points shared by CLI and GUI (ADR-0010).

use heron_core::ErrorCode;
use heron_source::market::{InstallPlan, MarketItem};
use heron_store::models::SourceRow;
use heron_store::repos::source;

use crate::{Ctx, HeronError, Result};

/// View of one built-in market combining the static catalogue with the persisted enable toggle.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarketInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub capability: heron_source::Capability,
    pub enabled_by_default: bool,
    pub enabled: bool,
    pub base_url: &'static str,
}

fn market_row(id: &str, enabled: bool) -> Option<SourceRow> {
    let market = heron_source::MARKETS.iter().find(|m| m.id == id)?;
    Some(SourceRow {
        id: market.id.to_owned(),
        name: market.name.to_owned(),
        kind: "market".to_owned(),
        adapter: Some(market.id.to_owned()),
        url: Some(market.base_url.to_owned()),
        ref_name: None,
        subpath: None,
        path: None,
        priority: 0,
        trusted: true,
        enabled,
        capability: Some(
            match market.capability {
                heron_source::Capability::Indexed => "indexed",
                heron_source::Capability::Guided => "guided",
            }
            .to_owned(),
        ),
        last_synced_at: None,
        last_commit: None,
        last_probe_ok: None,
    })
}

/// Seed the `source` table from the built-in markets on first run so toggles have rows to update.
/// Existing rows preserve the user's persisted choice.
pub fn bootstrap(ctx: &mut Ctx) {
    for market in heron_source::MARKETS {
        let exists = source::get(ctx.db.conn(), market.id)
            .ok()
            .flatten()
            .is_some();
        if exists {
            continue;
        }
        if let Some(row) = market_row(market.id, market.enabled_by_default) {
            let _ = ctx.db.write(|tx| source::upsert(tx, &row));
        }
    }
}

/// List built-in markets with their persisted enabled state.
pub fn list(ctx: &Ctx) -> Vec<MarketInfo> {
    heron_source::MARKETS
        .iter()
        .map(|m| {
            let enabled = source::get(ctx.db.conn(), m.id)
                .ok()
                .flatten()
                .map(|row| row.enabled)
                .unwrap_or(m.enabled_by_default);
            MarketInfo {
                id: m.id,
                name: m.name,
                capability: m.capability,
                enabled_by_default: m.enabled_by_default,
                enabled,
                base_url: m.base_url,
            }
        })
        .collect()
}

/// Persist the enabled toggle for a built-in market.
pub fn set_enabled(ctx: &mut Ctx, market: &str, enabled: bool) -> Result<()> {
    let Some(row) = market_row(market, enabled) else {
        return Err(HeronError::from(
            ErrorCode::MarketUnavailable
                .msg()
                .with("market", market.to_owned()),
        ));
    };
    ctx.db.write(|tx| source::upsert(tx, &row))?;
    Ok(())
}

/// Search one market adapter. Network call; callers on UI threads should run it off-thread.
pub fn search(market: &str, query: &str, limit: usize) -> Result<Vec<MarketItem>> {
    let adapter = heron_source::adapter(market)?;
    adapter.search(query, limit)
}

/// Resolve an install plan from a market item key. Network call.
pub fn install_plan(market: &str, key: &str) -> Result<InstallPlan> {
    let adapter = heron_source::adapter(market)?;
    adapter.install_plan(key)
}
