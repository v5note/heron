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

/// One parked directory sitting in `~/.heron/trash`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    /// Directory name inside `dirs.trash()`, e.g. `wechat-mp-2026-11-27T07-00-00-000Z`.
    pub id: String,
    /// Slug recovered from `SKILL.md` frontmatter or the `<slug>-<timestamp>` prefix.
    pub slug: String,
    /// Full path on disk.
    pub path: String,
}

/// List every skill directory parked in `dirs.trash()`.
pub fn list_trash(ctx: &Ctx) -> Result<Vec<TrashEntry>> {
    let dir = ctx.dirs.trash();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let rd = std::fs::read_dir(&dir).map_err(|e| {
        HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", dir.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    for entry in rd {
        let entry = entry.map_err(|e| {
            HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", dir.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let slug = infer_trashed_slug(&path, &id);
        entries.push(TrashEntry {
            id,
            slug,
            path: path.to_string_lossy().to_string(),
        });
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}

/// Restore a trashed skill directory back into the store and re-register its index row.
pub fn restore(ctx: &mut Ctx, trash_id: &str) -> Result<SkillRow> {
    let trash_dir = ctx.dirs.trash().join(trash_id);
    if !trash_dir.is_dir() {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", trash_id.to_owned())
            .into());
    }
    let slug = infer_trashed_slug(&trash_dir, trash_id);
    if skill::get_ci(ctx.db.conn(), &slug)?.is_some() {
        return Err(ErrorCode::SlugConflict
            .msg()
            .with("slug", slug)
            .into());
    }
    let dest = ctx.skill_path(&slug);
    if dest.exists() {
        return Err(ErrorCode::SlugConflict
            .msg()
            .with("slug", slug)
            .into());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", parent.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
    }
    std::fs::rename(&trash_dir, &dest).map_err(|e| {
        HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", trash_dir.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;

    let (display_name, description) = read_meta(&dest, &slug);
    let row = SkillRow {
        slug: slug.clone(),
        display_name,
        description,
        tags: Vec::new(),
        status: "active".to_owned(),
        origin: "store".to_owned(),
        storage_path: None,
        subpath: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    ctx.db.write(|tx| {
        let ev = event::record(
            tx,
            ctx.actor,
            "restore",
            Some(&slug),
            Some(&trash_dir.to_string_lossy()),
            false,
        )?;
        skill::insert(tx, &row)?;
        event::finish(tx, ev, false)?;
        Ok(())
    })?;
    Ok(row)
}

/// Permanently remove every directory parked in `dirs.trash()`. Returns the count removed.
pub fn empty_trash(ctx: &mut Ctx) -> Result<usize> {
    let entries = list_trash(ctx)?;
    let mut removed = 0;
    for entry in &entries {
        heron_platform::link::remove_dir(std::path::Path::new(&entry.path))?;
        removed += 1;
    }
    if removed > 0 {
        ctx.db.write(|tx| {
            let ev = event::record(tx, ctx.actor, "trash_empty", None, None, false)?;
            event::finish(tx, ev, false)?;
            Ok(())
        })?;
    }
    Ok(removed)
}

fn infer_trashed_slug(dir: &std::path::Path, id: &str) -> String {
    if let Ok((parsed, _)) = heron_scan::parse_skill_dir(dir) {
        if let Some(name) = parsed.name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
    }
    // Strip the `-YYYY-MM-DDT...` suffix added by `trash_path`.
    if let Some(idx) = id.rfind("-20") {
        let candidate = &id[..idx];
        if !candidate.is_empty() {
            return candidate.to_owned();
        }
    }
    id.to_owned()
}

fn read_meta(dir: &std::path::Path, fallback_slug: &str) -> (String, String) {
    if let Ok((parsed, _)) = heron_scan::parse_skill_dir(dir) {
        let name = parsed
            .name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback_slug.to_owned());
        let desc = parsed
            .description
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        return (name, desc);
    }
    (fallback_slug.to_owned(), String::new())
}
