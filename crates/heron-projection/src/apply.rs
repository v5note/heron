//! Executing a projection plan.
//!
//! Every mutation goes through the platform link layer (risk R3). Copies are written to a
//! sibling temp directory first and then swapped in, so an interruption never leaves a
//! half-materialized skill in a client's scan path.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Hash, ProjectionMode};
use heron_platform::link::{self, LinkKind, LinkOutcome};

use crate::plan::{PlanAction, ProjectPlan};
use crate::Result;

/// What the caller should record in the index after `apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedProjection {
    pub path: PathBuf,
    pub mode: ProjectionMode,
    /// Real inode kind after the operation (`reparse_symlink`, `reparse_junction`, `dir`).
    pub inode_kind: String,
    pub content_hash: Option<Hash>,
}

pub fn apply_plan(plan: &ProjectPlan) -> Result<Vec<AppliedProjection>> {
    if plan.is_blocked() {
        let step = plan.steps.iter().find(|s| {
            matches!(
                s,
                PlanAction::Blocked { .. } | PlanAction::ModifiedCopy { .. }
            )
        });
        return Err(match step {
            Some(PlanAction::Blocked {
                path,
                reason: crate::plan::BlockedReason::ForeignLink,
            }) => ErrorCode::Drifted
                .msg()
                .with("path", path.display().to_string())
                .with("reason", "foreign_link")
                .into(),
            Some(PlanAction::Blocked {
                path,
                reason: crate::plan::BlockedReason::SourceMissing { source },
            }) => ErrorCode::LinkTargetMissing
                .msg()
                // ADR-0003 §3: `target` is what the link points at, i.e. the store copy. Naming the
                // destination here sends people looking in the client's skills folder for a
                // directory that was never supposed to exist yet.
                .with("target", source.display().to_string())
                .with("path", path.display().to_string())
                .with("detail", "store_content_missing")
                .into(),
            Some(PlanAction::Blocked { path, .. }) => ErrorCode::NotManaged
                .msg()
                .with("path", path.display().to_string())
                .into(),
            Some(PlanAction::ModifiedCopy { path }) => ErrorCode::Drifted
                .msg()
                .with("path", path.display().to_string())
                .with("reason", "copy_modified")
                .into(),
            _ => ErrorCode::Internal
                .msg()
                .with("detail", "blocked_plan")
                .into(),
        });
    }

    let mut applied = Vec::new();
    for step in &plan.steps {
        match step {
            PlanAction::AlreadyPresent => continue,
            PlanAction::CreateLink { target, path, mode } => {
                // If a managed copy occupies the path, the link layer would (correctly) refuse;
                // remove the copy we own first so the link can take its place.
                if link::probe(path)?.kind.is_real_dir() {
                    std::fs::remove_dir_all(path).map_err(|e| {
                        ErrorCode::Permission
                            .msg()
                            .with("path", path.display().to_string())
                            .with("detail", e.to_string())
                    })?;
                }
                let outcome = link::create_dir_link(target, path, *mode)?;
                let warning = match &outcome {
                    LinkOutcome::Downgraded { warning, .. } => Some(warning.clone()),
                    _ => None,
                };
                let inode_kind = inode_kind(path)?;
                let effective_mode = match inode_kind.as_str() {
                    "reparse_junction" => ProjectionMode::Junction,
                    "reparse_symlink" => ProjectionMode::Symlink,
                    _ => *mode,
                };
                if let Some(w) = warning {
                    // The downgrade is expected and non-fatal: surface it as a message upstream.
                    let _ = w;
                }
                applied.push(AppliedProjection {
                    path: path.clone(),
                    mode: effective_mode,
                    inode_kind,
                    content_hash: None,
                });
            }
            PlanAction::ReplaceLink { path, target, mode } => {
                // Keep the platform-specific link flavour when relinking.
                if heron_platform::link::probe(path)?.kind == LinkKind::Junction {
                    link::remove_link(path)?;
                    link::create_dir_link(target, path, ProjectionMode::Junction)?;
                } else {
                    link::relink(path, target)?;
                }
                applied.push(AppliedProjection {
                    path: path.clone(),
                    mode: *mode,
                    inode_kind: inode_kind(path)?,
                    content_hash: None,
                });
            }
            PlanAction::CopyTree { from, to, hash } => {
                copy_tree_atomic(from, to)?;
                applied.push(AppliedProjection {
                    path: to.clone(),
                    mode: ProjectionMode::Copy,
                    inode_kind: "dir".to_owned(),
                    content_hash: hash.clone(),
                });
            }
            PlanAction::WritePointer { path, real } => {
                write_pointer(path, real)?;
                applied.push(AppliedProjection {
                    path: path.clone(),
                    mode: ProjectionMode::Pointer,
                    inode_kind: "dir".to_owned(),
                    content_hash: None,
                });
            }
            PlanAction::Blocked { .. } | PlanAction::ModifiedCopy { .. } => {
                unreachable!("blocked plans return early")
            }
        }
    }
    Ok(applied)
}

/// Remove a projection. Only links and pointer directories are removed; a copy is removed only
/// when its hash still matches what Heron wrote.
pub fn remove_projection(
    path: &Path,
    mode: ProjectionMode,
    recorded_hash: Option<&Hash>,
) -> Result<()> {
    let info = link::probe(path)?;
    match info.kind {
        LinkKind::Missing => Ok(()),
        kind if kind.is_link() => Ok(link::remove_link(path)?),
        LinkKind::Directory => match mode {
            ProjectionMode::Copy | ProjectionMode::Pointer => {
                if let Some(expected) = recorded_hash {
                    let actual = heron_scan::fingerprint::fingerprint_dir(
                        path,
                        &heron_scan::FingerprintOptions::default(),
                    )?;
                    if &actual.hash != expected {
                        return Err(ErrorCode::Drifted
                            .msg()
                            .with("path", path.display().to_string())
                            .with("reason", "copy_modified")
                            .into());
                    }
                }
                std::fs::remove_dir_all(path).map_err(|e| {
                    ErrorCode::Permission
                        .msg()
                        .with("path", path.display().to_string())
                        .with("detail", e.to_string())
                })?;
                Ok(())
            }
            _ => Err(ErrorCode::SafetyRefused
                .msg()
                .with("path", path.display().to_string())
                .with("reason", "not_a_projection")
                .into()),
        },
        _ => Err(ErrorCode::SafetyRefused
            .msg()
            .with("path", path.display().to_string())
            .with("reason", "not_a_projection")
            .into()),
    }
}

fn inode_kind(path: &Path) -> Result<String> {
    Ok(match link::probe(path)?.kind {
        LinkKind::Junction => "reparse_junction",
        LinkKind::SymbolicDir => "reparse_symlink",
        LinkKind::SymbolicFile => "reparse_file",
        LinkKind::Directory => "dir",
        LinkKind::File => "file",
        LinkKind::Missing => "missing",
    }
    .to_owned())
}

/// Copy `from` into `to` via a temp sibling + rename, so a crash never leaves a partial copy.
pub fn copy_tree_atomic(from: &Path, to: &Path) -> Result<()> {
    let parent = to.parent().ok_or_else(|| {
        heron_core::HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", to.display().to_string()),
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|e| {
        ErrorCode::Permission
            .msg()
            .with("path", parent.display().to_string())
            .with("detail", e.to_string())
    })?;

    let temp = parent.join(format!(".{}.heron-tmp", file_name(to)?));
    let _ = std::fs::remove_dir_all(&temp);
    copy_dir_recursive(from, &temp)?;

    // Replace the destination (it is either absent or a managed copy the caller vetted).
    if to.exists() {
        if link::probe(to)?.kind.is_link() {
            link::remove_link(to)?;
        } else {
            std::fs::remove_dir_all(to).map_err(|e| {
                ErrorCode::Permission
                    .msg()
                    .with("path", to.display().to_string())
                    .with("detail", e.to_string())
            })?;
        }
    }
    std::fs::rename(&temp, to).map_err(|e| {
        let _ = std::fs::remove_dir_all(&temp);
        heron_core::HeronError::from(
            ErrorCode::Permission
                .msg()
                .with("path", to.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| {
        ErrorCode::Permission
            .msg()
            .with("path", to.display().to_string())
            .with("detail", e.to_string())
    })?;
    for entry in std::fs::read_dir(from).map_err(|e| {
        ErrorCode::Permission
            .msg()
            .with("path", from.display().to_string())
            .with("detail", e.to_string())
    })? {
        let entry = entry.map_err(|e| ErrorCode::Permission.msg().with("detail", e.to_string()))?;
        let name = entry.file_name();
        let src = entry.path();
        let dst = to.join(&name);
        let ty = entry
            .file_type()
            .map_err(|e| ErrorCode::Permission.msg().with("detail", e.to_string()))?;
        if ty.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else if ty.is_file() {
            std::fs::copy(&src, &dst).map_err(|e| {
                ErrorCode::Permission
                    .msg()
                    .with("path", dst.display().to_string())
                    .with("detail", e.to_string())
            })?;
        }
        // Symlinks inside a skill are skipped: the store copy is the authority.
    }
    Ok(())
}

fn write_pointer(path: &Path, real: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| {
        ErrorCode::Permission
            .msg()
            .with("path", path.display().to_string())
            .with("detail", e.to_string())
    })?;
    let name = file_name(real).unwrap_or_else(|_| "skill".to_owned());
    let body = format!(
        "---\nname: {name}\ndescription: Pointer to the canonical copy managed by Heron.\n---\n\n# {name}\n\n\
Read the real instructions at `{real}` and follow them exactly.\n\
Any relative paths it mentions resolve against that directory, not this one.\n",
        real = real.display()
    );
    heron_platform::atomic::write_atomic(&path.join("SKILL.md"), body.as_bytes())?;
    Ok(())
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| {
            heron_core::HeronError::from(ErrorCode::Internal.msg().with("detail", "no_file_name"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{plan_projection, ProjectionRequest};

    fn fixture() -> (tempfile::TempDir, ProjectionRequest) {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("store/wechat-mp");
        std::fs::create_dir_all(source.join("references")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: wechat-mp\n---\n").unwrap();
        std::fs::write(source.join("references/a.md"), "ref").unwrap();
        let req = ProjectionRequest {
            source,
            dest: tmp.path().join("skills/wechat-mp"),
            mode: ProjectionMode::Symlink,
            source_hash: None,
            recorded_hash: None,
            already_managed: false,
        };
        (tmp, req)
    }

    #[test]
    fn apply_link_then_remove_keeps_store_intact() {
        let (_tmp, req) = fixture();
        let applied = apply_plan(&plan_projection(&req).unwrap()).unwrap();
        assert_eq!(applied.len(), 1);
        assert!(req.dest.join("references/a.md").is_file());

        remove_projection(&req.dest, ProjectionMode::Symlink, None).unwrap();
        assert!(!req.dest.exists());
        assert!(req.source.join("SKILL.md").is_file(), "store untouched");
    }

    #[test]
    fn apply_copy_then_remove() {
        let (_tmp, mut req) = fixture();
        req.mode = ProjectionMode::Copy;
        let plan = plan_projection(&req).unwrap();
        let applied = apply_plan(&plan).unwrap();
        assert_eq!(applied[0].inode_kind, "dir");
        assert!(req.dest.join("references/a.md").is_file());
        assert!(
            !req.dest.join(".wechat-mp.heron-tmp").exists(),
            "temp dir is swapped in"
        );

        let hash = heron_scan::fingerprint_dir(&req.dest, &Default::default())
            .unwrap()
            .hash;
        remove_projection(&req.dest, ProjectionMode::Copy, Some(&hash)).unwrap();
        assert!(!req.dest.exists());
        assert!(req.source.join("SKILL.md").is_file());
    }

    #[test]
    fn removing_a_modified_copy_is_refused() {
        let (_tmp, mut req) = fixture();
        req.mode = ProjectionMode::Copy;
        apply_plan(&plan_projection(&req).unwrap()).unwrap();
        let hash = heron_scan::fingerprint_dir(&req.dest, &Default::default())
            .unwrap()
            .hash;

        std::fs::write(req.dest.join("notes.md"), "user edit").unwrap();
        let err = remove_projection(&req.dest, ProjectionMode::Copy, Some(&hash)).unwrap_err();
        assert_eq!(err.code(), "E_DRIFTED");
        assert!(req.dest.join("notes.md").is_file(), "user data preserved");
    }

    #[test]
    fn pointer_mode_writes_a_usable_skill_md() {
        let (_tmp, mut req) = fixture();
        req.mode = ProjectionMode::Pointer;
        apply_plan(&plan_projection(&req).unwrap()).unwrap();
        let text = std::fs::read_to_string(req.dest.join("SKILL.md")).unwrap();
        assert!(text.contains("name: wechat-mp"));
        assert!(text.contains(&req.source.display().to_string()));
    }

    #[test]
    fn a_missing_store_copy_is_reported_as_the_source() {
        let (_tmp, mut req) = fixture();
        req.source = req.source.with_file_name("finesse-ui");
        let err = apply_plan(&plan_projection(&req).unwrap()).unwrap_err();
        assert_eq!(err.code(), "E_LINK_TARGET_MISSING");
        let msg = err.as_msg();
        assert_eq!(
            msg.params.get("target").map(String::as_str),
            Some(req.source.to_string_lossy().as_ref()),
            "the report has to name the store copy that is gone"
        );
        assert!(
            !req.dest.exists(),
            "a blocked plan must leave nothing in the client's folder"
        );
    }

    #[test]
    fn switching_modes_round_trips() {
        let (_tmp, mut req) = fixture();
        apply_plan(&plan_projection(&req).unwrap()).unwrap();
        assert!(link::probe(&req.dest).unwrap().kind.is_link());

        // link -> copy
        req.already_managed = true;
        req.mode = ProjectionMode::Copy;
        apply_plan(&plan_projection(&req).unwrap()).unwrap();
        assert!(!link::probe(&req.dest).unwrap().kind.is_link());
        assert!(req.dest.join("SKILL.md").is_file());

        // copy -> link
        req.mode = ProjectionMode::Symlink;
        let plan = plan_projection(&req).unwrap();
        assert!(
            matches!(plan.steps[0], PlanAction::CreateLink { .. }),
            "managed copy must convert to a link, got {:?}",
            plan.steps[0]
        );
        apply_plan(&plan).unwrap();
        assert!(link::probe(&req.dest).unwrap().kind.is_link());
        assert!(req.source.join("SKILL.md").is_file());
    }
}
