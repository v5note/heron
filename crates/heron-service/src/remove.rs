//! Remove a managed skill: take the links back, move the store copy to the trash, drop the index
//! rows (design §7.4 use-case shape: plan → confirm → execute + audit event).
//!
//! Deleting is *not* the same as unprojecting. `unproject` only retracts one client's link and
//! keeps the skill in the library; `remove` retires the library entry itself. Two rules make that
//! safe:
//!
//! * every link we placed is removed **before** the index row goes away, because `projection.slug`
//!   references `skill(slug)` without a cascade — a dangling pointer would either fail the delete
//!   or survive as a broken link nobody owns;
//! * the content is moved to `dirs.trash()` rather than deleted, and only when the store owns it.
//!   A live-bound skill (`storage_path` outside the store) belongs to the user's own directory and
//!   is never touched.

use std::path::PathBuf;

use heron_core::ErrorCode;
use heron_store::models::SkillRow;
use heron_store::repos::{event, projection, skill};

use crate::project;
use crate::{Ctx, HeronError, Result};

/// What `remove` is about to do, computed without touching the disk.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovePlan {
    pub slug: String,
    pub display_name: String,
    /// Where the authoritative content lives today.
    pub content: String,
    /// `false` when the skill only borrows an external directory (adopted in place / live-bound):
    /// nothing is moved, only the index rows and the links go away.
    pub owned: bool,
    /// Client paths that will lose their link.
    pub links: Vec<String>,
    /// Trash destination for the store copy, `None` when `owned` is `false`.
    pub trash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveOutcome {
    pub slug: String,
    pub links: Vec<String>,
    pub trash: Option<String>,
}

/// Resolve the row behind a possibly differently-cased slug.
fn find(ctx: &Ctx, slug: &str) -> Result<SkillRow> {
    let wanted = slug.trim();
    if let Some(row) = skill::get(ctx.db.conn(), wanted)? {
        return Ok(row);
    }
    skill::get_ci(ctx.db.conn(), wanted)?.ok_or_else(|| {
        ErrorCode::NotManaged
            .msg()
            .with("slug", wanted.to_owned())
            .into()
    })
}

/// Build the removal plan for one skill. Pure: reads the index and the filesystem layout only.
pub fn plan(ctx: &Ctx, slug: &str) -> Result<RemovePlan> {
    let row = find(ctx, slug)?;
    let content = ctx.content_path(&row);
    let owned = content.starts_with(ctx.store_root());
    let links = projection::list_for_skill(ctx.db.conn(), &row.slug)?
        .into_iter()
        .map(|p| p.path)
        .collect();
    Ok(RemovePlan {
        slug: row.slug.clone(),
        display_name: row.display_name.clone(),
        content: content.to_string_lossy().to_string(),
        owned,
        links,
        trash: owned.then(|| trash_path(ctx, &row.slug).to_string_lossy().to_string()),
    })
}

/// `~/.heron/trash/<slug>-<timestamp>`. The timestamp keeps a reinstalled-and-redeleted slug from
/// colliding with an entry that is still waiting to be restored.
fn trash_path(ctx: &Ctx, slug: &str) -> PathBuf {
    let stamp = heron_platform::time::now_rfc3339().replace([':', '.', '+'], "-");
    ctx.dirs.trash().join(format!("{slug}-{stamp}"))
}

/// Execute the plan: retract the links, move the content aside, forget the index rows.
pub fn apply(ctx: &mut Ctx, plan: &RemovePlan) -> Result<RemoveOutcome> {
    let event_id = ctx.db.write(|tx| {
        event::record(
            tx,
            ctx.actor,
            "remove",
            Some(&plan.slug),
            plan.trash.as_deref(),
            false,
        )
    })?;

    let links = project::unproject_all(ctx, &plan.slug)?;
    let moved = if plan.owned {
        move_to_trash(ctx, plan)?
    } else {
        None
    };

    ctx.db.write(|tx| {
        // `source_binding` and `version` cascade, which is what frees the slug for a reinstall.
        skill::delete(tx, &plan.slug)?;
        event::finish(tx, event_id, false)?;
        Ok(())
    })?;

    Ok(RemoveOutcome {
        slug: plan.slug.clone(),
        links,
        trash: moved.map(|p| p.to_string_lossy().to_string()),
    })
}

fn move_to_trash(ctx: &Ctx, plan: &RemovePlan) -> Result<Option<PathBuf>> {
    let from = PathBuf::from(&plan.content);
    let Some(dest) = plan.trash.as_deref() else {
        return Ok(None);
    };
    if !from.exists() {
        // Index-only entry: the directory is already gone, nothing to park in the trash.
        return Ok(None);
    }
    let dest = PathBuf::from(dest);
    let parent = dest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ctx.dirs.trash());
    std::fs::create_dir_all(&parent).map_err(|e| {
        HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", parent.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    std::fs::rename(&from, &dest).map_err(|e| {
        HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", from.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    Ok(Some(dest))
}
