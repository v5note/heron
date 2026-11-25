//! Install a skill from a remote repository (design §8.2 B).
//!
//! Flow: parse input → fetch into the bare cache → read catalog → pick the skill directory →
//! extract into a staging directory → validate frontmatter → move into the store → commit the
//! repository with the upstream commit pinned in `heron.skill.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Slug, SlugPolicy};
use heron_platform::dirs::Dirs;
use heron_source::catalog::{self, Catalog, RepoFiles};
use heron_source::url::{self, SourceKind};
use heron_store::models::SkillRow;
use heron_store::repos::{event, skill};

use crate::{Ctx, Result};

#[derive(Debug, Clone, Default)]
pub struct InstallRequest {
    /// URL, `owner/repo`, tree link or local path.
    pub input: String,
    /// Overrides the ref parsed from the input.
    pub ref_override: Option<String>,
    /// Overrides the subpath parsed from the input.
    pub subpath_override: Option<String>,
    /// Install under a different slug (when two sources publish the same name).
    pub as_slug: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallPreview {
    pub source: url::ParsedSource,
    /// Candidate skill directories found in the repository.
    pub available: Vec<String>,
    /// The one that would be installed. `None` means the repository holds several distinct skills
    /// and none of them is declared as the intended one: `available` is the choice to present, and
    /// an install without picking from it fails with `E_SOURCE_AMBIGUOUS`.
    pub selected: Option<String>,
    pub slug: String,
    pub commit: String,
    pub ref_name: Option<String>,
    /// True when the skill already exists and installing again would upgrade it.
    pub existing: bool,
    /// The skill cannot stand alone outside its repository (e.g. `huashu-motion`, whose `bin/om`
    /// reaches `../../..` for `cli/`): the install exports the whole checkout instead of lifting
    /// the skill directory out of it.
    pub whole_repo: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallReport {
    pub slug: String,
    pub path: String,
    pub version: String,
    pub commit: String,
    pub source: String,
}

/// Everything the install needs from the remote, resolved and validated.
struct Resolved {
    parsed: url::ParsedSource,
    catalog: Catalog,
    /// Which skill directory the source points at. `None` means the repository holds several and
    /// the author never declared which is theirs — a question for the user, answered by the
    /// candidate picker, not something to decide silently. Only `preview` may proceed without one;
    /// every install path goes through `chosen`.
    skill_dir: Option<String>,
    /// Whether `skill_dir` can be lifted out of the repository or needs the whole checkout.
    /// Meaningless while `skill_dir` is `None`, where it stays at its default.
    layout: heron_source::SkillLayout,
    /// The frontmatter `name` of the selected skill, when it is readable. A repository whose
    /// skill sits at the root has no directory name to fall back on, so this is what keeps the
    /// derived slug meaningful instead of the placeholder `root`.
    skill_name: Option<String>,
    commit: String,
    ref_name: Option<String>,
}

impl Resolved {
    /// The directory to install, or the ambiguity error that standing in for the user's choice.
    ///
    /// Carries the same `reason` / `count` / `examples` parameters as
    /// `catalog::resolve_skill_dir`, since that is what the CLI and the UI both expand into a
    /// candidate list.
    fn chosen(&self) -> Result<&String> {
        self.skill_dir.as_ref().ok_or_else(|| {
            let paths: Vec<&str> = self.catalog.items.iter().map(|i| i.path.as_str()).collect();
            ErrorCode::SourceAmbiguous
                .msg()
                .with("reason", "multiple_skills")
                .with("count", paths.len().to_string())
                .with(
                    "examples",
                    paths.iter().copied().take(5).collect::<Vec<_>>().join(", "),
                )
                .into()
        })
    }
}

/// What a request points at, decided without touching the network.
struct Target {
    parsed: url::ParsedSource,
    remote: heron_git::RemoteUrl,
    wanted_ref: Option<String>,
    wanted_sub: Option<String>,
}

fn target(req: &InstallRequest) -> Result<Target> {
    let parsed = url::parse_source(&req.input)?;
    let (remote_url, ref_from_url, subpath_from_url) = match &parsed.kind {
        SourceKind::Git {
            url,
            ref_name,
            subpath,
        } => (url.clone(), ref_name.clone(), subpath.clone()),
        SourceKind::Local { .. } => {
            return Err(ErrorCode::SourceAmbiguous
                .msg()
                .with("input", req.input.clone())
                .with("reason", "local_paths_are_adopted_not_installed")
                .into())
        }
        SourceKind::Archive { .. } => {
            return Err(ErrorCode::SourceAmbiguous
                .msg()
                .with("input", req.input.clone())
                .with("reason", "archive_not_supported_yet")
                .into())
        }
    };
    Ok(Target {
        parsed,
        remote: heron_git::RemoteUrl::parse(&remote_url)?,
        wanted_ref: req.ref_override.clone().or(ref_from_url),
        wanted_sub: req.subpath_override.clone().or(subpath_from_url),
    })
}

/// The network stage on its own. `Dirs` is cloneable precisely so that a caller can spend the
/// whole fetch budget outside its state lock: a blocked route then costs this install, and
/// nothing else, while every other command stays responsive.
pub fn prefetch(dirs: &Dirs, req: &InstallRequest) -> Result<()> {
    let mut repo = heron_git::CachedRepo::new(&dirs.sources_cache(), target(req)?.remote);
    repo.ensure()
}

fn resolve_remote(dirs: &Dirs, req: &InstallRequest) -> Result<Resolved> {
    let t = target(req)?;
    let mut repo = heron_git::CachedRepo::new(&dirs.sources_cache(), t.remote);
    repo.ensure()?;

    let resolved_ref = repo.resolve(t.wanted_ref.as_deref())?;
    let commit = resolved_ref.commit.clone();
    let Target {
        parsed,
        wanted_ref,
        wanted_sub,
        ..
    } = t;

    let (skill_dir, layout) = match wanted_sub.as_deref() {
        Some(sub) if !sub.is_empty() => {
            let normalized = sub.trim_matches('/').to_owned();
            let skill_dir =
                catalog::resolve_skill_dir(Some(&normalized), &empty_catalog(), &|p| {
                    repo.tree(&commit, p)
                        .map(|e| e.iter().any(|f| f.path == "SKILL.md"))
                        .unwrap_or(false)
                })?;
            let files = tree_files(&repo, &commit);
            let layout = detect_layout(&repo, &commit, &skill_dir, &files);
            (skill_dir, layout)
        }
        _ => {
            let tree = repo.tree(&commit, "")?;
            let files: Vec<String> = tree
                .iter()
                .filter(|e| e.kind == heron_git::TreeEntryKind::File && !e.path.contains(".git"))
                .map(|e| e.path.clone())
                .collect();
            // Only the manifests are read as blobs: `catalog::load` needs their content, while the
            // scan beneath it only ever needs the path list. Forgetting an entry here silently
            // demotes that catalog to the scan fallback, because `read` would return `None`.
            let contents: BTreeMap<String, String> = [
                "heron.source.toml",
                ".claude-plugin/marketplace.json",
                ".claude-plugin/plugin.json",
            ]
            .iter()
            .filter(|p| files.iter().any(|f| f == *p))
            .filter_map(|p| {
                repo.read_blob(&commit, p)
                    .ok()
                    .map(|c| ((*p).to_owned(), c))
            })
            .collect();
            let read = |p: &str| contents.get(p).cloned();
            let files_ref = RepoFiles {
                files: &files,
                read: &read,
            };
            let catalog = catalog::load(&files_ref)?;
            let skill_dir = catalog::resolve_skill_dir(None, &catalog, &|p| {
                repo.tree(&commit, p)
                    .map(|e| e.iter().any(|f| f.path == "SKILL.md"))
                    .unwrap_or(false)
            });
            // Ambiguity *with candidates* is a question, not a failure: hand the list to the
            // preview so the UI can offer a choice. A repository with no skills at all, or one the
            // caller under-specified some other way, still has to say so.
            let skill_dir = match skill_dir {
                Ok(dir) => Some(dir),
                Err(err) if err.code() == "E_SOURCE_AMBIGUOUS" && catalog.items.len() > 1 => None,
                Err(err) => return Err(err),
            };
            let (skill_name, layout) = match &skill_dir {
                Some(dir) => (
                    read_skill_name(&repo, &commit, dir),
                    detect_layout(&repo, &commit, dir, &files),
                ),
                None => (None, heron_source::SkillLayout::Skill),
            };
            return Ok(Resolved {
                parsed,
                catalog,
                skill_name,
                skill_dir,
                layout,
                commit,
                ref_name: wanted_ref,
            });
        }
    };

    // Explicit subpath: build a single-item catalog for preview purposes.
    let catalog = Catalog {
        source: catalog::CatalogSource::ScanFallback,
        items: vec![catalog::CatalogItem {
            slug: skill_dir
                .rsplit('/')
                .next()
                .unwrap_or(&skill_dir)
                .to_owned(),
            path: skill_dir.clone(),
            version: None,
            description: None,
        }],
    };
    Ok(Resolved {
        parsed,
        catalog,
        skill_name: read_skill_name(&repo, &commit, &skill_dir),
        skill_dir: Some(skill_dir),
        layout,
        commit,
        ref_name: wanted_ref,
    })
}

/// All blobs in the commit tree, ignoring git-internal paths.
fn tree_files(repo: &heron_git::CachedRepo, commit: &str) -> Vec<String> {
    repo.tree(commit, "")
        .map(|tree| {
            tree.iter()
                .filter(|e| e.kind == heron_git::TreeEntryKind::File && !e.path.contains(".git"))
                .map(|e| e.path.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Can the skill at `skill_dir` be lifted out of the checkout, or does it reference siblings?
/// `files` must be the repository-wide blob list from `tree_files`.
fn detect_layout(
    repo: &heron_git::CachedRepo,
    commit: &str,
    skill_dir: &str,
    files: &[String],
) -> heron_source::SkillLayout {
    heron_source::detect_layout(skill_dir, files, &|p| repo.read_blob(commit, p).ok())
}

/// The frontmatter `name` of the skill at `skill_dir`, best-effort: an unreadable or unparsable
/// blob just means the caller falls back to the next slug candidate.
fn read_skill_name(repo: &heron_git::CachedRepo, commit: &str, skill_dir: &str) -> Option<String> {
    let path = if skill_dir.is_empty() {
        "SKILL.md".to_owned()
    } else {
        format!("{skill_dir}/SKILL.md")
    };
    let text = repo.read_blob(commit, &path).ok()?;
    heron_scan::parse_skill_md(text.as_bytes())
        .0
        .name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// The repository name from a git source URL (`owner/repo.git` → `repo`).
fn repo_name(parsed: &url::ParsedSource) -> String {
    let SourceKind::Git { url, .. } = &parsed.kind else {
        return String::new();
    };
    let tail = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    tail.strip_suffix(".git").unwrap_or(tail).to_owned()
}

/// The first candidate that survives slug normalization.
fn first_slug(candidates: impl IntoIterator<Item = String>) -> Option<String> {
    candidates
        .into_iter()
        .filter(|c| !c.trim().is_empty())
        .find_map(|c| Slug::derive(&c, SlugPolicy::Lenient).ok())
        .map(|s| s.as_str().to_owned())
}

fn empty_catalog() -> Catalog {
    Catalog {
        source: catalog::CatalogSource::ScanFallback,
        items: vec![],
    }
}

/// Compute what an install would do without touching the store.
pub fn preview(ctx: &Ctx, req: &InstallRequest) -> Result<InstallPreview> {
    let resolved = resolve_remote(&ctx.dirs, req)?;
    let slug = req.as_slug.clone().unwrap_or_else(|| {
        let dir_tail = resolved
            .skill_dir
            .as_deref()
            .and_then(|dir| dir.rsplit('/').next())
            .unwrap_or("")
            .to_owned();
        first_slug([
            dir_tail,
            resolved.skill_name.clone().unwrap_or_default(),
            repo_name(&resolved.parsed),
        ])
        .unwrap_or_else(|| "root".to_owned())
    });
    let slug = Slug::derive(&slug, SlugPolicy::Lenient)?
        .as_str()
        .to_owned();
    let existing = skill::get(ctx.db.conn(), &slug)?.is_some();
    Ok(InstallPreview {
        source: resolved.parsed,
        available: resolved
            .catalog
            .items
            .iter()
            .map(|i| i.path.clone())
            .collect(),
        selected: resolved.skill_dir,
        slug,
        commit: resolved.commit,
        ref_name: resolved.ref_name,
        existing,
        whole_repo: resolved.layout.whole_repo(),
    })
}

/// Install (or replace) a skill from a remote.
pub fn install(ctx: &mut Ctx, req: &InstallRequest) -> Result<InstallReport> {
    let preview = preview(ctx, req)?;
    if preview.existing {
        return Err(ErrorCode::SlugConflict
            .msg()
            .with("slug", preview.slug.clone())
            .with("detail", "use upgrade for an already installed skill")
            .into());
    }

    let resolved = resolve_remote(&ctx.dirs, req)?;
    // Installing needs a decision the preview was allowed to leave open.
    let skill_dir = resolved.chosen()?.clone();
    let remote_url = match &resolved.parsed.kind {
        SourceKind::Git { url, .. } => url.clone(),
        _ => unreachable!("resolve_remote rejected non-git sources"),
    };
    let remote = heron_git::RemoteUrl::parse(&remote_url)?;
    let repo = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote.clone());

    // Extract into a staging dir first so a bad download never reaches the store. A whole-repo
    // skill keeps its checkout intact: the store directory IS the repository root and the entry
    // lives at `skill_dir` inside it, exactly like a plain `git clone` would leave it.
    let whole_repo = resolved.layout.whole_repo();
    let staging_root = ctx.dirs.data.join("staging");
    std::fs::create_dir_all(&staging_root).ok();
    let staging = staging_root.join(format!("{}-{}", preview.slug, &resolved.commit[..8]));
    let _ = std::fs::remove_dir_all(&staging);
    repo.checkout_subdir(
        &resolved.commit,
        if whole_repo { "" } else { &skill_dir },
        &staging,
    )?;

    let skill_md = if whole_repo {
        staging.join(&skill_dir).join("SKILL.md")
    } else {
        staging.join("SKILL.md")
    };
    if !skill_md.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ErrorCode::Frontmatter
            .msg()
            .with("detail", "no SKILL.md at the selected path")
            .with("path", skill_dir.clone())
            .into());
    }
    let bytes = std::fs::read(&skill_md).map_err(|e| {
        heron_core::HeronError::from(ErrorCode::Permission.msg().with("detail", e.to_string()))
    })?;
    let (frontmatter, issues) = heron_scan::parse_skill_md(&bytes);
    if !frontmatter.is_ok() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ErrorCode::Frontmatter
            .msg()
            .with(
                "detail",
                issues
                    .iter()
                    .map(|i| i.detail.clone())
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .into());
    }

    // Move into the store and commit with the upstream commit as baseline.
    std::fs::create_dir_all(ctx.store_root()).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", ctx.store_root().display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    let dest = ctx.skill_path(&preview.slug);
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ErrorCode::SlugConflict
            .msg()
            .with("slug", preview.slug.clone())
            .into());
    }
    std::fs::rename(&staging, &dest).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", dest.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;

    let version = frontmatter
        .metadata_version()
        .map(|v| heron_core::Version::parse(v, Some(&resolved.commit)).to_string())
        .unwrap_or_else(|| format!("{}+{}", heron_core::Version::ZERO, &resolved.commit[..7]));
    let mut meta = heron_vcs::SkillMeta::new(&preview.slug, "upstream", &resolved.commit, &version);
    meta.source = Some(heron_vcs::repo::MetaSource {
        url: remote.url.clone(),
        ref_name: resolved.ref_name.clone(),
        // Whole-repo: the store directory is the versioned root already, so upstream paths are
        // relative to it without a prefix. Skill layout keeps the lifted-out subtree marker.
        subpath: if whole_repo {
            None
        } else {
            Some(skill_dir.clone())
        },
    });
    let vrepo = heron_vcs::SkillRepo::open(&dest);
    vrepo.init(&meta)?;
    // Remember the local commit that materialized this upstream revision: diffs and rollbacks
    // use it, because the upstream sha may never exist in this repository.
    let installed_head = vrepo.head()?;
    meta.baseline.local_commit = Some(installed_head.clone());
    vrepo.write_metadata(&meta)?;
    vrepo.commit_all("heron: record baseline")?;
    meta.baseline.local_commit = Some(vrepo.head()?);
    // The upstream commit lives in the *remote* repository; in the skill's own repository the
    // equivalent is the commit Heron just created. Tag that, and keep the upstream sha in
    // `heron.skill.json` as the merge base.
    let local_head = vrepo.head()?;
    vrepo.tag_version(&version, &local_head)?;

    let description = frontmatter.description.clone().unwrap_or_default();
    ctx.db.write(|tx| {
        skill::insert(
            tx,
            &SkillRow {
                slug: preview.slug.clone(),
                display_name: frontmatter
                    .name
                    .clone()
                    .unwrap_or_else(|| preview.slug.clone()),
                description,
                tags: vec![],
                status: "active".to_owned(),
                origin: "store".to_owned(),
                storage_path: None,
                // The DB records the inner entry point for whole-repo installs; upgrades and
                // projection both resolve `content_path + subpath` from here.
                subpath: if whole_repo {
                    Some(skill_dir.clone())
                } else {
                    None
                },
                created_at: String::new(),
                updated_at: String::new(),
            },
        )?;
        event::record(
            tx,
            ctx.actor,
            "install",
            Some(&preview.slug),
            Some(&format!("{} @ {}", remote.url, &resolved.commit[..8])),
            false,
        )?;
        Ok(())
    })?;

    Ok(InstallReport {
        slug: preview.slug,
        path: dest.to_string_lossy().to_string(),
        version,
        commit: resolved.commit,
        source: remote.url,
    })
}

/// Where the store keeps remote skills, exposed for tests.
pub fn store_root(ctx: &Ctx) -> PathBuf {
    ctx.dirs.store()
}

#[allow(dead_code)]
fn _p(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(raw: &str) -> url::ParsedSource {
        url::ParsedSource {
            original: raw.to_owned(),
            kind: SourceKind::Git {
                url: raw.to_owned(),
                ref_name: None,
                subpath: None,
            },
            skill_subpath: None,
            candidates: vec![],
            notes: vec![],
        }
    }

    #[test]
    fn repo_root_skill_uses_the_frontmatter_name() {
        // The skill sits at the repository root: there is no directory name, so the previous
        // behaviour silently produced the placeholder `root`.
        let parsed = git("https://github.com/alchaincyf/huashu-design");
        let got = first_slug([
            String::new(),
            "huashu-design".to_owned(),
            repo_name(&parsed),
        ]);
        assert_eq!(got.as_deref(), Some("huashu-design"));
    }

    #[test]
    fn repo_root_skill_falls_back_to_the_repository_name() {
        let parsed = git("https://github.com/alchaincyf/huashu-design.git");
        let got = first_slug([String::new(), String::new(), repo_name(&parsed)]);
        assert_eq!(got.as_deref(), Some("huashu-design"));
    }

    #[test]
    fn directory_name_wins_over_the_frontmatter() {
        let parsed = git("https://github.com/v5note/wechat-mp");
        let got = first_slug([
            "wechat-mp-tools".to_owned(),
            "WeChat MP Toolkit".to_owned(),
            repo_name(&parsed),
        ]);
        assert_eq!(got.as_deref(), Some("wechat-mp-tools"));
    }

    #[test]
    fn nothing_usable_yields_no_slug_so_the_caller_keeps_root() {
        assert_eq!(first_slug([String::new(), "、。".to_owned()]), None);
        assert_eq!(repo_name(&git("")), "");
    }
}
