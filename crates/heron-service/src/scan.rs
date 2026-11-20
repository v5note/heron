//! Scan use case: run the scanner and persist what it found.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;

use heron_scan::classify::{Classifier, HitKind};
use heron_scan::walker::{scan, ScanOptions, ScanResult};
use heron_store::models::DetectedUpsert;
use heron_store::repos::{detected, target};

use crate::{clients, Ctx, Result};

#[derive(Debug, Clone, Default)]
pub struct ScanRequest {
    pub extra_roots: Vec<PathBuf>,
    pub max_dirs: Option<usize>,
    pub extra_excludes: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanReport {
    pub skills: usize,
    pub legacy_commands: usize,
    pub nested_suspects: usize,
    pub needs_attention: usize,
    pub dirs_visited: usize,
    pub duration_ms: u64,
    pub truncated: bool,
    pub roots_missing: Vec<String>,
}

impl ScanReport {
    pub fn from_result(result: &ScanResult) -> Self {
        Self {
            skills: result.count(HitKind::Skill),
            legacy_commands: result.count(HitKind::LegacyCommand),
            nested_suspects: result.count(HitKind::NestedSuspect),
            needs_attention: result.hits.iter().filter(|h| !h.frontmatter_ok()).count(),
            dirs_visited: result.stats.dirs_visited,
            duration_ms: result.stats.duration_ms,
            truncated: result.stats.truncated,
            roots_missing: result
                .stats
                .roots_missing
                .iter()
                .map(|p| heron_platform::paths::display(p))
                .collect(),
        }
    }
}

/// Run a scan and write the results into the index.
pub fn run(ctx: &mut Ctx, home: &std::path::Path, req: &ScanRequest) -> Result<ScanReport> {
    clients::bootstrap(&mut ctx.db, home)?;

    let specs = clients::loaded_specs(&ctx.db)?;
    let mut projects = project_roots(ctx)?;
    for root in &req.extra_roots {
        if !projects
            .iter()
            .any(|p| heron_platform::paths::same_path_ci(p, root))
        {
            projects.push(root.clone());
        }
    }
    let classifier = Classifier::new(specs, projects);

    let mut options = ScanOptions {
        extra_excludes: req.extra_excludes.clone(),
        ..ScanOptions::default()
    };
    if let Some(limit) = req.max_dirs {
        options.max_dirs = limit;
    }

    let result = scan(&classifier, &options)?;
    persist(ctx, &result)?;
    Ok(ScanReport::from_result(&result))
}

fn persist(ctx: &mut Ctx, result: &ScanResult) -> Result<()> {
    let mut seen: HashSet<String> = HashSet::new();
    ctx.db.write(|tx| {
        for hit in &result.hits {
            let path = hit.path.to_string_lossy().to_string();
            seen.insert(path.clone());
            detected::upsert(
                tx,
                &DetectedUpsert {
                    path,
                    real_path: Some(hit.real_path.to_string_lossy().to_string()),
                    client_id: hit.client.clone(),
                    scope: hit.scope.map(|s| match s {
                        heron_scan::classify::Scope::Global => "global".to_owned(),
                        heron_scan::classify::Scope::Project => "project".to_owned(),
                    }),
                    project_path: hit
                        .project
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    observed_slug: hit.slug.clone(),
                    content_hash: None,
                    frontmatter_ok: Some(hit.frontmatter_ok()),
                    kind: match hit.kind {
                        HitKind::Skill => "skill",
                        HitKind::LegacyCommand => "legacy-command",
                        HitKind::NestedSuspect => "nested-suspect",
                    }
                    .to_owned(),
                    mtime_ns: file_mtime_ns(&hit.path),
                    size_bytes: file_size(&hit.path),
                },
            )?;
        }
        detected::prune_not_in(tx, &seen)?;
        Ok(())
    })?;
    ctx.db
        .kv_set("last_scan_at", &heron_platform::time::now_rfc3339())?;
    Ok(())
}

fn project_roots(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    Ok(target::list(ctx.db.conn())?
        .into_iter()
        .filter(|t| t.scope == "project")
        .filter_map(|t| t.project_path.map(PathBuf::from))
        .collect())
}

fn file_mtime_ns(path: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_nanos() as i64)
}

fn file_size(path: &std::path::Path) -> Option<i64> {
    Some(std::fs::metadata(path).ok()?.len() as i64)
}

/// Name and description as written in each detected file's frontmatter.
///
/// `detected` only persists the slug, and the discover page shows a card per skill, so these two
/// fields are read back from the file. Read-only and best-effort: an unreadable or sloppy file
/// yields empty fields instead of failing the whole list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedMeta {
    pub path: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

pub fn describe(paths: &[String]) -> Vec<DetectedMeta> {
    paths
        .iter()
        .map(|path| {
            let fields = read_frontmatter_head(path).map(|bytes| {
                let (frontmatter, _) = heron_scan::parse_skill_md(&bytes);
                (frontmatter.name, frontmatter.description)
            });
            let (name, description) = fields.unwrap_or((None, None));
            DetectedMeta {
                path: path.clone(),
                name: name.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()),
                description: description
                    .map(|v| v.trim().to_owned())
                    .filter(|v| !v.is_empty()),
            }
        })
        .collect()
}

/// At most one frontmatter block: the parser ignores anything past this limit anyway, and a stray
/// large file must not be loaded whole just to show a card.
fn read_frontmatter_head(path: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(heron_scan::parse::FRONTMATTER_SCAN_LIMIT as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}
