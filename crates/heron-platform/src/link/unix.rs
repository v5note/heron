//! Unix implementation: symbolic links only.

use std::os::unix::fs::symlink;
use std::path::Path;

use heron_core::{Msg, ProjectionMode};

use super::{LinkInfo, LinkKind, LinkOutcome};
use crate::dirs::io_msg;

pub(super) fn probe(path: &Path) -> Result<LinkInfo, Msg> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LinkInfo::missing()),
        Err(e) => return Err(io_msg(path)(e)),
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path).ok();
        let resolved_exists = path.exists();
        let kind = match target.as_deref().map(std::fs::metadata) {
            Some(Ok(t)) if t.is_dir() => LinkKind::SymbolicDir,
            Some(Ok(_)) => LinkKind::SymbolicFile,
            // Broken link: report it as a directory link so callers treat it as a link.
            _ => LinkKind::SymbolicDir,
        };
        Ok(LinkInfo {
            kind,
            target,
            resolved_exists,
        })
    } else if meta.is_dir() {
        Ok(LinkInfo {
            kind: LinkKind::Directory,
            target: None,
            resolved_exists: true,
        })
    } else {
        Ok(LinkInfo {
            kind: LinkKind::File,
            target: None,
            resolved_exists: true,
        })
    }
}

pub(super) fn create(target: &Path, link: &Path, mode: ProjectionMode) -> Result<LinkOutcome, Msg> {
    let _ = mode; // Junctions do not exist on unix; both modes produce a symlink.
    symlink(target, link).map_err(io_msg(link))?;
    Ok(LinkOutcome::Created(LinkKind::SymbolicDir))
}

pub(super) fn unlink(path: &Path, _kind: LinkKind) -> Result<(), Msg> {
    // remove_file on a symlink removes the link, never the target.
    std::fs::remove_file(path).map_err(io_msg(path))
}
