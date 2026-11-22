//! Viewing and resolving differences (the data behind the GUI conflict editor).

use heron_core::{ErrorCode, Result};
use serde::Serialize;

use crate::upgrade::UpgradeOutcome;
use crate::Ctx;

#[derive(Debug, Clone, Serialize)]
pub struct FileDiff {
    pub path: String,
    /// Content at the comparison point (baseline or a version tag).
    pub before: Option<String>,
    /// Content as it exists now.
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    /// `local` (working tree vs baseline) or the tag that was compared against.
    pub against: String,
    pub files: Vec<FileDiff>,
}

/// Diff a managed skill against its upstream baseline or a specific version tag.
pub fn diff_skill(ctx: &Ctx, slug: &str, against: Option<&str>) -> Result<DiffReport> {
    let dir = ctx.content_path_of(slug);
    if !dir.is_dir() {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    let repo = heron_vcs::SkillRepo::open(&dir);
    let meta = repo
        .read_metadata()?
        .ok_or_else(|| ErrorCode::NotManaged.msg().with("slug", slug.to_owned()))?;

    let (label, base_rev) = match against {
        Some(tag) => (tag.to_owned(), tag.to_owned()),
        None => (
            "local".to_owned(),
            meta.baseline
                .local_commit
                .clone()
                .unwrap_or_else(|| meta.baseline.commit.clone()),
        ),
    };

    // Local edits may be uncommitted; compare with the baseline the skill was installed from.
    let mut files = Vec::new();
    for path in repo.changed_since_baseline()? {
        files.push(FileDiff {
            before: repo.read_file_at(&base_rev, &path).ok(),
            after: std::fs::read_to_string(dir.join(&path)).ok(),
            path,
        });
    }
    Ok(DiffReport {
        against: label,
        files,
    })
}

/// The two sides of a conflict plus the upstream commit they come from.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictPayload {
    pub slug: String,
    pub base_commit: String,
    pub upstream_commit: String,
    pub files: Vec<FileDiff>,
}

/// Everything the UI needs to show a conflict: ours and theirs for each file.
///
/// This never touches the working tree: it asks git which files would conflict, then reads both
/// sides out of the object database.
pub fn conflict_payload(ctx: &Ctx, slug: &str) -> Result<ConflictPayload> {
    let dir = ctx.content_path_of(slug);
    let repo = heron_vcs::SkillRepo::open(&dir);
    let meta = repo
        .read_metadata()?
        .ok_or_else(|| ErrorCode::NotManaged.msg().with("slug", slug.to_owned()))?;
    let source = meta.source.as_ref().ok_or_else(|| {
        ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .with("detail", "no_source")
    })?;

    let remote = heron_git::RemoteUrl::parse(&source.url)?;
    let mut cached = heron_git::CachedRepo::new(&ctx.dirs.sources_cache(), remote);
    // Refresh first: a stale cache would compare against an old upstream revision.
    cached.ensure()?;
    let upstream = cached.resolve(source.ref_name.as_deref())?.commit;

    // Uncommitted edits are part of "ours": commit them first so the conflict set matches what
    // `upgrade` would actually encounter.
    repo.commit_all("heron: local changes before conflict review")?;
    let ours = repo.head()?;
    repo.import_objects(cached.path(), &upstream).ok();
    repo.import_objects(cached.path(), &meta.baseline.commit)
        .ok();

    let paths = match repo.merge_from_upstream(&upstream, &meta.baseline.commit)? {
        heron_vcs::MergeOutcome::Conflicts { files } => {
            files.into_iter().map(|f| f.path).collect::<Vec<_>>()
        }
        heron_vcs::MergeOutcome::Clean { .. } => {
            return Err(ErrorCode::Internal
                .msg()
                .with("detail", "no_conflict_to_resolve")
                .into());
        }
    };

    // "ours" lives in the skill repository; "theirs" only exists in the cache.
    let files = paths
        .into_iter()
        .map(|path| FileDiff {
            before: repo.read_file_at(&ours, &path).ok(),
            after: cached
                .read_blob(
                    &upstream,
                    &format!("{}/{}", source.subpath.clone().unwrap_or_default(), path),
                )
                .ok()
                .or_else(|| cached.read_blob(&upstream, &path).ok())
                .or_else(|| repo.read_file_at(&upstream, &path).ok()),
            path,
        })
        .collect();

    Ok(ConflictPayload {
        slug: slug.to_owned(),
        base_commit: meta.baseline.commit,
        upstream_commit: upstream,
        files,
    })
}

/// Write the user's resolutions, commit them, and retry the upgrade.
pub fn resolve(ctx: &mut Ctx, slug: &str, resolved: &[(String, String)]) -> Result<UpgradeOutcome> {
    let dir = ctx.content_path_of(slug);
    let repo = heron_vcs::SkillRepo::open(&dir);
    if !repo.is_repo() {
        return Err(ErrorCode::NotManaged
            .msg()
            .with("slug", slug.to_owned())
            .into());
    }
    for (path, content) in resolved {
        repo.write_file(path, content)?;
    }
    repo.commit_all("heron: manual conflict resolution")?;
    // The resolution is now "ours": retrying the upgrade merges upstream on top of it. Keep-local
    // is forced on here — a manual conflict resolution only exists in the merge mode, so the retry
    // must preserve the resolved content rather than overwrite it.
    crate::upgrade::upgrade(ctx, slug, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_diff_serialises_both_sides() {
        let diff = FileDiff {
            path: "SKILL.md".into(),
            before: Some("a".into()),
            after: None,
        };
        let json = serde_json::to_value(&diff).unwrap();
        assert_eq!(json["path"], "SKILL.md");
        assert_eq!(json["before"], "a");
        assert!(json["after"].is_null());
    }
}
