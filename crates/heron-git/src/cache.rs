//! The bare cache: one repository per remote, shared by every skill that comes from it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use heron_core::{ErrorCode, HeronError, Result};
use heron_vcs::git::{identity_args, Git, RemoteRun, NETWORK_TIMEOUT};

use crate::remote::{RefKind, RemoteUrl, ResolvedRef};

/// Refspecs for one fetch. `+HEAD:refs/remotes/origin/HEAD` is what makes the remote's default
/// branch visible: `git fetch` never creates that symref on its own, so without it a repository
/// carrying extra branches (CI and auto-commit branches are common) resolves to an arbitrary head.
const FETCH_REFSPECS: [&str; 2] = [
    "+refs/heads/*:refs/remotes/origin/*",
    "+HEAD:refs/remotes/origin/HEAD",
];

/// Recent history, shallow enough to fetch in seconds: three-way merges need a real ancestry,
/// while a full clone would cost minutes on large skill repositories.
const FETCH_DEPTH: &str = "--depth=200";

/// Branch names that mean "the default" when the remote never advertised `HEAD`. Ordered.
const DEFAULT_BRANCH_CANDIDATES: [&str; 3] = ["main", "master", "develop"];

/// Only these failures are specific to a shallow fetch: retrying a timeout or an auth error
/// without `--depth` would just spend the same budget twice. (`unshallow` contains `shallow`.)
const UNSHALLOW_MARKERS: [&str; 2] = ["shallow", "Server does not allow"];

/// `<cache_root>/<cache_key>/bare.git`
pub fn cache_dir_for(cache_root: &Path, remote: &RemoteUrl) -> PathBuf {
    cache_root.join(remote.cache_key()).join("bare.git")
}

/// A cached bare repository.
pub struct CachedRepo {
    remote: RemoteUrl,
    dir: PathBuf,
    git: Git,
    timeout: Duration,
}

impl CachedRepo {
    pub fn new(cache_root: &Path, remote: RemoteUrl) -> Self {
        Self::with_git(cache_root, remote, Git::default_binary())
    }

    /// The git program is injectable so the failure modes of [`Self::ensure`] can be tested
    /// without reaching a network.
    pub fn with_git(cache_root: &Path, remote: RemoteUrl, git: Git) -> Self {
        let dir = cache_dir_for(cache_root, &remote);
        Self {
            remote,
            dir,
            git,
            timeout: NETWORK_TIMEOUT,
        }
    }

    /// Overrides the per-attempt budget; only the tests need a short one.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn remote(&self) -> &RemoteUrl {
        &self.remote
    }

    /// Create or refresh the bare cache. Safe to call repeatedly; the network request is a
    /// shallow fetch of all heads, tags and the remote's default branch.
    pub fn ensure(&mut self) -> Result<()> {
        if !self.dir.join("HEAD").is_file() {
            self.init_bare()?;
        }
        let first = self.fetch(&[FETCH_DEPTH])?;
        if first.ok {
            return Ok(());
        }
        if !needs_unshallow(&first.stderr) {
            return Err(self.failure(&first));
        }
        // A shallow fetch on a repository that already has history can be refused by older
        // servers; only that refusal is worth paying for a second, un-shallow attempt.
        let retry = self.fetch(&[])?;
        if retry.ok {
            Ok(())
        } else {
            Err(self.failure(&retry))
        }
    }

    fn fetch(&self, extra: &[&str]) -> Result<RemoteRun> {
        let mut args: Vec<&str> = vec!["fetch", "--prune", "--tags"];
        args.extend_from_slice(extra);
        args.push(self.remote.url.as_str());
        args.extend_from_slice(&FETCH_REFSPECS);
        self.git.run_network(&self.dir, &args, self.timeout)
    }

    /// A budget that ran out is a different diagnosis from a refused connection: the first says
    /// "your network or proxy", the second says "this remote".
    fn failure(&self, run: &RemoteRun) -> HeronError {
        let detail = if run.timed_out {
            format!(
                "timed out after {}s without a usable route; check the network or set a proxy in Settings",
                self.timeout.as_secs().max(1)
            )
        } else {
            first_line(&run.stderr)
        };
        let code = if run.timed_out {
            ErrorCode::CloneTimeout
        } else {
            ErrorCode::Network
        };
        code.msg()
            .with("url", self.remote.url.clone())
            .with("detail", detail)
            .into()
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
        if let Ok(commit) = self.rev_parse("refs/remotes/origin/HEAD") {
            return Ok(ResolvedRef {
                name: "HEAD".to_owned(),
                kind: RefKind::Branch,
                commit,
            });
        }
        // Older git and servers that refuse the HEAD refspec leave no origin/HEAD; then the
        // conventional branch name beats the first entry of a lexicographically sorted list.
        let heads = self.heads()?;
        pick_default(&heads).cloned().ok_or_else(|| {
            HeronError::from(
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
        let tarball = parent.join(format!(
            ".{}-heron.tar",
            dest.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".into())
        ));

        // A tar stream is binary, and [`Git::run`] hands back text: every byte that is not valid
        // UTF-8 would be replaced and the archive would not parse. Letting git write the file
        // itself keeps the bytes intact, which matters as soon as a skill ships images or fonts.
        let archive = self.git.run(
            &self.dir,
            &[
                "archive",
                "--format=tar",
                "-o",
                &tarball.display().to_string(),
                &spec,
            ],
        )?;
        if !archive.ok() {
            let _ = std::fs::remove_file(&tarball);
            return Err(ErrorCode::RefNotFound
                .msg()
                .with("ref", spec)
                .with("detail", first_line(&archive.stderr))
                .into());
        }

        let extracted = std::fs::read(&tarball)
            .map_err(HeronError::from)
            .and_then(|bytes| extract_tar(&bytes, &temp));
        let _ = std::fs::remove_file(&tarball);
        extracted?;
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

/// Whether git complained about the *shallow* part of the fetch specifically.
fn needs_unshallow(stderr: &str) -> bool {
    UNSHALLOW_MARKERS
        .iter()
        .any(|marker| stderr.contains(marker))
}

/// The default branch among `heads`, preferring the conventional names over whatever
/// `for-each-ref` happens to list first.
fn pick_default(heads: &[ResolvedRef]) -> Option<&ResolvedRef> {
    DEFAULT_BRANCH_CANDIDATES
        .iter()
        .find_map(|want| heads.iter().find(|head| &head.name == want))
        .or_else(|| heads.first())
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

    #[test]
    fn only_a_shallow_refusal_justifies_the_unshallowed_retry() {
        assert!(needs_unshallow(
            "fatal: git upload-pack: Server does not allow request for unadvertised object"
        ));
        assert!(needs_unshallow(
            "error: shallow file has changed since we read it"
        ));
        assert!(!needs_unshallow(
            "fatal: unable to access: Failed to connect to 127.0.0.1 port 1"
        ));
        assert!(!needs_unshallow(
            "error: RPC failed; curl 56 Recv failure: Connection reset"
        ));
        assert!(!needs_unshallow(""));
    }

    #[test]
    fn default_branch_prefers_the_conventional_name() {
        let head = |name: &str| ResolvedRef {
            name: name.to_owned(),
            kind: RefKind::Branch,
            commit: format!("{}-commit", name.len()),
        };
        // A repository carrying CI branches, where "first listed" used to win by accident.
        let heads = [
            head("auto-optimize/20260420"),
            head("master"),
            head("releases/2026-05-07"),
        ];
        assert_eq!(pick_default(&heads).unwrap().name, "master");
        let heads = [head("auto-optimize/x"), head("main"), head("master")];
        assert_eq!(pick_default(&heads).unwrap().name, "main");
        let heads = [head("trunk"), head("beta")];
        assert_eq!(pick_default(&heads).unwrap().name, "trunk");
        assert!(pick_default(&[]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_stalled_fetch_is_reported_as_a_clone_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        // A stand-in for a route that black-holes the transfer: it never writes, never exits.
        let stub = tmp.path().join("stalled-git");
        std::fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        let remote = RemoteUrl::parse("https://github.com/a/b").unwrap();
        let dir = cache_dir_for(tmp.path(), &remote);
        std::fs::create_dir_all(&dir).unwrap();
        // Present HEAD means ensure() goes straight to the fetch.
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let mut repo =
            CachedRepo::with_git(tmp.path(), remote, Git::new(stub.display().to_string()))
                .with_timeout(Duration::from_millis(200));
        let err = repo.ensure().unwrap_err();
        assert_eq!(err.code(), "E_CLONE_TIMEOUT");
        let detail = err
            .as_msg()
            .params
            .get("detail")
            .cloned()
            .unwrap_or_default();
        assert!(detail.contains("proxy"), "{detail}");
    }
}
