//! Adopt: bring a skill that already lives somewhere on disk into the store (design §8.2 A).
//!
//! Invariants:
//! * the original is **always** backed up before anything changes;
//! * identical content is de-duplicated onto one slug instead of creating a second copy;
//! * the original location is replaced by a projection, never silently deleted;
//! * a user-chosen "keep original" leaves the file where it is and only registers it.

use std::path::{Path, PathBuf};

use heron_core::{Hash, ProjectionMode, Slug, SlugPolicy};
use heron_scan::FingerprintOptions;
use heron_store::models::{DetectedRow, ProjectionRow, SkillRow, TargetRow};
use heron_store::repos::{detected, event, projection, skill, target};

use crate::clients::default_landing;
use crate::{Ctx, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptPlacement {
    /// Move the original into the store; the old location becomes a projection of it.
    Replace,
    /// Leave the original untouched and only register it in the library.
    KeepOriginal,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdoptStep {
    CreateStore { slug: String, from: String },
    Deduplicate { slug: String, already_at: String },
    Backup { from: String, to: String },
    LinkOriginalPath { path: String, target: String },
    RegisterOnly { path: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdoptPlan {
    pub detail: DetectedRow,
    pub slug: Option<String>,
    pub steps: Vec<AdoptStep>,
    pub issues: Vec<String>,
    /// True when the same content already exists under a slug: no copy will be made.
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdoptReport {
    pub adopted: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// Build the plan for one detected item without touching the disk.
pub fn plan(ctx: &Ctx, row: &DetectedRow) -> Result<AdoptPlan> {
    let issues: Vec<String> = if row.frontmatter_ok == Some(false) {
        vec!["frontmatter_invalid".to_owned()]
    } else {
        vec![]
    };
    let Some(raw_slug) = row.observed_slug.clone() else {
        return Ok(AdoptPlan {
            detail: row.clone(),
            slug: None,
            steps: vec![],
            issues,
            duplicate_of: None,
        });
    };
    let slug = Slug::derive(&raw_slug, SlugPolicy::Lenient)?;
    let path = PathBuf::from(&row.path);
    let legacy = row.kind == "legacy-command";
    let content_dir = if legacy {
        // A slash command is a single file: adopt *it*, never the whole commands directory.
        path.clone()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
    };
    let hash = if legacy {
        single_file_hash(&path)?
    } else {
        heron_scan::fingerprint_dir(&content_dir, &FingerprintOptions::default())?.hash
    };

    // De-duplicate against an existing managed skill with identical content.
    let mut duplicate_of = None;
    for existing in skill::list(ctx.db.conn(), false)? {
        let existing_dir = ctx.skill_path(&existing.slug);
        if !existing_dir.is_dir() {
            continue;
        }
        if let Ok(fp) = heron_scan::fingerprint_dir(&existing_dir, &FingerprintOptions::default()) {
            if fp.hash == hash {
                duplicate_of = Some(existing.slug.clone());
                break;
            }
        }
    }

    let final_slug = duplicate_of
        .clone()
        .unwrap_or_else(|| slug.as_str().to_owned());
    let mut steps = Vec::new();
    if duplicate_of.is_none() {
        steps.push(AdoptStep::CreateStore {
            slug: final_slug.clone(),
            from: content_dir.to_string_lossy().to_string(),
        });
    } else {
        steps.push(AdoptStep::Deduplicate {
            slug: final_slug.clone(),
            already_at: ctx.skill_path(&final_slug).to_string_lossy().to_string(),
        });
    }
    steps.push(AdoptStep::Backup {
        from: content_dir.to_string_lossy().to_string(),
        to: "<backups>/adopt".to_owned(),
    });
    steps.push(AdoptStep::LinkOriginalPath {
        path: path.to_string_lossy().to_string(),
        target: ctx.skill_path(&final_slug).to_string_lossy().to_string(),
    });

    Ok(AdoptPlan {
        detail: row.clone(),
        slug: Some(final_slug),
        steps,
        issues,
        duplicate_of,
    })
}

/// Execute a plan. `dry_run` returns the plan without side effects.
pub fn execute(ctx: &mut Ctx, plan: &AdoptPlan, placement: AdoptPlacement) -> Result<AdoptReport> {
    let mut report = AdoptReport {
        adopted: vec![],
        skipped: vec![],
        errors: vec![],
    };
    let Some(slug) = plan.slug.clone() else {
        report
            .skipped
            .push(format!("{}: no slug", plan.detail.path));
        return Ok(report);
    };

    let source_md = PathBuf::from(&plan.detail.path);
    let legacy = plan.detail.kind == "legacy-command";
    let source_dir = source_md
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| source_md.clone());
    let dest = ctx.skill_path(&slug);
    let duplicate = plan.duplicate_of.is_some();

    // 1) Always back up before the first mutation.
    let backup_dir = ctx.backups_dir().join(format!("adopt-{slug}"));
    std::fs::create_dir_all(&backup_dir).ok();
    if legacy {
        std::fs::copy(&source_md, backup_dir.join("SKILL.md")).map_err(|e| {
            heron_core::HeronError::from(
                heron_core::ErrorCode::Permission
                    .msg()
                    .with("path", source_md.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
    } else {
        copy_tree(&source_dir, &backup_dir)?;
    }

    let event_id = ctx.db.write(|tx| {
        event::record(
            tx,
            ctx.actor,
            "adopt",
            Some(&slug),
            Some(&serde_json::to_string(plan).unwrap_or_default()),
            true,
        )
    })?;

    // 2) Materialize the store copy + its repository.
    if !duplicate {
        std::fs::create_dir_all(ctx.store_root()).ok();
        if legacy {
            // The command file becomes the skill body.
            std::fs::create_dir_all(&dest).ok();
            std::fs::copy(&source_md, dest.join("SKILL.md")).map_err(|e| {
                heron_core::HeronError::from(
                    heron_core::ErrorCode::Permission
                        .msg()
                        .with("path", source_md.display().to_string())
                        .with("detail", e.to_string()),
                )
            })?;
        } else {
            copy_tree(&source_dir, &dest)?;
        }
        let repo = heron_vcs::SkillRepo::open(&dest);
        let origin = if plan.detail.kind == "legacy-command" {
            "legacy-command"
        } else {
            "local"
        };
        // A store directory without a repository also means "the previous adopt got this far and
        // then failed", so the same call repairs it instead of failing on its leftovers.
        if let Err(e) = repo.init(&heron_vcs::SkillMeta::new(&slug, origin, "", "0.0.0")) {
            let mut msg = e.as_msg();
            let detail = msg
                .params
                .get("detail")
                .map(|d| format!("store copy at {} ({d})", dest.display()))
                .unwrap_or_else(|| format!("store copy at {}", dest.display()));
            msg.params.insert("detail".to_owned(), detail);
            return Err(heron_core::HeronError::from(msg));
        }
    }

    // 3) Register in the index.
    let already = skill::get(ctx.db.conn(), &slug)?.is_some();
    if !already {
        let display_name = plan
            .detail
            .observed_slug
            .clone()
            .unwrap_or_else(|| slug.clone());
        let origin = if plan.detail.kind == "legacy-command" {
            "legacy-command"
        } else {
            "adopted"
        };
        ctx.db.write(|tx| {
            skill::insert(
                tx,
                &SkillRow {
                    slug: slug.clone(),
                    display_name,
                    description: String::new(),
                    tags: vec![],
                    status: "active".to_owned(),
                    origin: origin.to_owned(),
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )
        })?;
    }

    // 4) Point the original location at the store (or leave it alone).
    match placement {
        AdoptPlacement::Replace if legacy => {
            // The original file stays where it is: a client still reads it, and rewriting a file
            // into a link is riskier than keeping both in sync (implementation doc §8.3).
        }
        AdoptPlacement::Replace => {
            let landing = default_landing(&home_dir()?);
            let managed_target = ensure_default_target(ctx, &landing)?;
            let mode = ProjectionMode::resolve_auto(heron_core::PlatformKind::current());
            let dest_link = source_dir.clone();
            if heron_platform::link::probe(&dest_link)?.kind.is_real_dir() {
                std::fs::remove_dir_all(&dest_link).map_err(|e| {
                    heron_core::HeronError::from(
                        heron_core::ErrorCode::Permission
                            .msg()
                            .with("path", dest_link.display().to_string())
                            .with("detail", e.to_string()),
                    )
                })?;
            }
            heron_platform::link::create_dir_link(&ctx.skill_path(&slug), &dest_link, mode)?;
            // Several copies of one skill may live in the same client directory (e.g. both
            // `.agents/skills/pdf` and `.claude/skills/pdf`), so the recorded link name must be
            // derived from the original location, not from the slug.
            // `<client>@<slot>`: verify reads the client prefix to judge visibility against the
            // right scan paths.
            let client_kind = plan
                .detail
                .client_id
                .clone()
                .unwrap_or_else(|| "heron".to_owned());
            let link_name = format!(
                "{client_kind}@{}",
                heron_platform::paths::project_key(&dest_link)
            );
            ctx.db.write(|tx| {
                projection::insert(
                    tx,
                    &ProjectionRow {
                        slug: slug.clone(),
                        target_id: managed_target.id.clone(),
                        link_name,
                        path: dest_link.to_string_lossy().to_string(),
                        mode: match mode {
                            ProjectionMode::Junction => "junction",
                            _ => "symlink",
                        }
                        .to_owned(),
                        version: None,
                        content_hash: None,
                        inode_kind: None,
                        ..Default::default()
                    },
                )?;
                Ok(())
            })?;
        }
        AdoptPlacement::KeepOriginal => {}
    }

    // 5) Mark the detected row as adopted and close the undo window.
    ctx.db.write(|tx| {
        detected::set_state(tx, &plan.detail.path, "adopted")?;
        event::finish(tx, event_id, false)?;
        Ok(())
    })?;
    report.adopted.push(slug);
    Ok(report)
}

fn ensure_default_target(ctx: &mut Ctx, landing: &Path) -> Result<TargetRow> {
    if let Some(existing) = target::get_default(ctx.db.conn())? {
        return Ok(existing);
    }
    let row = TargetRow {
        id: "heron:global".to_owned(),
        kind: "heron".to_owned(),
        scope: "global".to_owned(),
        project_path: None,
        path: landing.to_string_lossy().to_string(),
        enabled: true,
        is_default: true,
        discover: "manual".to_owned(),
    };
    ctx.db.write(|tx| target::upsert(tx, &row))?;
    Ok(row)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
        })
        .ok_or_else(|| {
            heron_core::HeronError::from(
                heron_core::ErrorCode::Internal
                    .msg()
                    .with("detail", "no_home_dir"),
            )
        })
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| {
        heron_core::HeronError::from(
            heron_core::ErrorCode::Permission
                .msg()
                .with("path", to.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    for entry in std::fs::read_dir(from).map_err(|e| {
        heron_core::HeronError::from(
            heron_core::ErrorCode::Permission
                .msg()
                .with("path", from.display().to_string())
                .with("detail", e.to_string()),
        )
    })? {
        let entry = entry.map_err(|e| {
            heron_core::HeronError::from(
                heron_core::ErrorCode::Permission
                    .msg()
                    .with("detail", e.to_string()),
            )
        })?;
        let ty = entry.file_type().map_err(|e| {
            heron_core::HeronError::from(
                heron_core::ErrorCode::Permission
                    .msg()
                    .with("detail", e.to_string()),
            )
        })?;
        let dst = to.join(entry.file_name());
        if ty.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_tree(&entry.path(), &dst)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dst).map_err(|e| {
                heron_core::HeronError::from(
                    heron_core::ErrorCode::Permission
                        .msg()
                        .with("path", dst.display().to_string())
                        .with("detail", e.to_string()),
                )
            })?;
        }
    }
    Ok(())
}

/// Hash of a single file (used for legacy command files).
fn single_file_hash(path: &Path) -> Result<Hash> {
    let bytes = std::fs::read(path).map_err(|e| {
        heron_core::HeronError::from(
            heron_core::ErrorCode::Permission
                .msg()
                .with("path", path.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"file");
    hasher.update(&heron_core::hash::normalize_text_bytes(&bytes));
    Ok(Hash::from_hex(hasher.finalize().to_hex().to_string()))
}

/// Convenience for tests: content hash of a directory.
pub fn content_hash(dir: &Path) -> Result<Hash> {
    Ok(heron_scan::fingerprint_dir(dir, &FingerprintOptions::default())?.hash)
}
