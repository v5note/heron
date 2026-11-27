//! The scanner: walk configured roots, classify hits, parse frontmatter.
//!
//! Never mutates the file system, never follows links, never recurses into a directory that
//! already contains `SKILL.md` (that directory *is* the skill).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use heron_core::Result;
use serde::Serialize;
use walkdir::WalkDir;

use crate::classify::{Classifier, HitKind, Scope, CUSTOM_CLIENT_ID};
use crate::parse::{parse_skill_md, Confidence, Frontmatter, ParseIssue};
use crate::rules::IgnoreRules;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Extra absolute roots to descend into (project roots), in addition to client paths.
    pub project_roots: Vec<PathBuf>,
    pub max_depth: usize,
    pub max_dirs: usize,
    pub follow_links: bool,
    pub extra_excludes: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            project_roots: Vec::new(),
            max_depth: 8,
            max_dirs: 50_000,
            follow_links: false,
            extra_excludes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub path: PathBuf,
    pub real_path: PathBuf,
    pub kind: HitKind,
    pub client: Option<String>,
    pub scope: Option<Scope>,
    pub project: Option<PathBuf>,
    pub slug: Option<String>,
    pub frontmatter: Option<Frontmatter>,
    pub issues: Vec<ParseIssue>,
}

impl Hit {
    pub fn frontmatter_ok(&self) -> bool {
        self.frontmatter
            .as_ref()
            .map(|f| f.is_ok() && f.confidence != Confidence::None)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NestedSuspect {
    pub path: PathBuf,
    pub client: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanStats {
    pub dirs_visited: usize,
    pub files_seen: usize,
    pub skipped_ignored: usize,
    pub skipped_depth: usize,
    pub duration_ms: u64,
    pub truncated: bool,
    pub roots_missing: Vec<PathBuf>,
    /// 1-based index of the root being walked; `0` before the first root starts.
    pub root_index: usize,
    /// Every root the walk intends to descend into, including the ones that turn out to be missing.
    pub root_total: usize,
    /// The root currently being walked, `None` once the traversal is over.
    pub current_root: Option<PathBuf>,
    /// The walk stopped because [`ScanMonitor::cancel`] was raised.
    pub canceled: bool,
}

/// Live progress is handed to the observer at most this often...
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);
/// ... and at least this often: whichever of the two limits is reached first decides.
pub const PROGRESS_DIRS: usize = 200;

/// What a monitored walk calls back into: the counters so far and **every** hit found so far.
pub type ProgressObserver<'a> = &'a mut dyn FnMut(&ScanStats, &[Hit]);

/// Cancellation + live progress plumbing for a monitored walk.
///
/// A walk never blocks on its observer: the observer runs on the walking thread, so whatever it
/// does (emitting an event) is part of the scan's cost.
#[derive(Default)]
pub struct ScanMonitor<'a> {
    /// Raised from elsewhere — a cancel button — to stop the walk as soon as possible.
    pub cancel: Option<&'a AtomicBool>,
    /// Called on the walker's throttle, and once at every root boundary. Gets the counters so far
    /// plus **every** hit found so far, so a caller that only wants the new ones can slice off the
    /// prefix it already handled.
    pub on_progress: Option<ProgressObserver<'a>>,
}

impl<'a> ScanMonitor<'a> {
    /// Walk to completion, report nothing: what the CLI and the tests use.
    pub fn quiet() -> Self {
        Self::default()
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub hits: Vec<Hit>,
    pub nested: Vec<NestedSuspect>,
    pub stats: ScanStats,
}

impl ScanResult {
    pub fn count(&self, kind: HitKind) -> usize {
        self.hits.iter().filter(|h| h.kind == kind).count()
    }
}

/// Run a scan over every configured client location and project root.
pub fn scan(classifier: &Classifier, options: &ScanOptions) -> Result<ScanResult> {
    scan_monitored(classifier, options, &mut ScanMonitor::quiet())
}

/// Every distinct root the walk descends into: each client's global and legacy command paths,
/// then the project roots. Split out so a caller can report `root_total` before the first entry
/// is read (the prepare phase of the GUI's progress event).
pub fn collect_roots(classifier: &Classifier, options: &ScanOptions) -> Vec<PathBuf> {
    // Roots: every distinct client global path, plus each project root.
    let mut roots: Vec<PathBuf> = Vec::new();
    for client in classifier.clients() {
        for p in client
            .global_paths
            .iter()
            .chain(client.command_paths.iter())
        {
            if !roots.iter().any(|r| {
                heron_platform::paths::comparison_key(r) == heron_platform::paths::comparison_key(p)
            }) {
                roots.push(p.clone());
            }
        }
    }
    for project in classifier
        .projects()
        .iter()
        .chain(options.project_roots.iter())
    {
        if !roots.iter().any(|r| {
            heron_platform::paths::comparison_key(r)
                == heron_platform::paths::comparison_key(project)
        }) {
            roots.push(project.clone());
        }
    }
    roots
}

/// Walk [`collect_roots`], reporting progress on a throttle and stopping as soon as
/// `monitor.cancel` is raised. Whatever was found up to that point is returned: a cancelled walk
/// is a partial result, not a failure.
pub fn scan_monitored(
    classifier: &Classifier,
    options: &ScanOptions,
    monitor: &mut ScanMonitor<'_>,
) -> Result<ScanResult> {
    let started = Instant::now();
    let rules = IgnoreRules::new(&options.extra_excludes)?;
    let mut stats = ScanStats::default();
    let mut hits: Vec<Hit> = Vec::new();
    let mut nested: Vec<NestedSuspect> = Vec::new();
    let roots = collect_roots(classifier, options);
    stats.root_total = roots.len();

    // Throttle state for the observer: 120ms or 200 directories, whichever comes first.
    let mut tick = Instant::now();
    let mut dirs_at_tick = 0usize;

    for (index, root) in roots.iter().enumerate() {
        if monitor.cancelled() {
            stats.canceled = true;
            break;
        }
        stats.root_index = index + 1;
        stats.current_root = Some(root.clone());
        if !root.exists() {
            stats.roots_missing.push(root.clone());
            report(monitor, &mut tick, &mut dirs_at_tick, &stats, &hits, true);
            continue;
        }
        let walker = WalkDir::new(root)
            .max_depth(options.max_depth)
            .follow_links(options.follow_links)
            .sort_by_file_name();

        for entry in walker.into_iter() {
            if monitor.cancelled() {
                stats.canceled = true;
                break;
            }
            if stats.truncated {
                break;
            }
            report(monitor, &mut tick, &mut dirs_at_tick, &stats, &hits, false);
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    // Permission errors inside other people's folders are not fatal.
                    continue;
                }
            };
            let path = entry.path();
            if path == root.as_path() {
                continue;
            }

            if path.is_dir() {
                stats.dirs_visited += 1;
                if stats.dirs_visited >= options.max_dirs {
                    stats.truncated = true;
                    break;
                }
                if rules.is_ignored(path)
                    || IgnoreRules::is_noise_file(entry.file_name().to_string_lossy().as_ref())
                {
                    stats.skipped_ignored += 1;
                    continue;
                }
                // Stop descending as soon as this directory is a skill.
                if path.join("SKILL.md").is_file() {
                    if let Some(hit) = classify_skill(path, classifier) {
                        if classifier.is_nested_suspect(path) {
                            nested.push(NestedSuspect {
                                path: path.to_path_buf(),
                                client: hit.client.clone(),
                            });
                            hits.push(Hit {
                                kind: HitKind::NestedSuspect,
                                ..hit
                            });
                        } else {
                            hits.push(hit);
                        }
                    }
                }
                continue;
            }

            if entry.file_name() == "SKILL.md" {
                // Handled when the parent directory was visited.
                continue;
            }
            stats.files_seen += 1;
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".md") && is_in_commands_dir(path, classifier) {
                if let Some(hit) = classify_command(path, classifier) {
                    hits.push(hit);
                }
            }
        }
        if stats.canceled {
            break;
        }
        // Root boundary: a tail of hits must not wait for the next throttle window.
        report(monitor, &mut tick, &mut dirs_at_tick, &stats, &hits, true);
    }

    stats.current_root = None;
    // The traversal ended early (cancelled or truncated) without reaching the boundary report.
    report(monitor, &mut tick, &mut dirs_at_tick, &stats, &hits, true);

    hits.sort_by(|a, b| a.path.cmp(&b.path));
    stats.duration_ms = started.elapsed().as_millis() as u64;
    Ok(ScanResult {
        hits,
        nested,
        stats,
    })
}

/// Hand the current counters to the observer, at most once per [`PROGRESS_INTERVAL`] /
/// [`PROGRESS_DIRS`] unless `force` is set (root boundaries and the end of the walk).
fn report(
    monitor: &mut ScanMonitor<'_>,
    tick: &mut Instant,
    dirs_at_tick: &mut usize,
    stats: &ScanStats,
    hits: &[Hit],
    force: bool,
) {
    if monitor.on_progress.is_none() {
        return;
    }
    if !force
        && stats.dirs_visited - *dirs_at_tick < PROGRESS_DIRS
        && tick.elapsed() < PROGRESS_INTERVAL
    {
        return;
    }
    *tick = Instant::now();
    *dirs_at_tick = stats.dirs_visited;
    if let Some(observer) = &mut monitor.on_progress {
        observer(stats, hits);
    }
}

fn classify_skill(dir: &Path, classifier: &Classifier) -> Option<Hit> {
    let skill_md = dir.join("SKILL.md");
    // Classify on the path as encountered: `Classifier::classify` matches projection links by that
    // path, so Heron's own projections are suppressed here before we ever consider resolving.
    let classification = classifier.classify(&skill_md, false)?;
    let (frontmatter, issues) = crate::parse::parse_file(&skill_md).ok()?;
    let slug = frontmatter
        .name
        .as_deref()
        .and_then(|n| heron_core::Slug::derive(n, heron_core::SlugPolicy::Lenient).ok())
        .map(|s| s.as_str().to_owned())
        .or_else(|| classifier.slug_for(&skill_md, &classification));
    let real_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    // The link we followed can land inside the store: a user-made link in one client's directory
    // pointing at a link Heron wrote, which itself points at `~/.heron/skills/<slug>`. The content
    // there is already taken over, so resolving it must not hand the library's own copy back as a
    // discovery.
    if classifier.is_store_content(&real_dir) {
        return None;
    }
    // A skill reached through a user-made symlink (its real directory lives outside the scanned
    // roots) is reported at that real directory, not at the link: Discover shows the skill's own
    // home, and adopting it then treats it as an ordinary directory. A symlink that survives the
    // projection filter above is always the user's, never one Heron created.
    let link = is_symlink(dir);
    let path = if link {
        real_dir.join("SKILL.md")
    } else {
        skill_md.clone()
    };
    // The source we report must match the address we show. A link sitting in `~/.agents/skills`
    // only tells us where the user pointed an agent at; the content it names lives elsewhere, and
    // that directory is not where the skill lives. So a followed link is credited to the client
    // that owns the real directory, and to no client when nothing does — the GUI then groups it
    // under「其他」instead of the client whose directory merely held the link.
    let (client, project) = if !link {
        (
            classification.client.clone(),
            classification.project.clone(),
        )
    } else if classifier.claims(&real_dir) {
        match classifier.classify(&path, false) {
            Some(owner) => (owner.client, owner.project),
            None => (Some(CUSTOM_CLIENT_ID.to_owned()), None),
        }
    } else {
        (Some(CUSTOM_CLIENT_ID.to_owned()), None)
    };
    Some(Hit {
        path,
        real_path: real_dir,
        kind: classification.kind,
        client,
        scope: Some(classification.scope),
        project,
        slug,
        frontmatter: Some(frontmatter),
        issues,
    })
}

/// True when `path` itself is a symbolic link (an intermediate component being a link does not count).
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

fn classify_command(file: &Path, classifier: &Classifier) -> Option<Hit> {
    let classification = classifier.classify(file, true)?;
    let bytes =
        heron_platform::atomic::read_head(file, crate::parse::FRONTMATTER_SCAN_LIMIT).ok()?;
    let (frontmatter, issues) = parse_skill_md(&bytes);
    let slug = classifier.slug_for(file, &classification);
    Some(Hit {
        path: file.to_path_buf(),
        real_path: file.to_path_buf(),
        kind: HitKind::LegacyCommand,
        client: classification.client,
        scope: Some(classification.scope),
        project: classification.project,
        slug,
        frontmatter: Some(frontmatter),
        issues,
    })
}

fn is_in_commands_dir(path: &Path, classifier: &Classifier) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    classifier.clients().iter().any(|c| {
        let mut candidates: Vec<PathBuf> = c.command_paths.clone();
        for rel in &c.project_command_paths {
            for project in classifier.projects() {
                candidates.push(project.join(rel));
            }
        }
        candidates.iter().any(|base| {
            let base_key = heron_platform::paths::comparison_key(base);
            let p_key = heron_platform::paths::comparison_key(parent);
            p_key == base_key || p_key.starts_with(&format!("{base_key}/"))
        })
    })
}
