//! Directory links: the single entry point for creating, probing and removing
//! projections (design §10.2, risk R3 — the highest-severity risk in the product).
//!
//! Invariants enforced here:
//! 1. We never recurse into a link when deleting it. On Windows a junction is removed with
//!    `RemoveDirectoryW`, which only detaches the reparse point; `remove_dir_all` on a
//!    junction can delete the *target's* content and is therefore banned.
//! 2. We never delete a real directory through the link API: [`remove_link`] refuses with
//!    `E_SAFETY_REFUSED` so that a bug upstream cannot destroy user data.
//! 3. Creation is idempotent: linking to the same target twice reports [`LinkOutcome::AlreadyPresent`].

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Msg, ProjectionMode};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
use windows as imp;

/// What a path actually is, as reported by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Directory symbolic link (`IO_REPARSE_TAG_SYMLINK` on Windows).
    SymbolicDir,
    /// NTFS directory junction (`IO_REPARSE_TAG_MOUNT_POINT` on Windows). Never produced on unix.
    Junction,
    /// File symbolic link.
    SymbolicFile,
    /// A regular directory.
    Directory,
    /// A regular file.
    File,
    /// Nothing at this path.
    Missing,
}

impl LinkKind {
    pub fn is_link(self) -> bool {
        matches!(
            self,
            LinkKind::SymbolicDir | LinkKind::Junction | LinkKind::SymbolicFile
        )
    }

    pub fn is_real_dir(self) -> bool {
        matches!(self, LinkKind::Directory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInfo {
    pub kind: LinkKind,
    /// Target recorded in the link itself (may not exist).
    pub target: Option<PathBuf>,
    /// Whether the target currently exists.
    pub resolved_exists: bool,
}

impl LinkInfo {
    pub fn missing() -> Self {
        Self {
            kind: LinkKind::Missing,
            target: None,
            resolved_exists: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutcome {
    /// The link (or copy-ready directory) already pointed where we wanted.
    AlreadyPresent,
    Created(LinkKind),
    /// The caller asked for a symbolic link, the OS refused, and we used a junction.
    Downgraded {
        from: LinkKind,
        to: LinkKind,
        warning: Msg,
    },
}

/// Inspect `path` without following the link.
pub fn probe(path: &Path) -> Result<LinkInfo, Msg> {
    imp::probe(path)
}

/// Create `link` pointing at `target`.
///
/// * `ProjectionMode::Symlink` — symbolic link; on Windows, falls back to a junction when
///   the process lacks `SeCreateSymbolicLinkPrivilege`.
/// * `ProjectionMode::Junction` — junction on Windows, symbolic link elsewhere.
/// * `ProjectionMode::Copy` / `Pointer` — rejected here; those are materialized by the
///   projection crate, which owns copying and pointer generation.
pub fn create_dir_link(
    target: &Path,
    link: &Path,
    mode: ProjectionMode,
) -> Result<LinkOutcome, Msg> {
    if !target.is_dir() {
        return Err(ErrorCode::LinkTargetMissing
            .msg()
            .with("target", target.display().to_string()));
    }
    match mode {
        ProjectionMode::Symlink | ProjectionMode::Junction => {}
        ProjectionMode::Copy | ProjectionMode::Pointer => {
            return Err(ErrorCode::Internal
                .msg()
                .with("detail", "create_dir_link_only_handles_links"));
        }
    }

    match probe(link)? {
        LinkInfo {
            kind: LinkKind::Missing,
            ..
        } => {}
        LinkInfo {
            kind,
            target: Some(existing),
            ..
        } if kind.is_link() && same_path(&existing, target) => {
            return Ok(LinkOutcome::AlreadyPresent);
        }
        LinkInfo { kind, .. } if kind.is_link() => {
            // Points somewhere else: detach first (never recursive).
            remove_link(link)?;
        }
        LinkInfo { .. } => {
            return Err(ErrorCode::SafetyRefused
                .msg()
                .with("path", link.display().to_string())
                .with("reason", "exists_and_is_not_a_link"));
        }
    }

    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(crate::dirs::io_msg(parent))?;
    }
    imp::create(target, link, mode)
}

/// Detach a link. Refuses to touch anything that is not a link.
pub fn remove_link(path: &Path) -> Result<(), Msg> {
    let info = probe(path)?;
    match info.kind {
        LinkKind::Missing => Ok(()),
        k if k.is_link() => imp::unlink(path, k),
        _ => Err(ErrorCode::SafetyRefused
            .msg()
            .with("path", path.display().to_string())
            .with("reason", "not_a_link")),
    }
}

/// Point an existing link at a new target.
pub fn relink(path: &Path, new_target: &Path) -> Result<LinkOutcome, Msg> {
    let previous = probe(path)?;
    if !previous.kind.is_link() {
        return Err(ErrorCode::SafetyRefused
            .msg()
            .with("path", path.display().to_string())
            .with("reason", "not_a_link"));
    }
    remove_link(path)?;
    let mode = match previous.kind {
        LinkKind::Junction => ProjectionMode::Junction,
        _ => ProjectionMode::Symlink,
    };
    create_dir_link(new_target, path, mode)
}

fn same_path(a: &Path, b: &Path) -> bool {
    Some(a) == Some(b) || dunce::simplified(a) == dunce::simplified(b)
}
