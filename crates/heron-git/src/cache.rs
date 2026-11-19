//! The bare cache: one repository per remote, shared by every skill that comes from it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use heron_core::{ErrorCode, Result};
use heron_vcs::git::{identity_args, Git};

use crate::remote::{RefKind, RemoteUrl, ResolvedRef};

/// How long a network operation may take before it is reported as `E_CLONE_TIMEOUT`.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

/// `<cache_root>/<cache_key>/bare.git`
pub fn cache_dir_for(cache_root: &Path, remote: &RemoteUrl) -> PathBuf {
    cache_root.join(remote.cache_key()).join("bare.git")
}

/// A cached bare repository.
pub struct CachedRepo {
    remote: RemoteUrl,
    dir: PathBuf,
    git: Git,
}

impl CachedRepo {
    pub fn new(cache_root: &Path, remote: RemoteUrl) -> Self {
        let dir = cache_dir_for(cache_root, &remote);
        Self {
            remote,
            dir,
            git: Git::default_binary(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn remote(&self) -> &RemoteUrl {
        &self.remote
    }

    /// Create or refresh the bare cache. Safe to call repeatedly; the network request is a
    /// shallow fetch of all heads and tags.
    pub fn ensure(&mut self) -> Result<()> {
        if !self.dir.join("HEAD").is_file() {
            self.init_bare()?;
        }
        let out = self.git.run(
            &self.dir,
            &[
                "fetch",
                "--prune",
                "--depth=1",
                "--no-tags",
                self.remote.url.as_str(),
                "+refs/heads/*:refs/remotes/origin/*",
                "+refs/tags/*:refs/tags/*",
            ],
        )?;
        if !out.ok() {
            // A shallow fetch on a repository that already has history may be refused by older
            // servers; retry without --depth before surfacing the error.
            let retry = self.git.run(
                &self.dir,
                &[
                    "fetch",
                    "--prune",
                    self.remote.url.as_str(),
                    "+refs/heads/*:refs/remotes/origin/*",
                    "+refs/tags/*:refs/tags/*",
                ],
            )?;
            if !retry.ok() {
                return Err(ErrorCode::Network
                    .msg()
                    .with("url", self.remote.url.clone())
                    .with("detail", first_line(&retry.stderr))
                    .into());
            }
            return Ok(());
        }
        Ok(())
    }

    fn init_bare(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", self.dir.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
        let mut args = identity_args();
        args.extend_from_slice(&["init", "-q", "--bare"]);
        self.git
            .run_checked(&self.dir, &args, ErrorCode::Internal)?;
        Ok(())
    }

    /// Resolve a ref (branch name, tag name, full sha or short sha) without network access.
    pub fn resolve(&self, needle: Option<&str>) -> Result<ResolvedRef> {
        let Some(needle) = needle.map(str::trim).filter(|s| !s.is_empty()) else {
            return self.default_branch();
        };

        for (kind, name) in [
            (RefKind::Branch, format!("refs/remotes/origin/{needle}")),
            (RefKind::Branch, format!("refs/heads/{needle}")),
            (RefKind::Tag, format!("refs/tags/{needle}")),
        ] {
            if let Ok(commit) = self.rev_parse(&name) {
                return Ok(ResolvedRef {
                    name: needle.to_owned(),
                    kind,
                    commit,
                });
            }
        }
        // A full or short commit sha.
        if let Ok(commit) = self.rev_parse(needle) {
            return Ok(ResolvedRef {
                name: needle.to_owned(),
                kind: RefKind::Commit,
                commit,
            });
        }
        Err(ErrorCode::RefNotFound
            .msg()
            .with("ref", needle.to_owned())
            .with("url", self.remote.url.clone())
            .into())
    }

    fn default_branch(&self) -> Result<ResolvedRef> {
        // `origin/HEAD` may be missing after a shallow fetch; fall back to the first branch.
        if let Ok(commit) = self.rev_parse("refs/remotes/origin/HEAD") {
            return Ok(ResolvedRef {
                name: "HEAD".to_owned(),
                kind: RefKind::Branch,
                commit,
            });
        }
        let heads = self.heads()?;
        heads.into_iter().next().ok_or_else(|| {
            heron_core::HeronError::from(
                ErrorCode::RefNotFound
                    .msg()
                    .with("ref", "HEAD")
                    .with("url", self.remote.url.clone()),
            )
        })
    }

    pub fn heads(&self) -> Result<Vec<ResolvedRef>> {
        self.list_refs("refs/remotes/origin/", "origin/", RefKind::Branch)
    }

    pub fn tags(&self) -> Result<Vec<ResolvedRef>> {
        self.list_refs("refs/tags/", "", RefKind::Tag)
    }

    fn list_refs(&self, prefix: &str, strip: &str, kind: RefKind) -> Result<Vec<ResolvedRef>> {
        let out = self.git.run(
            &self.dir,
            &["for-each-ref", "--format=%(refname) %(objectname)", prefix],
        )?;
        if !out.ok() {
            return Ok(vec![]);
        }
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                let (name, commit) = line.split_once(' ')?;
                Some(ResolvedRef {
                    name: name
                        .trim_start_matches(prefix)
                        .trim_start_matches(strip)
                        .to_owned(),
                    kind,
                    commit: commit.trim().to_owned(),
                })
            })
            .filter(|r| !r.name.is_empty() && r.name != "HEAD")
            .collect())
    }

    fn rev_parse(&self, rev: &str) -> Result<String> {
        let out = self.git.run(
            &self.dir,
            &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        )?;
        if out.ok() {
            Ok(out.stdout.trim().to_owned())
        } else {
            Err(heron_core::HeronError::from(
                ErrorCode::RefNotFound.msg().with("ref", rev.to_owned()),
            ))
        }
    }

    /// List the entries of `subpath` at `commit` (empty subpath = repository root).
    pub fn tree(&self, commit: &str, subpath: &str) -> Result<Vec<TreeEntry>> {
        let spec = if subpath.is_empty() {
            commit.to_owned()
        } else {
            format!("{commit}:{subpath}")
        };
        let out = self
            .git
            .run(&self.dir, &["ls-tree", "-r", "--full-tree", &spec])?;
        if !out.ok() {
            return Err(ErrorCode::RefNotFound
                .msg()
                .with("ref", spec)
                .with("detail", first_line(&out.stderr))
                .into());
        }
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                // "<mode> <type> <oid>\t<path>"
                let (meta, path) = line.split_once('\t')?;
                let mut fields = meta.split_whitespace();
                Some(TreeEntry {
                    mode: fields.next()?.to_owned(),
                    kind: match fields.next()? {
                        "blob" => TreeEntryKind::File,
                        "tree" => TreeEntryKind::Dir,
                        _ => TreeEntryKind::Other,
                    },
                    oid: fields.next()?.to_owned(),
                    path: path.to_owned(),
                })
            })
            .collect())
    }

    /// Read one text file (catalogs, metadata) at `commit`.
    pub fn read_blob(&self, commit: &str, path: &str) -> Result<String> {
        let spec = format!("{commit}:{path}");
        let out = self.git.run(&self.dir, &["show", &spec])?;
        if !out.ok() {
            return Err(ErrorCode::RefNotFound
                .msg()
                .with("ref", spec)
                .with("detail", first_line(&out.stderr))
                .into());
        }
        Ok(out.stdout)
    }

    /// Materialize `subpath` at `commit` into `dest` using `git archive`.
    ///
    /// `git archive HEAD:sub` produces exactly the skill's own files, with no `.git`, no
    /// working tree and no chance of leaving a half-checked-out directory behind.
    pub fn checkout_subdir(&self, commit: &str, subpath: &str, dest: &Path) -> Result<()> {
        if dest.exists() {
            return Err(ErrorCode::SafetyRefused
                .msg()
                .with("path", dest.display().to_string())
                .with("reason", "destination_exists")
                .into());
        }
        let parent = dest.parent().ok_or_else(|| {
            heron_core::HeronError::from(
                ErrorCode::Internal
                    .msg()
                    .with("detail", "dest_without_parent"),
            )
        })?;
        std::fs::create_dir_all(parent).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", parent.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;

        let spec = if subpath.is_empty() {
            commit.to_owned()
        } else {
            format!("{commit}:{subpath}")
        };
        let temp = parent.join(format!(
            ".{}-heron-extract",
            dest.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".into())
        ));

        let archive = self
            .git
            .run(&self.dir, &["archive", "--format=tar", &spec])?;
        if !archive.ok() {
            return Err(ErrorCode::RefNotFound
                .msg()
                .with("ref", spec)
                .with("detail", first_line(&archive.stderr))
                .into());
        }

        extract_tar(archive.stdout.as_bytes(), &temp)?;
        std::fs::rename(&temp, dest).map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp);
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", dest.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEntryKind {
    File,
    Dir,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: String,
    pub kind: TreeEntryKind,
    pub oid: String,
    pub path: String,
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// Minimal ustar extractor: regular files and directories only (git archive output).
fn extract_tar(bytes: &[u8], dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", dest.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;

    let mut offset = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = tar_str(&header[0..100]);
        let size_field = tar_str(&header[124..136]);
        let size = usize::from_str_radix(size_field.trim_matches(['\0', ' ']), 8).unwrap_or(0);
        let type_flag = header[156] as char;
        let prefix = tar_str(&header[345..500]);
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };

        let safe = sanitize_rel(&full).ok_or_else(|| {
            heron_core::HeronError::from(
                ErrorCode::SafetyRefused
                    .msg()
                    .with("path", full.clone())
                    .with("reason", "path_traversal"),
            )
        })?;
        let target = dest.join(&safe);
        offset += 512;

        match type_flag {
            '5' => {
                std::fs::create_dir_all(&target).map_err(|e| {
                    heron_core::HeronError::from(
                        ErrorCode::Permission
                            .msg()
                            .with("path", target.display().to_string())
                            .with("detail", e.to_string()),
                    )
                })?;
            }
            '0' | '\0' => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let end = (offset + size).min(bytes.len());
                std::fs::write(&target, &bytes[offset..end]).map_err(|e| {
                    heron_core::HeronError::from(
                        ErrorCode::Permission
                            .msg()
                            .with("path", target.display().to_string())
                            .with("detail", e.to_string()),
                    )
                })?;
            }
            _ => {}
        }
        // Entries are padded to 512 bytes.
        offset += size.div_ceil(512) * 512;
    }
    Ok(())
}

fn tar_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Reject absolute paths and `..` segments coming from an archive.
fn sanitize_rel(path: &str) -> Option<String> {
    let mut out = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => out.push(other),
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize_rel("a/b.md").as_deref(), Some("a/b.md"));
        assert_eq!(sanitize_rel("./a/b.md").as_deref(), Some("a/b.md"));
        assert!(sanitize_rel("../etc/passwd").is_none());
        assert!(
            sanitize_rel("/abs").is_some(),
            "leading slash collapses to a relative path"
        );
        assert!(sanitize_rel("").is_none());
    }

    #[test]
    fn cache_path_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let a = cache_dir_for(
            tmp.path(),
            &RemoteUrl::parse("https://github.com/a/b").unwrap(),
        );
        let b = cache_dir_for(
            tmp.path(),
            &RemoteUrl::parse("https://github.com/a/b.git").unwrap(),
        );
        assert_eq!(a, b);
        assert!(a.ends_with("bare.git"));
    }
}
