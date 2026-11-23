//! Verify: would each projection actually be loaded by its client? (design §10.11.1)

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use heron_projection::verify::{classify, VerifyInput, Visibility};
use heron_store::db::{iso_from_unix_millis, now_iso};
use heron_store::models::VisibilityRow;
use heron_store::repos::{client, projection, target, visibility};

use crate::{Ctx, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub slug: String,
    pub target_id: String,
    pub path: String,
    pub mode: String,
    pub visibility: Visibility,
}

/// Verify every projection (or only those of `slug`), and refresh the visibility cache.
pub fn verify(ctx: &mut Ctx, slug: Option<&str>) -> Result<Vec<VerifyReport>> {
    let rows = match slug {
        Some(slug) => projection::list_for_skill(ctx.db.conn(), slug)?,
        None => projection::list_all(ctx.db.conn())?,
    };
    let targets = target::list(ctx.db.conn())?;
    let clients = client::list(ctx.db.conn())?;
    let prev = visibility::list(ctx.db.conn())?;
    let session_rescan: Vec<String> = crate::clients::builtin()?
        .into_iter()
        .filter(|c| c.rescan.as_deref() == Some("session"))
        .map(|c| c.id)
        .collect();
    let checked_at = now_iso();

    let mut reports = Vec::new();
    let mut cache = Vec::new();
    for row in rows {
        let Some(target_row) = targets.iter().find(|t| t.id == row.target_id) else {
            continue;
        };
        // An adopted original keeps its own location (inside a client directory). Judge it by
        // that client's scan paths; everything else by its target's.
        let (kind, project) = if let Some((client, _)) = row.link_name.split_once('@') {
            (client.to_owned(), None)
        } else {
            (target_row.kind.clone(), target_row.project_path.clone())
        };
        let scan_paths = scan_paths_for(&kind, project.as_deref(), &clients, &targets);
        let occupied = occupied_names(&kind, project.as_deref());
        let path = PathBuf::from(&row.path);
        // A link resolves when its target exists; a plain copy resolves when the dir exists.
        let resolves = match heron_platform::link::probe(&path) {
            Ok(info) => {
                if info.kind.is_link() {
                    info.resolved_exists
                } else if info.kind.is_real_dir() {
                    true
                } else {
                    path.exists()
                }
            }
            Err(_) => path.exists(),
        };

        let rescans_on_session = session_rescan.iter().any(|id| id == &kind);
        // Restart accounting for session-rescan clients (design §12.3): a landing stays
        // "restart required" until some earlier verify has already confirmed it — the user has
        // then had a chance to reopen the client. A newer on-disk mtime (re-projection or an
        // in-place refresh under the same row) starts the clock over.
        let landing_unconfirmed = rescans_on_session
            && match prev.iter().find(|v| v.projection_id == row.id) {
                None => true,
                Some(v) => match landed_at(&path) {
                    Some(ms) => v.checked_at.as_str() < iso_from_unix_millis(ms).as_str(),
                    None => false,
                },
            };

        let visibility = classify(&VerifyInput {
            path,
            scan_paths,
            occupied_names: occupied,
            resolves,
            rescans_on_session,
            landing_unconfirmed,
        })?;

        // The table's CHECK has no `unknown`; classify never yields it, but the cache says so
        // explicitly instead of tripping the constraint.
        if !matches!(visibility, Visibility::Unknown) {
            cache.push(VisibilityRow {
                projection_id: row.id,
                client_id: kind,
                state: visibility.as_str().to_owned(),
                shadowed_by: match &visibility {
                    Visibility::Shadowed { by } => Some(by.clone()),
                    _ => None,
                },
                checked_at: checked_at.clone(),
            });
        }
        reports.push(VerifyReport {
            slug: row.slug.clone(),
            target_id: row.target_id.clone(),
            path: row.path.clone(),
            mode: row.mode.clone(),
            visibility,
        });
    }
    ctx.db.write(|tx| {
        for row in &cache {
            visibility::upsert(tx, row)?;
        }
        Ok(())
    })?;
    Ok(reports)
}

/// When the projection itself landed on disk (the link's own mtime, target untouched).
fn landed_at(path: &Path) -> Option<i64> {
    std::fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// Which directories a client scans, resolved for this projection's scope.
fn scan_paths_for(
    kind: &str,
    project: Option<&str>,
    clients: &[heron_store::models::ClientRow],
    targets: &[heron_store::models::TargetRow],
) -> Vec<PathBuf> {
    let _ = targets;
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(row) = clients.iter().find(|c| c.id == kind) {
        for p in &row.global_paths {
            paths.push(PathBuf::from(p));
        }
        if let Some(project) = project {
            for rel in &row.project_paths {
                paths.push(PathBuf::from(project).join(rel));
            }
        }
    }
    paths
}

/// Names already claimed by higher-priority sources (plugins) for that client.
fn occupied_names(_kind: &str, _project: Option<&str>) -> Vec<String> {
    // Plugin detection lands with the native-scan work (M3); until then nothing is shadowed.
    Vec::new()
}

/// Is this client known at all? Used by the CLI to warn about unknown target kinds.
pub fn known_client(id: &str) -> bool {
    id == "custom"
        || crate::clients::builtin()
            .map(|m| m.iter().any(|c| c.id == id))
            .unwrap_or(false)
}

/// Helper so the report can be rendered in one line.
pub fn describe(visibility: &Visibility) -> String {
    match visibility {
        Visibility::Visible => "visible".to_owned(),
        Visibility::NotScanned => "not scanned".to_owned(),
        Visibility::Shadowed { by } => format!("shadowed by {by}"),
        Visibility::RestartRequired => "restart required".to_owned(),
        Visibility::Broken => "broken".to_owned(),
        Visibility::Unknown => "unknown".to_owned(),
    }
}
