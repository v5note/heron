//! Scan use case: run the scanner and persist what it found.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;

use heron_scan::classify::{Classifier, HitKind};
use heron_scan::walker::{collect_roots, scan_monitored, ScanMonitor, ScanOptions, ScanResult};
use heron_store::models::{DetectedRow, DetectedUpsert};
use heron_store::repos::{detected, projection, target};

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

/// A scan that is ready to run: configuration snapshot, plus the roots it will descend into.
///
/// It deliberately holds no lock. The GUI builds one on the IPC thread, then walks it on a worker
/// thread, so the database mutex is free for everything else the window asks for.
pub struct ScanPlan {
    classifier: Classifier,
    options: ScanOptions,
    roots: Vec<PathBuf>,
}

impl ScanPlan {
    /// The roots the walk will visit, in visit order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
}

/// Stage 1: read the configuration. This is the only stage that touches the database up front.
pub fn prepare(ctx: &mut Ctx, home: &std::path::Path, req: &ScanRequest) -> Result<ScanPlan> {
    prepare_with(ctx, home, req, &crate::detect::RealMachine)
}

/// The same stage with the probing environment injected, so presence detection can be tested
/// without installing or uninstalling anything.
pub fn prepare_with(
    ctx: &mut Ctx,
    home: &std::path::Path,
    req: &ScanRequest,
    machine: &dyn crate::detect::Machine,
) -> Result<ScanPlan> {
    clients::bootstrap(&mut ctx.db, home)?;
    // Fill in clients a newer binary added, so this very scan has rows to put verdicts on and the
    // grid does not lose a card it has not measured yet.
    clients::sync_catalog(&mut ctx.db, home, false)?;

    // Detection runs *before* the roots are collected: a client discovered for the first time today
    // is scanned today, in the same pass that decided it exists.
    let matrix = clients::builtin()?;
    let detection = crate::detect::run(
        &matrix,
        home,
        machine,
        &crate::detect::DetectOptions::default(),
    );
    // A write failure here is deliberately not fatal, and there is no logger to tell us about it:
    // the previous verdicts simply survive, and the next scan re-measures. Failing a scan the user
    // asked for because an index was busy would be the worse bug.
    let _ = clients::apply_presence(&mut ctx.db, &detection);

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
    // This app's own projections sit inside the scanned client directories. Telling the classifier
    // where they are keeps a taken-over skill out of the discovery list.
    let projected: Vec<PathBuf> = projection::list_all(ctx.db.conn())?
        .into_iter()
        .map(|row| PathBuf::from(row.path))
        .collect();
    let classifier = Classifier::new(specs, projects).with_projections(projected);

    let mut options = ScanOptions {
        extra_excludes: req.extra_excludes.clone(),
        ..ScanOptions::default()
    };
    if let Some(limit) = req.max_dirs {
        options.max_dirs = limit;
    }
    let roots = collect_roots(&classifier, &options);
    Ok(ScanPlan {
        classifier,
        options,
        roots,
    })
}

/// Stage 2: traverse and parse. Never touches the database or any lock, and stops early when the
/// monitor's cancel flag is raised.
pub fn walk(plan: &ScanPlan, monitor: &mut ScanMonitor<'_>) -> Result<ScanResult> {
    scan_monitored(&plan.classifier, &plan.options, monitor)
}

/// Drop the rows a scan indexed for a directory Heron projected itself.
///
/// A scan no longer reports those, but the index can still hold rows written before the projection
/// was recorded — an older build, or a projection made on this machine after the last walk. They are
/// managed content, so the discovery list must not offer them again.
pub fn without_projection_outputs(ctx: &Ctx, rows: Vec<DetectedRow>) -> Result<Vec<DetectedRow>> {
    let projected = projection::list_all(ctx.db.conn())?;
    if projected.is_empty() {
        return Ok(rows);
    }
    Ok(rows
        .into_iter()
        .filter(|row| {
            let Some(dir) = std::path::Path::new(&row.path).parent() else {
                return true;
            };
            let dir = heron_platform::paths::normalize(dir);
            !projected
                .iter()
                .any(|p| heron_platform::paths::same_path_ci(std::path::Path::new(&p.path), &dir))
        })
        .collect())
}

/// Stage 3: write what the walk found.
///
/// `prune` also drops the `detected` rows this scan did not see. A cancelled scan must pass
/// `false`: pruning against a partial hit list would delete rows for directories it never reached.
pub fn persist(ctx: &mut Ctx, result: &ScanResult, prune: bool) -> Result<()> {
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
                    scope: hit.scope.map(|s| s.as_str().to_owned()),
                    project_path: hit
                        .project
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    observed_slug: hit.slug.clone(),
                    content_hash: None,
                    frontmatter_ok: Some(hit.frontmatter_ok()),
                    kind: hit.kind.as_str().to_owned(),
                    mtime_ns: file_mtime_ns(&hit.path),
                    size_bytes: file_size(&hit.path),
                },
            )?;
        }
        if prune {
            detected::prune_not_in(tx, &seen)?;
        }
        Ok(())
    })?;
    ctx.db
        .kv_set("last_scan_at", &heron_platform::time::now_rfc3339())?;
    Ok(())
}

/// Stage 4: turn a finished walk into the numbers the report shows.
pub fn summarize(result: &ScanResult) -> ScanReport {
    ScanReport::from_result(result)
}

/// Run a scan and write the results into the index, every stage back to back. This is what the
/// CLI calls; the GUI runs the stages itself so it can report progress between them.
pub fn run(ctx: &mut Ctx, home: &std::path::Path, req: &ScanRequest) -> Result<ScanReport> {
    let plan = prepare(ctx, home, req)?;
    let result = walk(&plan, &mut ScanMonitor::quiet())?;
    persist(ctx, &result, true)?;
    Ok(summarize(&result))
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
