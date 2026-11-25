//! Project / unproject: materialize a store skill into a target (design §7.1).

use std::path::PathBuf;

use heron_core::ProjectionMode;
use heron_projection::plan::{plan_projection, ProjectPlan, ProjectionRequest};
use heron_store::models::{ProjectionRow, TargetRow};
use heron_store::repos::{event, projection, skill, target};

use crate::{Ctx, Result};

#[derive(Debug, Clone)]
pub struct ProjectRequest {
    pub slug: String,
    pub target_id: String,
    /// `None` uses the target's/global default.
    pub mode: Option<ProjectionMode>,
    /// Name of the directory inside the target (defaults to the slug).
    pub link_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectOutcome {
    pub slug: String,
    pub target_id: String,
    pub path: String,
    pub mode: String,
    pub applied: bool,
}

/// Build the plan for one projection request.
pub fn plan(ctx: &Ctx, req: &ProjectRequest) -> Result<(ProjectPlan, ProjectionRequest)> {
    let target = load_target(ctx, &req.target_id)?;
    // Whole-repository skills project their entry directory, never the checkout root: the client
    // scanner wants `SKILL.md` at the top of what it sees.
    let content = ctx.content_path_of(&req.slug);
    let source = ctx.projectable_path_of(&req.slug);
    let link_name = req.link_name.clone().unwrap_or_else(|| req.slug.clone());
    let dest = PathBuf::from(&target.path).join(&link_name);
    let mut mode = req.mode.unwrap_or_else(default_mode);
    // Copying only the inner entry would lift the skill out of its checkout again — exactly the
    // incomplete copy the whole-repo layout exists to prevent — so nested skills link instead.
    if source != content && matches!(mode, ProjectionMode::Copy) {
        mode = ProjectionMode::resolve_auto(heron_core::PlatformKind::current());
    }

    let existing = projection::list_for_skill(ctx.db.conn(), &req.slug)?
        .into_iter()
        .find(|p| p.target_id == target.id && p.link_name == link_name);

    let recorded_hash = existing
        .as_ref()
        .and_then(|p| p.content_hash.as_deref())
        .map(heron_core::Hash::from_hex);

    let request = ProjectionRequest {
        source,
        dest,
        mode,
        source_hash: None,
        recorded_hash,
        already_managed: existing.is_some(),
    };
    let plan = plan_projection(&request)?;
    Ok((plan, request))
}

/// Execute a projection. Refuses when the plan is blocked (user data in the way).
pub fn apply(
    ctx: &mut Ctx,
    req: &ProjectRequest,
    plan: &ProjectPlan,
    request: &ProjectionRequest,
) -> Result<ProjectOutcome> {
    let event_id = ctx.db.write(|tx| {
        event::record(
            tx,
            ctx.actor,
            "project",
            Some(&req.slug),
            Some(&req.target_id),
            true,
        )
    })?;

    let applied = heron_projection::apply::apply_plan(plan)?;
    let Some(last) = applied.last() else {
        // No-op: already present.
        ctx.db.write(|tx| event::finish(tx, event_id, false))?;
        return Ok(ProjectOutcome {
            slug: req.slug.clone(),
            target_id: req.target_id.clone(),
            path: request.dest.to_string_lossy().to_string(),
            mode: mode_str(request.mode).to_owned(),
            applied: false,
        });
    };

    let existing = projection::list_for_skill(ctx.db.conn(), &req.slug)?
        .into_iter()
        .find(|p| p.target_id == req.target_id && p.path == last.path.to_string_lossy());
    ctx.db.write(|tx| {
        match existing {
            Some(row) => {
                tx.conn()
                    .execute(
                        "UPDATE projection SET mode = ?2, inode_kind = ?3, state = 'ok' WHERE id = ?1",
                        rusqlite::params![row.id, mode_str(last.mode), last.inode_kind],
                    )
                    .map_err(heron_store::sql_err)?;
            }
            None => {
                projection::insert(
                    tx,
                    &ProjectionRow {
                        slug: req.slug.clone(),
                        target_id: req.target_id.clone(),
                        link_name: last
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path: last.path.to_string_lossy().to_string(),
                        mode: mode_str(last.mode).to_owned(),
                        version: None,
                        content_hash: last.content_hash.as_ref().map(|h| h.to_string()),
                        inode_kind: Some(last.inode_kind.clone()),
                        ..Default::default()
                    },
                )?;
            }
        }
        event::finish(tx, event_id, false)?;
        Ok(())
    })?;

    Ok(ProjectOutcome {
        slug: req.slug.clone(),
        target_id: req.target_id.clone(),
        path: last.path.to_string_lossy().to_string(),
        mode: mode_str(last.mode).to_owned(),
        applied: true,
    })
}

pub fn unproject(ctx: &mut Ctx, slug: &str, target_id: &str) -> Result<()> {
    let rows = projection::list_for_skill(ctx.db.conn(), slug)?;
    let mut removed = 0;
    for row in rows.into_iter().filter(|p| p.target_id == target_id) {
        remove_link(ctx, &row)?;
        removed += 1;
    }
    if removed == 0 {
        return Err(heron_core::ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    Ok(())
}

/// Take back **every** link a skill owns, across all targets. Used by `remove`: a skill cannot be
/// deleted while pointers to it still sit in a client's skills directory. Unlike `unproject` an
/// empty result is fine — the skill may simply never have been projected.
pub fn unproject_all(ctx: &mut Ctx, slug: &str) -> Result<Vec<String>> {
    let rows = projection::list_for_skill(ctx.db.conn(), slug)?;
    let mut removed = Vec::with_capacity(rows.len());
    for row in &rows {
        remove_link(ctx, row)?;
        removed.push(row.path.clone());
    }
    Ok(removed)
}

fn remove_link(ctx: &mut Ctx, row: &ProjectionRow) -> Result<()> {
    let mode = mode_of(&row.mode);
    let hash = row.content_hash.as_deref().map(heron_core::Hash::from_hex);
    heron_projection::apply::remove_projection(&PathBuf::from(&row.path), mode, hash.as_ref())?;
    ctx.db.write(|tx| projection::delete(tx, row.id))?;
    Ok(())
}

fn mode_of(raw: &str) -> ProjectionMode {
    match raw {
        "copy" => ProjectionMode::Copy,
        "pointer" => ProjectionMode::Pointer,
        "junction" => ProjectionMode::Junction,
        _ => ProjectionMode::Symlink,
    }
}

fn load_target(ctx: &Ctx, target_id: &str) -> Result<TargetRow> {
    target::get(ctx.db.conn(), target_id)?.ok_or_else(|| {
        heron_core::ErrorCode::TargetNotFound
            .msg()
            .with("target", target_id.to_owned())
            .into()
    })
}

fn default_mode() -> ProjectionMode {
    ProjectionMode::resolve_auto(heron_core::PlatformKind::current())
}

fn mode_str(mode: ProjectionMode) -> &'static str {
    match mode {
        ProjectionMode::Copy => "copy",
        ProjectionMode::Symlink => "symlink",
        ProjectionMode::Junction => "junction",
        ProjectionMode::Pointer => "pointer",
    }
}

/// Every target known to the index, for listing in the CLI.
pub fn list_targets(ctx: &Ctx) -> Result<Vec<TargetRow>> {
    target::list(ctx.db.conn())
}

/// Reuse the skill repo helper so `project` and `adopt` agree on slugs.
pub fn managed_slug(ctx: &Ctx, raw: &str) -> Result<String> {
    let existing = skill::get_ci(ctx.db.conn(), raw)?;
    Ok(existing.map(|s| s.slug).unwrap_or_else(|| raw.to_owned()))
}
