//! Install a skill from a remote repository (design §8.2 B).
//!
//! Flow: parse input → fetch into the bare cache → read catalog → pick the skill directory →
//! extract into a staging directory → validate frontmatter → move into the store → commit the
//! repository with the upstream commit pinned in `heron.skill.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Slug, SlugPolicy};
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
    /// The one that would be installed.
    pub selected: Option<String>,
    pub slug: String,
    pub commit: String,
    pub ref_name: Option<String>,
    /// True when the skill already exists and installing again would upgrade it.
    pub existing: bool,
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
    skill_dir: String,
    commit: String,
    ref_name: Option<String>,
}

fn resolve_remote(ctx: &Ctx, req: &InstallRequest) -> Result<Resolved> {
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

    let remote = heron_git::RemoteUrl::parse(&remote_url)?;
    let mut repo = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote);
    repo.ensure()?;

    let wanted_ref = req.ref_override.clone().or(ref_from_url);
    let resolved_ref = repo.resolve(wanted_ref.as_deref())?;
    let commit = resolved_ref.commit.clone();

    let wanted_sub = req.subpath_override.clone().or(subpath_from_url);
    let skill_dir = match wanted_sub.as_deref() {
        Some(sub) if !sub.is_empty() => {
            let normalized = sub.trim_matches('/').to_owned();
            catalog::resolve_skill_dir(Some(&normalized), &empty_catalog(), &|p| {
                repo.tree(&commit, p)
                    .map(|e| e.iter().any(|f| f.path == "SKILL.md"))
                    .unwrap_or(false)
            })?
        }
        _ => {
            let tree = repo.tree(&commit, "")?;
            let files: Vec<String> = tree
                .iter()
                .filter(|e| e.kind == heron_git::TreeEntryKind::File && !e.path.contains(".git"))
                .map(|e| e.path.clone())
                .collect();
            let contents: BTreeMap<String, String> =
                ["heron.source.toml", ".claude-plugin/marketplace.json"]
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
            })?;
            return Ok(Resolved {
                parsed,
                catalog,
                skill_dir,
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
        skill_dir,
        commit,
        ref_name: wanted_ref,
    })
}

fn empty_catalog() -> Catalog {
    Catalog {
        source: catalog::CatalogSource::ScanFallback,
        items: vec![],
    }
}

/// Compute what an install would do without touching the store.
pub fn preview(ctx: &Ctx, req: &InstallRequest) -> Result<InstallPreview> {
    let resolved = resolve_remote(ctx, req)?;
    let wanted_slug = req.as_slug.clone().unwrap_or_else(|| {
        resolved
            .skill_dir
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("root")
            .to_owned()
    });
    let slug = Slug::derive(&wanted_slug, SlugPolicy::Lenient)?
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
        selected: Some(resolved.skill_dir),
        slug,
        commit: resolved.commit,
        ref_name: resolved.ref_name,
        existing,
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

    let resolved = resolve_remote(ctx, req)?;
    let remote_url = match &resolved.parsed.kind {
        SourceKind::Git { url, .. } => url.clone(),
        _ => unreachable!("resolve_remote rejected non-git sources"),
    };
    let remote = heron_git::RemoteUrl::parse(&remote_url)?;
    let repo = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote.clone());

    // Extract into a staging dir first so a bad download never reaches the store.
    let staging_root = ctx.dirs.data.join("staging");
    std::fs::create_dir_all(&staging_root).ok();
    let staging = staging_root.join(format!("{}-{}", preview.slug, &resolved.commit[..8]));
    let _ = std::fs::remove_dir_all(&staging);
    repo.checkout_subdir(&resolved.commit, &resolved.skill_dir, &staging)?;

    let skill_md = staging.join("SKILL.md");
    if !skill_md.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ErrorCode::Frontmatter
            .msg()
            .with("detail", "no SKILL.md at the selected path")
            .with("path", resolved.skill_dir.clone())
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
        subpath: Some(resolved.skill_dir.clone()),
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
