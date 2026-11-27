//! Outdated / upgrade / rollback (design §10.4, implementation doc §5.5).
//!
//! Upgrading a skill is a three-way merge between the baseline recorded at install time, the
//! user's current content and the new upstream commit. A conflict never touches the working
//! tree: the caller gets the conflicting files and decides.

use serde::Serialize;

use heron_core::{ErrorCode, PinPolicy};
use heron_store::repos::{event, skill, source};

use crate::{Ctx, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeState {
    UpToDate,
    HasUpdate,
    Pinned,
    LocalModified,
    Diverged,
    UpstreamGone,
    NotTracked,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeCandidate {
    pub slug: String,
    pub state: UpgradeState,
    pub installed: String,
    pub baseline: String,
    pub latest: Option<String>,
    pub source: Option<String>,
}

/// Compare every managed skill against the cached upstream state.
///
/// This never hits the network: callers refresh with `refresh_sources` first, so `outdated`
/// stays fast and works offline.
pub fn outdated(ctx: &Ctx, pin: PinPolicy) -> Result<Vec<UpgradeCandidate>> {
    let mut out = Vec::new();
    for row in skill::list(ctx.db.conn(), false)? {
        let dir = ctx.content_path(&row);
        if !dir.is_dir() {
            continue;
        }
        let repo = heron_vcs::SkillRepo::open(&dir);
        let Some(meta) = repo.read_metadata()? else {
            out.push(UpgradeCandidate {
                slug: row.slug.clone(),
                state: UpgradeState::NotTracked,
                installed: String::new(),
                baseline: String::new(),
                latest: None,
                source: None,
            });
            continue;
        };

        let Some(src) = meta.source.as_ref() else {
            out.push(UpgradeCandidate {
                slug: row.slug.clone(),
                state: UpgradeState::NotTracked,
                installed: meta.baseline.version.clone(),
                baseline: meta.baseline.commit.clone(),
                latest: None,
                source: None,
            });
            continue;
        };

        let remote = heron_git::RemoteUrl::parse(&src.url)?;
        let cached = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote);
        let latest = cached
            .resolve(src.ref_name.as_deref())
            .ok()
            .map(|r| r.commit);
        // Local edits may still be uncommitted when we check; compare the working tree.
        let has_local_changes = repo.has_local_changes()? || repo.worktree_differs_from_head()?;

        let state = match (&latest, has_local_changes) {
            (None, _) => UpgradeState::UpstreamGone,
            (Some(new), _) if Some(new) == Some(&meta.baseline.commit) => {
                if has_local_changes {
                    UpgradeState::LocalModified
                } else {
                    UpgradeState::UpToDate
                }
            }
            (Some(_), true) => UpgradeState::Diverged,
            (Some(_), false) => {
                if pin.is_pinned() {
                    UpgradeState::Pinned
                } else {
                    UpgradeState::HasUpdate
                }
            }
        };

        out.push(UpgradeCandidate {
            slug: row.slug.clone(),
            state,
            installed: meta.baseline.version.clone(),
            baseline: meta.baseline.commit.clone(),
            latest,
            source: Some(src.url.clone()),
        });
    }
    Ok(out)
}

/// Refresh every cached remote referenced by a managed skill.
///
/// Remotes are deduplicated by canonical URL before fetching (so multiple skills from the same
/// monorepo trigger one `git fetch`, not N) and fetched concurrently in small batches.
pub fn refresh_sources(ctx: &Ctx) -> Result<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut remotes = Vec::new();

    for row in source::list(ctx.db.conn())? {
        if row.kind != "git" || !row.enabled {
            continue;
        }
        let Some(url) = row.url else { continue };
        let remote = heron_git::RemoteUrl::parse(&url)?;
        if seen.insert(remote.url.clone()) {
            remotes.push(remote);
        }
    }
    // Remotes referenced only by a skill's metadata also need refreshing.
    for row in skill::list(ctx.db.conn(), false)? {
        let dir = ctx.content_path(&row);
        if !dir.is_dir() {
            continue;
        }
        let repo = heron_vcs::SkillRepo::open(&dir);
        let Some(Some(src)) = repo.read_metadata()?.map(|m| m.source) else {
            continue;
        };
        let remote = heron_git::RemoteUrl::parse(&src.url)?;
        if seen.insert(remote.url.clone()) {
            remotes.push(remote);
        }
    }

    let cache_root = ctx.dirs.sources_cache();
    let mut refreshed = 0usize;
    const WORKERS: usize = 4;
    for chunk in remotes.chunks(WORKERS) {
        let count: usize = std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|remote| {
                    let cache_root = cache_root.clone();
                    let remote = remote.clone();
                    s.spawn(move || {
                        let mut cached = heron_git::CachedRepo::new(&cache_root, remote);
                        usize::from(cached.ensure().is_ok())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or(0))
                .sum()
        });
        refreshed += count;
    }
    Ok(refreshed)
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum UpgradeOutcome {
    Upgraded {
        slug: String,
        from: String,
        to: String,
        files_changed: usize,
    },
    UpToDate {
        slug: String,
    },
    Conflicts {
        slug: String,
        files: Vec<heron_vcs::ConflictFile>,
    },
    Skipped {
        slug: String,
        reason: String,
    },
}

/// Upgrade one skill to the latest upstream revision.
///
/// `keep_local` selects how local edits are treated, mirroring 设置 → 通用 → 更新时保留本地改动:
/// - `true` (default): three-way merge — the user's local edits and upstream changes are combined;
///   a conflicting file is reported as `Conflicts` and the worktree is left untouched.
/// - `false`: hard overwrite — every local edit (uncommitted *and* already-committed) is discarded
///   in favour of the upstream snapshot, so nothing ever reports a conflict.
pub fn upgrade(ctx: &mut Ctx, slug: &str, keep_local: bool) -> Result<UpgradeOutcome> {
    let dir = ctx.content_path_of(slug);
    if !dir.is_dir() {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    let repo = heron_vcs::SkillRepo::open(&dir);
    let Some(meta) = repo.read_metadata()? else {
        return Ok(UpgradeOutcome::Skipped {
            slug: slug.to_owned(),
            reason: "not_tracked".into(),
        });
    };
    let Some(src) = meta.source.clone() else {
        return Ok(UpgradeOutcome::Skipped {
            slug: slug.to_owned(),
            reason: "no_source".into(),
        });
    };

    let remote = heron_git::RemoteUrl::parse(&src.url)?;
    let mut cached = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote);
    cached.ensure()?;
    let latest = cached.resolve(src.ref_name.as_deref())?.commit;
    let cache_ref = cached.path().to_path_buf();

    if latest == meta.baseline.commit {
        return Ok(UpgradeOutcome::UpToDate {
            slug: slug.to_owned(),
        });
    }

    // 更新时保留本地改动: when on, commit the local edits first so the merge has a stable "ours".
    // When off (overwrite), rewind to the pristine upstream baseline instead — dropping both the
    // uncommitted worktree changes and any committed local revisions — so the three-way merge below
    // has no local contribution left and resolves straight to the upstream snapshot. This reuses
    // the exact merge path a no-local-edits upgrade already takes, keeping the post-merge commit,
    // version tag and event identical between the two modes; only the "ours" side differs.
    let head_before = if keep_local {
        repo.commit_all("heron: local changes before upgrade")?
    } else {
        if let Some(baseline_local) = meta.baseline.local_commit.as_deref() {
            repo.fast_forward_to(baseline_local)?;
        }
        repo.discard_worktree_changes()?;
        String::new()
    };
    // The upstream commit lives in the cache; import it *and the recorded baseline* (a shallow
    // cache may not carry its parents) so git can see both sides of the merge.
    repo.import_objects(cached.path(), &latest)?;
    if !meta.baseline.commit.is_empty() {
        repo.import_objects(cached.path(), &meta.baseline.commit)?;
    }
    let outcome = repo.merge_from_upstream(&latest, &meta.baseline.commit)?;

    match outcome {
        heron_vcs::MergeOutcome::Clean {
            commit,
            files_changed,
            ..
        } => {
            repo.fast_forward_to(&commit)?;
            repo.set_upstream_point(&latest)?;
            let mut new_meta = meta.clone();
            new_meta.baseline.commit = latest.clone();
            // `dir` is the versioned root, which for a whole-repository skill is the repository
            // itself — `SKILL.md` sits at `subpath` inside it.
            new_meta.baseline.version = resolve_version(
                &ctx.skill_dir_of(slug),
                &cache_ref,
                &latest,
                repo_tag_hint(&cached, &latest),
            );
            new_meta.local_revision += 1;
            repo.write_metadata(&new_meta)?;
            let local_head = repo.commit_all(&format!("heron: upgrade to {}", &latest[..8]))?;
            // Tag the local merge commit; the upstream sha stays in the metadata baseline.
            repo.tag_version(&new_meta.baseline.version, &local_head)?;
            ctx.db.write(|tx| {
                event::record(
                    tx,
                    ctx.actor,
                    "upgrade",
                    Some(slug),
                    Some(&format!(
                        "{} -> {}",
                        &meta.baseline.commit[..8],
                        &latest[..8]
                    )),
                    false,
                )?;
                Ok(())
            })?;
            Ok(UpgradeOutcome::Upgraded {
                slug: slug.to_owned(),
                from: meta.baseline.commit,
                to: latest,
                files_changed,
            })
        }
        heron_vcs::MergeOutcome::Conflicts { files } => {
            // Restore the working tree exactly as it was before the attempt.
            if !head_before.is_empty() {
                repo.fast_forward_to(&head_before)?;
            }
            Ok(UpgradeOutcome::Conflicts {
                slug: slug.to_owned(),
                files,
            })
        }
    }
}

/// Roll back to a previously landed version tag.
pub fn rollback(ctx: &mut Ctx, slug: &str, to: &str) -> Result<String> {
    let dir = ctx.content_path_of(slug);
    let repo = heron_vcs::SkillRepo::open(&dir);
    if !repo.is_repo() {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    let tags = repo.list_tags()?;
    let tag = if to.starts_with('v') {
        to.to_owned()
    } else {
        format!("v{to}")
    };
    if !tags.iter().any(|t| t == &tag) {
        return Err(ErrorCode::RefNotFound
            .msg()
            .with("ref", to.to_owned())
            .with("slug", slug.to_owned())
            .into());
    }
    let previous = repo.reset_to(&tag)?;
    ctx.db.write(|tx| {
        event::record(
            tx,
            ctx.actor,
            "rollback",
            Some(slug),
            Some(&format!(
                "{} -> {}",
                &previous[..8.min(previous.len())],
                tag
            )),
            false,
        )?;
        Ok(())
    })?;
    Ok(previous)
}

/// Version of a landed upstream commit: the tag that points at it wins, then
/// \`SKILL.md metadata.version\`, and only then the \`0.0.0+sha\` fallback (design §10.4).
fn resolve_version(
    skill_dir: &std::path::Path,
    cache: &std::path::Path,
    commit: &str,
    tag_hint: Option<String>,
) -> String {
    if let Some(version) = tag_hint {
        return version;
    }
    let skill_md = skill_dir.join("SKILL.md");
    if let Ok(bytes) = std::fs::read(&skill_md) {
        let (frontmatter, _) = heron_scan::parse_skill_md(&bytes);
        if let Some(raw) = frontmatter.metadata_version() {
            let parsed = heron_core::Version::parse(raw, Some(commit));
            if !parsed.is_fallback() {
                return parsed.to_string();
            }
        }
    }
    let _ = cache;
    format!(
        "{}+{}",
        heron_core::Version::ZERO,
        &commit[..7.min(commit.len())]
    )
}

/// A tag such as \`v1.1.0\` that points at \`commit\` in the cache, if any.
fn repo_tag_hint(cached: &heron_git::CachedRepo, commit: &str) -> Option<String> {
    cached
        .tags()
        .ok()?
        .into_iter()
        .find(|t| t.commit == commit)
        .and_then(|t| {
            let parsed = heron_core::Version::parse(&t.name, None);
            (!parsed.is_fallback() && !matches!(parsed, heron_core::Version::Opaque { .. }))
                .then(|| parsed.to_string())
        })
}
