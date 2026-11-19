//! The scanner: walk configured roots, classify hits, parse frontmatter.
//!
//! Never mutates the file system, never follows links, never recurses into a directory that
//! already contains `SKILL.md` (that directory *is* the skill).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use heron_core::Result;
use serde::Serialize;
use walkdir::WalkDir;

use crate::classify::{Classifier, HitKind, Scope};
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
    let started = Instant::now();
    let rules = IgnoreRules::new(&options.extra_excludes)?;
    let mut stats = ScanStats::default();
    let mut hits: Vec<Hit> = Vec::new();
    let mut nested: Vec<NestedSuspect> = Vec::new();

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

    for root in roots {
        if !root.exists() {
            stats.roots_missing.push(root);
            continue;
        }
        let walker = WalkDir::new(&root)
            .max_depth(options.max_depth)
            .follow_links(options.follow_links)
            .sort_by_file_name();

        for entry in walker.into_iter() {
            if stats.truncated {
                break;
            }
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
    }

    hits.sort_by(|a, b| a.path.cmp(&b.path));
    stats.duration_ms = started.elapsed().as_millis() as u64;
    let _ = Duration::from_millis(0);
    Ok(ScanResult {
        hits,
        nested,
        stats,
    })
}

fn classify_skill(dir: &Path, classifier: &Classifier) -> Option<Hit> {
    let skill_md = dir.join("SKILL.md");
    let classification = classifier.classify(&skill_md, false)?;
    let (frontmatter, issues) = crate::parse::parse_file(&skill_md).ok()?;
    let slug = frontmatter
        .name
        .as_deref()
        .and_then(|n| heron_core::Slug::derive(n, heron_core::SlugPolicy::Lenient).ok())
        .map(|s| s.as_str().to_owned())
        .or_else(|| classifier.slug_for(&skill_md, &classification));
    Some(Hit {
        path: skill_md.clone(),
        real_path: std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()),
        kind: classification.kind,
        client: classification.client,
        scope: Some(classification.scope),
        project: classification.project,
        slug,
        frontmatter: Some(frontmatter),
        issues,
    })
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
