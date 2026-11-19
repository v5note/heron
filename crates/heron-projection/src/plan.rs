//! Planning a projection: pure computation over the current disk + index state.
//!
//! Decision table (implementation doc §7.1): the planner never decides to destroy data. When
//! something unexpected is in the way it emits `Blocked` with the reason instead of an action.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Hash, ProjectionMode};
use heron_platform::link::{self, LinkKind};
use serde::Serialize;

use crate::Result;

#[derive(Debug, Clone)]
pub struct ProjectionRequest {
    /// Directory of the skill inside the store.
    pub source: PathBuf,
    /// Directory to materialize into (`<target>/<link_name>`).
    pub dest: PathBuf,
    pub mode: ProjectionMode,
    /// Fingerprint of the store copy, recorded so drift can be detected later.
    pub source_hash: Option<Hash>,
    /// Fingerprint recorded for an existing copy projection, if any.
    pub recorded_hash: Option<Hash>,
    /// Whether the dest is already recorded as a projection of this skill.
    pub already_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlanAction {
    /// The destination already matches: nothing to do.
    AlreadyPresent,
    /// Create a link (symlink/junction) at `path` pointing at `target`.
    CreateLink {
        target: PathBuf,
        path: PathBuf,
        mode: ProjectionMode,
    },
    /// Copy the tree into `path` and record `hash`.
    CopyTree {
        from: PathBuf,
        to: PathBuf,
        hash: Option<Hash>,
    },
    /// Write a pointer `SKILL.md` that sends the client to `real`.
    WritePointer { path: PathBuf, real: PathBuf },
    /// Something else is in the way (a real directory, a foreign link): needs a decision.
    Blocked {
        path: PathBuf,
        reason: BlockedReason,
    },
    /// The destination is a link of ours pointing elsewhere: detach then recreate.
    ReplaceLink {
        path: PathBuf,
        target: PathBuf,
        mode: ProjectionMode,
    },
    /// Cannot copy over a modified copy without destroying user edits.
    ModifiedCopy { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedReason {
    /// A real directory/file occupies the destination and Heron does not manage it.
    NotManaged,
    /// The destination is a link owned by something else.
    ForeignLink,
    /// The store copy is missing.
    SourceMissing,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPlan {
    pub steps: Vec<PlanAction>,
    pub warnings: Vec<heron_core::Msg>,
    pub tokens: u32,
}

impl ProjectPlan {
    pub fn is_noop(&self) -> bool {
        self.steps
            .iter()
            .all(|s| matches!(s, PlanAction::AlreadyPresent))
    }

    pub fn is_blocked(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s,
                PlanAction::Blocked { .. } | PlanAction::ModifiedCopy { .. }
            )
        })
    }
}

/// Compute the plan for one projection.
pub fn plan_projection(req: &ProjectionRequest) -> Result<ProjectPlan> {
    if !req.source.is_dir() {
        return Ok(blocked(
            &req.dest,
            BlockedReason::SourceMissing,
            "projection.source_missing",
        ));
    }

    let info = link::probe(&req.dest)?;
    let mut warnings = Vec::new();

    let step = if info.kind == LinkKind::Missing {
        create_step(req)
    } else if req.mode.is_link() {
        // Wanted: a link.
        if info.kind.is_link() {
            match &info.target {
                Some(existing) if same_dir(existing, &req.source) => PlanAction::AlreadyPresent,
                _ if req.already_managed => PlanAction::ReplaceLink {
                    path: req.dest.clone(),
                    target: req.source.clone(),
                    mode: req.mode,
                },
                _ => PlanAction::Blocked {
                    path: req.dest.clone(),
                    reason: BlockedReason::ForeignLink,
                },
            }
        } else if req.already_managed {
            // A managed copy being converted into a link.
            create_step(req)
        } else {
            PlanAction::Blocked {
                path: req.dest.clone(),
                reason: BlockedReason::NotManaged,
            }
        }
    } else {
        // Wanted: copy or pointer.
        if info.kind.is_link() {
            // A managed link being converted into real content.
            if req.already_managed {
                create_step(req)
            } else {
                PlanAction::Blocked {
                    path: req.dest.clone(),
                    reason: BlockedReason::ForeignLink,
                }
            }
        } else if !req.already_managed {
            PlanAction::Blocked {
                path: req.dest.clone(),
                reason: BlockedReason::NotManaged,
            }
        } else if let (Some(recorded), Some(actual)) =
            (req.recorded_hash.clone(), current_hash(&req.dest))
        {
            if recorded != actual {
                PlanAction::ModifiedCopy {
                    path: req.dest.clone(),
                }
            } else {
                create_step(req)
            }
        } else {
            create_step(req)
        }
    };

    if matches!(req.mode, ProjectionMode::Junction) && !cfg!(windows) {
        warnings.push(
            ErrorCode::Internal
                .msg()
                .with("detail", "junction_requested_off_windows"),
        );
    }

    Ok(ProjectPlan {
        steps: vec![step],
        warnings,
        tokens: 0,
    })
}

fn create_step(req: &ProjectionRequest) -> PlanAction {
    match req.mode {
        ProjectionMode::Symlink | ProjectionMode::Junction => PlanAction::CreateLink {
            target: req.source.clone(),
            path: req.dest.clone(),
            mode: req.mode,
        },
        ProjectionMode::Copy => PlanAction::CopyTree {
            from: req.source.clone(),
            to: req.dest.clone(),
            hash: req.source_hash.clone(),
        },
        ProjectionMode::Pointer => PlanAction::WritePointer {
            path: req.dest.clone(),
            real: req.source.clone(),
        },
    }
}

fn blocked(dest: &Path, reason: BlockedReason, code: &'static str) -> ProjectPlan {
    ProjectPlan {
        steps: vec![PlanAction::Blocked {
            path: dest.to_path_buf(),
            reason,
        }],
        warnings: vec![ErrorCode::NotManaged.msg().with("detail", code)],
        tokens: 0,
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    heron_platform::paths::same_path_ci(a, b)
}

fn current_hash(path: &Path) -> Option<Hash> {
    heron_scan::fingerprint::fingerprint_dir(path, &heron_scan::FingerprintOptions::default())
        .ok()
        .map(|f| f.hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, ProjectionRequest) {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("store/pdf");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
        let req = ProjectionRequest {
            source,
            dest: tmp.path().join("skills/pdf"),
            mode: ProjectionMode::Symlink,
            source_hash: None,
            recorded_hash: None,
            already_managed: false,
        };
        (tmp, req)
    }

    #[test]
    fn creates_a_link_into_an_empty_location() {
        let (_tmp, req) = setup();
        let plan = plan_projection(&req).unwrap();
        assert!(matches!(plan.steps[0], PlanAction::CreateLink { .. }));
        assert!(!plan.is_blocked());
    }

    #[test]
    fn missing_source_blocks() {
        let (_tmp, mut req) = setup();
        req.source = req.dest.with_file_name("ghost");
        let plan = plan_projection(&req).unwrap();
        assert!(matches!(
            plan.steps[0],
            PlanAction::Blocked {
                reason: BlockedReason::SourceMissing,
                ..
            }
        ));
    }

    #[test]
    fn existing_identical_link_is_a_noop() {
        let (_tmp, req) = setup();
        heron_platform::link::create_dir_link(&req.source, &req.dest, ProjectionMode::Symlink)
            .unwrap();
        let plan = plan_projection(&req).unwrap();
        assert_eq!(plan.steps[0], PlanAction::AlreadyPresent);
        assert!(plan.is_noop());
    }

    #[test]
    fn foreign_directory_is_never_replaced() {
        let (_tmp, req) = setup();
        std::fs::create_dir_all(&req.dest).unwrap();
        std::fs::write(req.dest.join("user.txt"), "mine").unwrap();
        let plan = plan_projection(&req).unwrap();
        assert!(matches!(
            plan.steps[0],
            PlanAction::Blocked {
                reason: BlockedReason::NotManaged,
                ..
            }
        ));
        assert!(req.dest.join("user.txt").is_file());
    }

    #[test]
    fn foreign_link_is_reported_as_blocked() {
        let (tmp, mut req) = setup();
        let other = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(req.dest.parent().unwrap()).unwrap();
        heron_platform::link::create_dir_link(&other, &req.dest, ProjectionMode::Symlink).unwrap();
        req.already_managed = false;
        let plan = plan_projection(&req).unwrap();
        assert!(matches!(
            plan.steps[0],
            PlanAction::Blocked {
                reason: BlockedReason::ForeignLink,
                ..
            }
        ));
    }

    #[test]
    fn managed_link_pointing_elsewhere_is_replaced() {
        let (tmp, mut req) = setup();
        let other = tmp.path().join("store/pdf-v2");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(req.dest.parent().unwrap()).unwrap();
        heron_platform::link::create_dir_link(&other, &req.dest, ProjectionMode::Symlink).unwrap();
        req.already_managed = true;
        let plan = plan_projection(&req).unwrap();
        assert!(matches!(plan.steps[0], PlanAction::ReplaceLink { .. }));
    }

    #[test]
    fn modified_copy_is_flagged_not_overwritten() {
        let (tmp, mut req) = setup();
        req.mode = ProjectionMode::Copy;
        req.already_managed = true;
        let dest = req.dest.clone();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
        let clean = current_hash(&dest).unwrap();
        req.recorded_hash = Some(clean);
        // Untouched copy: rewriting is fine.
        assert!(matches!(
            plan_projection(&req).unwrap().steps[0],
            PlanAction::CopyTree { .. }
        ));

        std::fs::write(dest.join("notes.md"), "user edited this").unwrap();
        let plan = plan_projection(&req).unwrap();
        assert!(
            matches!(plan.steps[0], PlanAction::ModifiedCopy { .. }),
            "got {:?}",
            plan.steps[0]
        );
        let _ = tmp;
    }
}
