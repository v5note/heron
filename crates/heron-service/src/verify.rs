//! Verify: would each projection actually be loaded by its client? (design §10.11.1)

use std::path::PathBuf;

use heron_projection::verify::{classify, VerifyInput, Visibility};
use heron_store::repos::{client, projection, target};

use crate::clients::BUILTIN;
use crate::{Ctx, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub slug: String,
    pub target_id: String,
    pub path: String,
    pub mode: String,
    pub visibility: Visibility,
}

/// Verify every projection (or only those of `slug`).
pub fn verify(ctx: &Ctx, slug: Option<&str>) -> Result<Vec<VerifyReport>> {
    let rows = match slug {
        Some(slug) => projection::list_for_skill(ctx.db.conn(), slug)?,
        None => projection::list_all(ctx.db.conn())?,
    };
    let targets = target::list(ctx.db.conn())?;
    let clients = client::list(ctx.db.conn())?;

    let mut reports = Vec::new();
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

        let visibility = classify(&VerifyInput {
            path: path.clone(),
            scan_paths,
            occupied_names: occupied,
            resolves,
        })?;

        reports.push(VerifyReport {
            slug: row.slug.clone(),
            target_id: row.target_id.clone(),
            path: row.path.clone(),
            mode: row.mode.clone(),
            visibility,
        });
    }
    Ok(reports)
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
    id == "heron" || BUILTIN.iter().any(|c| c.id == id)
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
