//! A managed skill's git repository.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Result};
use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::merge::{merge_tree, MergeOutcome};

pub const METADATA_FILE: &str = "heron.skill.json";
pub const BRANCH_UPSTREAM: &str = "upstream";
pub const BRANCH_MAIN: &str = "main";
pub const ROLLBACK_BACKUP_PREFIX: &str = "heron-rollback-backup-";

/// Committed metadata used as the three-way merge baseline (implementation doc §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMeta {
    pub schema: u32,
    pub slug: String,
    /// `upstream` | `local` | `legacy-command`
    pub origin: String,
    pub source: Option<MetaSource>,
    pub baseline: Baseline,
    pub local_revision: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaSource {
    pub url: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub commit: String,
    pub version: String,
}

impl SkillMeta {
    pub fn new(slug: &str, origin: &str, commit: &str, version: &str) -> Self {
        Self {
            schema: 1,
            slug: slug.to_owned(),
            origin: origin.to_owned(),
            source: None,
            baseline: Baseline {
                commit: commit.to_owned(),
                version: version.to_owned(),
            },
            local_revision: 0,
            created_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitInfo {
    pub sha: String,
    pub summary: String,
    pub author: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct SkillRepo {
    root: PathBuf,
    git: Git,
}

impl SkillRepo {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            git: Git::default_binary(),
        }
    }

    pub fn open_with(root: impl Into<PathBuf>, git: Git) -> Self {
        Self {
            root: root.into(),
            git,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_repo(&self) -> bool {
        self.root.join(".git").is_dir()
    }

    /// Initialise a repository for a freshly adopted skill.
    ///
    /// `upstream` is created as a branch pointing at the initial commit, so later merges
    /// always have a base even before the first remote fetch.
    pub fn init(&self, meta: &SkillMeta) -> Result<()> {
        let root = &self.root;
        self.git.run_checked(
            root,
            &["init", "-q", "-b", BRANCH_MAIN],
            ErrorCode::Internal,
        )?;
        // Local config: never sign, never run user hooks, ignore the user's global settings.
        for (key, value) in [
            ("user.name", "Heron"),
            ("user.email", "heron@localhost"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", ""),
        ] {
            self.git
                .run_checked(root, &["config", key, value], ErrorCode::Internal)?;
        }
        self.write_metadata(meta)?;
        self.write_gitignore()?;
        let sha = self.commit_all(&format!("heron: adopt {}", meta.slug))?;
        self.git.run_checked(
            root,
            &["branch", BRANCH_UPSTREAM, &sha],
            ErrorCode::Internal,
        )?;
        Ok(())
    }

    /// `.gitignore` keeps OS noise out of the history but leaves skill content tracked.
    fn write_gitignore(&self) -> Result<()> {
        let content = ".DS_Store\nThumbs.db\n.heron/\n";
        heron_platform::atomic::write_atomic(&self.root.join(".gitignore"), content.as_bytes())?;
        Ok(())
    }

    /// Persist `heron.skill.json` (committed with the next `commit_all`).
    pub fn write_metadata(&self, meta: &SkillMeta) -> Result<()> {
        let mut meta = meta.clone();
        if meta.created_at.is_empty() {
            meta.created_at = now();
        }
        let json = serde_json::to_vec_pretty(&meta)
            .map_err(|e| ErrorCode::Internal.msg().with("detail", e.to_string()))?;
        heron_platform::atomic::write_atomic(&self.root.join(METADATA_FILE), &json)?;
        Ok(())
    }

    pub fn read_metadata(&self) -> Result<Option<SkillMeta>> {
        let path = self.root.join(METADATA_FILE);
        if !path.is_file() {
            return Ok(None);
        }
        let text = heron_platform::atomic::read_to_string_lenient(&path)?;
        let meta = serde_json::from_str(&text).map_err(|e| {
            ErrorCode::Internal
                .msg()
                .with("detail", format!("{METADATA_FILE}: {e}"))
        })?;
        Ok(Some(meta))
    }

    /// Stage everything and commit. Returns the new commit sha. No-op commits are avoided.
    pub fn commit_all(&self, message: &str) -> Result<String> {
        self.git
            .run_checked(&self.root, &["add", "-A"], ErrorCode::Internal)?;
        let staged = self
            .git
            .run(&self.root, &["diff", "--cached", "--quiet"])?
            .ok();
        if staged {
            // Nothing staged: return the current HEAD so callers always get a rev.
            return self.head();
        }
        let out = self.git.run_checked(
            &self.root,
            &["commit", "-q", "--no-verify", "-m", message],
            ErrorCode::Internal,
        );
        match out {
            Ok(_) => self.head(),
            Err(e) => {
                // A repository with no commits yet and nothing staged is not an error.
                if self.head_raw().is_none() {
                    Ok(String::new())
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn head(&self) -> Result<String> {
        self.head_raw().ok_or_else(|| {
            heron_core::HeronError::from(
                ErrorCode::Internal
                    .msg()
                    .with("detail", "repo has no commits"),
            )
        })
    }

    fn head_raw(&self) -> Option<String> {
        let out = self.git.run(&self.root, &["rev-parse", "HEAD"]).ok()?;
        if out.ok() {
            Some(out.stdout.trim().to_owned())
        } else {
            None
        }
    }

    pub fn rev_parse(&self, rev: &str) -> Result<String> {
        Ok(self
            .git
            .run_checked(&self.root, &["rev-parse", rev], ErrorCode::RefNotFound)?
            .trim()
            .to_owned())
    }

    /// Move the `upstream` branch to `commit`.
    pub fn set_upstream_point(&self, commit: &str) -> Result<()> {
        self.git.run_checked(
            &self.root,
            &["branch", "-f", BRANCH_UPSTREAM, commit],
            ErrorCode::Internal,
        )?;
        Ok(())
    }

    pub fn tag_version(&self, version: &str, commit: &str) -> Result<()> {
        let tag = format!("v{}", version.trim_start_matches('v'));
        self.git.run_checked(
            &self.root,
            &["tag", "-f", &tag, commit],
            ErrorCode::Internal,
        )?;
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<String>> {
        Ok(self
            .git
            .run_checked(&self.root, &["tag", "--list"], ErrorCode::Internal)?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }

    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let format = "%H%x1f%s%x1f%an%x1f%aI";
        let out = self.git.run_checked(
            &self.root,
            &["log", &format!("-n{limit}"), &format!("--format={format}")],
            ErrorCode::Internal,
        )?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\x1f');
                Some(CommitInfo {
                    sha: parts.next()?.to_owned(),
                    summary: parts.next()?.to_owned(),
                    author: parts.next()?.to_owned(),
                    timestamp: parts.next()?.to_owned(),
                })
            })
            .collect())
    }

    /// Files changed on `main` relative to the metadata baseline.
    pub fn changed_since_baseline(&self) -> Result<Vec<String>> {
        let Some(meta) = self.read_metadata()? else {
            return Ok(vec![]);
        };
        if meta.baseline.commit.is_empty() {
            return Ok(vec![]);
        }
        let range = format!("{}..HEAD", meta.baseline.commit);
        let out = self.git.run(&self.root, &["diff", "--name-only", &range])?;
        if !out.ok() {
            return Ok(vec![]);
        }
        Ok(out
            .stdout
            .lines()
            .map(str::to_owned)
            .filter(|l| !l.is_empty())
            .collect())
    }

    pub fn has_local_changes(&self) -> Result<bool> {
        Ok(!self.changed_since_baseline()?.is_empty())
    }

    /// Import objects from another repository (usually the bare cache) so commits fetched
    /// from upstream can be merged locally. Equivalent to `git fetch <path> <sha>`.
    pub fn import_objects(&self, from: &Path, rev: &str) -> Result<()> {
        let out = self.git.run(
            &self.root,
            &[
                "fetch",
                "-q",
                "--no-tags",
                from.to_str().unwrap_or_default(),
                rev,
            ],
        )?;
        if !out.ok() {
            return Err(heron_core::HeronError::from(
                ErrorCode::RefNotFound
                    .msg()
                    .with("ref", rev.to_owned())
                    .with("detail", first_line(&out.stderr)),
            ));
        }
        Ok(())
    }

    /// True when the working tree has changes that are not committed yet.
    pub fn worktree_differs_from_head(&self) -> Result<bool> {
        if self.head_raw().is_none() {
            return Ok(false);
        }
        let out = self.git.run(&self.root, &["status", "--porcelain"])?;
        Ok(out.ok() && !out.stdout.trim().is_empty())
    }

    /// Three-way merge `theirs` on top of the current working tree.
    ///
    /// The working tree is **not** touched when there are conflicts: the caller decides.
    /// `base` is the commit recorded as the merge baseline; both `base` and `theirs` must
    /// already exist in this repository (see [`Self::import_objects`]).
    pub fn merge_from_upstream(&self, theirs: &str, base: &str) -> Result<MergeOutcome> {
        let ours = self.head()?;
        if !self.git.supports_merge_tree(&self.root) {
            return Err(heron_core::HeronError::from(
                ErrorCode::GitUnsupported
                    .msg()
                    .with("detail", "merge-tree requires git >= 2.38"),
            ));
        }
        // `--merge-base` takes a tree, not a commit.
        let base_tree = self.rev_parse(&format!("{base}^{{tree}}"))?;
        merge_tree(&self.git, &self.root, &base_tree, &ours, theirs)
    }

    /// Apply a clean merge commit produced by [`Self::merge_from_upstream`].
    pub fn fast_forward_to(&self, commit: &str) -> Result<()> {
        self.git.run_checked(
            &self.root,
            &["reset", "-q", "--hard", commit],
            ErrorCode::Internal,
        )?;
        Ok(())
    }

    /// Reset to `rev`, tagging the previous HEAD so the move itself can be reverted.
    pub fn reset_to(&self, rev: &str) -> Result<String> {
        let previous = self.head()?;
        self.git.run_checked(
            &self.root,
            &[
                "tag",
                "-f",
                &format!("{ROLLBACK_BACKUP_PREFIX}{}", short(&previous)),
                &previous,
            ],
            ErrorCode::Internal,
        )?;
        let target = self.rev_parse(rev)?;
        self.git.run_checked(
            &self.root,
            &["reset", "-q", "--hard", &target],
            ErrorCode::Internal,
        )?;
        Ok(previous)
    }

    /// Revert an uncommitted mess after a failed operation.
    pub fn discard_worktree_changes(&self) -> Result<()> {
        if self.head_raw().is_none() {
            return Ok(());
        }
        self.git.run_checked(
            &self.root,
            &["reset", "-q", "--hard", "HEAD"],
            ErrorCode::Internal,
        )?;
        self.git
            .run_checked(&self.root, &["clean", "-q", "-fd"], ErrorCode::Internal)?;
        Ok(())
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn now() -> String {
    chrono_like()
}

/// RFC 3339 UTC timestamp without pulling a date library into this crate.
fn chrono_like() -> String {
    heron_platform::time::now_rfc3339()
}
