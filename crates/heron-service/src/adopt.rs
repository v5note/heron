//! Adopt: bring a skill that already lives somewhere on disk into the store (design §8.2 A).
//!
//! Invariants:
//! * the original is **always** backed up before anything changes;
//! * identical content is de-duplicated onto one slug instead of creating a second copy;
//! * the original location is replaced by a projection, never silently deleted;
//! * a user-chosen "keep original" leaves the file where it is and only registers it;
//! * content that lives behind a link Heron did not create is never re-pointed without the plan
//!   saying so (design §8.2 A / §11.75).

use std::path::{Path, PathBuf};

use heron_core::{Hash, ProjectionMode, Slug, SlugPolicy};
use heron_platform::link::LinkKind;
use heron_scan::FingerprintOptions;
use heron_store::models::{DetectedRow, ProjectionRow, SkillRow, SourceBindingRow, TargetRow};
use heron_store::repos::{binding, detected, event, projection, skill, target};

use crate::clients::default_landing;
use crate::{Ctx, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdoptPlacement {
    /// Move the original into the store; the old location becomes a projection of it.
    Replace,
    /// Leave the original untouched and only register it in the library.
    KeepOriginal,
    /// Bind the library to the directory a foreign link already points at. Nothing is copied,
    /// deleted or re-pointed — the user keeps editing the source they already have (就地引用).
    LiveBind,
}

impl AdoptPlacement {
    /// Stable identifiers used by the CLI flag and the GUI payload.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::KeepOriginal => "keep-original",
            Self::LiveBind => "live-bind",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "replace" | "move" => Some(Self::Replace),
            "keep-original" | "keep" => Some(Self::KeepOriginal),
            "live-bind" | "livebind" => Some(Self::LiveBind),
            _ => None,
        }
    }
}

/// Who currently owns the entry that was found on disk. Deciding this *before* touching anything
/// is what keeps adopt from turning a hand-made link into two diverging copies.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    /// A directory (or command file) the user owns outright: safe to move into the store.
    RealDir,
    /// Already a Heron projection. Adopt becomes an index repair.
    HeronLink,
    /// A link somebody else created — `npx skills`, or a developer pointing at their own repo.
    /// Its content stays where it is unless the user explicitly asks for a copy.
    ForeignLink { target: String },
    /// The entry vanished since the scan.
    Missing,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdoptStep {
    CreateStore { slug: String, from: String },
    Deduplicate { slug: String, already_at: String },
    Backup { from: String, to: String },
    LinkOriginalPath { path: String, target: String },
    RegisterOnly { path: String },
    /// Bind without moving: the library points at the directory the link already resolves to.
    LiveBind { slug: String, at: String },
    /// Copying this entry into the store leaves the directory the link pointed at unreferenced.
    SourceLeftBehind { path: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdoptPlan {
    pub detail: DetectedRow,
    pub slug: Option<String>,
    pub steps: Vec<AdoptStep>,
    pub issues: Vec<String>,
    /// True when the same content already exists under a slug: no copy will be made.
    pub duplicate_of: Option<String>,
    /// What currently owns the entry on disk.
    pub entry: EntryState,
    /// The placement adopt should default to for this entry.
    pub suggested: AdoptPlacement,
    /// Upstream an installer recorded for this entry, if any.
    pub upstream: Option<crate::npx_lock::Upstream>,
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
    let path = PathBuf::from(&row.path);
    let legacy = row.kind == "legacy-command";
    let content_dir = entry_path(row);
    let entry = entry_state(ctx, &content_dir)?;
    let upstream = if legacy {
        None
    } else {
        crate::npx_lock::for_entry(&content_dir)
    };
    let Some(raw_slug) = row.observed_slug.clone() else {
        return Ok(AdoptPlan {
            detail: row.clone(),
            slug: None,
            steps: vec![],
            issues,
            duplicate_of: None,
            suggested: suggest_placement(&entry, legacy, false),
            entry,
            upstream,
        });
    };
    let slug = Slug::derive(&raw_slug, SlugPolicy::Lenient)?;
    let hash = if legacy {
        single_file_hash(&path)?
    } else {
        heron_scan::fingerprint_dir(&content_dir, &FingerprintOptions::default())?.hash
    };

    // De-duplicate against an existing managed skill with identical content.
    let mut duplicate_of = None;
    let mut duplicate_at = None;
    for existing in skill::list(ctx.db.conn(), false)? {
        let existing_dir = ctx.content_path(&existing);
        if !existing_dir.is_dir() {
            continue;
        }
        if let Ok(fp) = heron_scan::fingerprint_dir(&existing_dir, &FingerprintOptions::default()) {
            if fp.hash == hash {
                duplicate_at = Some(existing_dir.to_string_lossy().to_string());
                duplicate_of = Some(existing.slug.clone());
                break;
            }
        }
    }

    let suggested = suggest_placement(&entry, legacy, duplicate_of.is_some());
    let final_slug = duplicate_of
        .clone()
        .unwrap_or_else(|| slug.as_str().to_owned());
    let mut steps = Vec::new();
    if suggested == AdoptPlacement::LiveBind {
        let target = match &entry {
            EntryState::ForeignLink { target } => target.clone(),
            _ => content_dir.to_string_lossy().to_string(),
        };
        steps.push(AdoptStep::LiveBind {
            slug: final_slug.clone(),
            at: target,
        });
    } else {
        if duplicate_of.is_none() {
            steps.push(AdoptStep::CreateStore {
                slug: final_slug.clone(),
                from: content_dir.to_string_lossy().to_string(),
            });
        } else {
            steps.push(AdoptStep::Deduplicate {
                slug: final_slug.clone(),
                already_at: duplicate_at
                    .unwrap_or_else(|| ctx.skill_path(&final_slug).to_string_lossy().to_string()),
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
        if let EntryState::ForeignLink { target } = &entry {
            steps.push(AdoptStep::SourceLeftBehind {
                path: target.clone(),
            });
        }
    }

    Ok(AdoptPlan {
        detail: row.clone(),
        slug: Some(final_slug),
        steps,
        issues,
        duplicate_of,
        entry,
        suggested,
        upstream,
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

    // A live bind changes no files: the content stays exactly where the user's own link points.
    if placement == AdoptPlacement::LiveBind {
        return live_bind(ctx, plan, &slug);
    }

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
                    storage_path: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )
        })?;
    }

    record_upstream(ctx, &slug, plan.upstream.as_ref())?;

    // 4) Point the original location at the store (or leave it alone).
    match placement {
        AdoptPlacement::Replace if legacy => {
            // The original file stays where it is: a client still reads it, and rewriting a file
            // into a link is riskier than keeping both in sync (implementation doc §8.3).
        }
        AdoptPlacement::Replace => {
            let mode = ProjectionMode::resolve_auto(heron_core::PlatformKind::current());
            let dest_link = source_dir.clone();
            // The link takes the place of the entry *where it was found*, so the projection belongs
            // to that client's target, not the landing. A landing-booked row makes `verify` name the
            // wrong client and `unproject --on` miss the link.
            let managed_target = match target::find_for_path(ctx.db.conn(), &dest_link)? {
                Some(row) => row,
                None => ensure_default_target(ctx, &default_landing(&home_dir()?))?,
            };
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
        // A live bind is decided at the top of `execute`, before anything was backed up or copied:
        // its whole promise is that no byte under the user's link is touched.
        AdoptPlacement::LiveBind => {}
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

/// The entry adopt treats as one unit: a skill's directory, or the command file itself.
fn entry_path(row: &DetectedRow) -> PathBuf {
    let path = PathBuf::from(&row.path);
    if row.kind == "legacy-command" {
        // A slash command is a single file: adopt *it*, never the whole commands directory.
        path
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or(path)
    }
}

/// A link we did not create records a choice the user already made about where the content lives.
/// Copying such an entry into the store would leave that choice dangling, so binding in place is
/// the default; identical content already in the store is the exception — deduplicating onto the
/// existing slug is strictly cheaper than keeping two bindings.
fn suggest_placement(entry: &EntryState, legacy: bool, duplicate: bool) -> AdoptPlacement {
    match entry {
        EntryState::ForeignLink { .. } if !legacy && !duplicate => AdoptPlacement::LiveBind,
        _ => AdoptPlacement::Replace,
    }
}

/// Who owns the entry on disk *right now*, which is not necessarily who owned it at scan time.
fn entry_state(ctx: &Ctx, content_dir: &Path) -> Result<EntryState> {
    let info = match heron_platform::link::probe(content_dir) {
        Ok(info) => info,
        Err(msg) => return Err(heron_core::HeronError::from(msg)),
    };
    if info.kind.is_link() {
        // The recorded target can be relative; the store check needs the path it really resolves to.
        let resolved = content_dir.canonicalize().ok();
        if resolved
            .as_ref()
            .is_some_and(|p| points_into_store(ctx, p))
        {
            return Ok(EntryState::HeronLink);
        }
        let target = resolved
            .or_else(|| info.target.clone())
            .unwrap_or_else(|| content_dir.to_path_buf());
        return Ok(EntryState::ForeignLink {
            target: target.to_string_lossy().to_string(),
        });
    }
    if matches!(info.kind, LinkKind::Missing) {
        return Ok(EntryState::Missing);
    }
    Ok(EntryState::RealDir)
}

/// Whether `resolved` (already canonical) is one of Heron's own store copies.
///
/// The projection rows are the other half of this answer, but the index can lag a rebuild; the path
/// is the part that cannot lie about who created what.
fn points_into_store(ctx: &Ctx, resolved: &Path) -> bool {
    let Some(store) = ctx.store_root().canonicalize().ok() else {
        return false;
    };
    resolved.starts_with(&store)
}

/// Provenance an installer recorded (or a link implied) must survive adoption. Turning a
/// GitHub-installed entry into an anonymous local copy is what made every adopted skill look
/// unserviceable in `outdated`.
fn record_upstream(ctx: &mut Ctx, slug: &str, upstream: Option<&crate::npx_lock::Upstream>) -> Result<()> {
    let Some(upstream) = upstream else {
        return Ok(());
    };
    let row = SourceBindingRow {
        slug: slug.to_owned(),
        kind: if upstream.url.is_some() {
            "git".to_owned()
        } else {
            "local".to_owned()
        },
        url: upstream.url.clone(),
        ref_name: None,
        subpath: upstream.subpath.clone(),
        pinned_commit: None,
        remote_version: None,
        // The installer's hash covers its own tree layout, not a git revision: keep it as display
        // information only, never as something `upgrade` could merge against.
        content_hash: None,
        is_primary: true,
    };
    ctx.db.write(|tx| binding::replace_primary(tx, &row))
}

/// Adopt · 就地引用: register content where it already is, changing nothing on disk.
///
/// The user's own link keeps pointing at their own directory; the library records that this slug's
/// authoritative content *is* that directory (`skill.storage_path`) and that the entry the client
/// reads is an existing projection of it.
fn live_bind(ctx: &mut Ctx, plan: &AdoptPlan, slug: &str) -> Result<AdoptReport> {
    let entry = entry_path(&plan.detail);
    let EntryState::ForeignLink { target } = &plan.entry else {
        return Err(heron_core::HeronError::from(
            heron_core::ErrorCode::SafetyRefused
                .msg()
                .with("path", entry.display().to_string())
                .with("reason", "live_bind_needs_a_foreign_link"),
        ));
    };
    let real = PathBuf::from(target);
    if !real.is_dir() {
        return Err(heron_core::HeronError::from(
            heron_core::ErrorCode::LinkTargetMissing
                .msg()
                .with("target", real.display().to_string()),
        ));
    }

    let event_id = ctx.db.write(|tx| {
        event::record(
            tx,
            ctx.actor,
            "adopt",
            Some(slug),
            Some(&serde_json::to_string(plan).unwrap_or_default()),
            true,
        )
    })?;

    // 1) The library row, bound to the external directory instead of a store copy.
    let display_name = plan
        .detail
        .observed_slug
        .clone()
        .unwrap_or_else(|| slug.to_owned());
    ctx.db.write(|tx| {
        if skill::get(tx.conn(), slug)?.is_none() {
            skill::insert(
                tx,
                &SkillRow {
                    slug: slug.to_owned(),
                    display_name: display_name.clone(),
                    description: String::new(),
                    tags: vec![],
                    status: "active".to_owned(),
                    origin: "adopted".to_owned(),
                    storage_path: Some(real.to_string_lossy().to_string()),
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )?;
        } else {
            skill::set_storage_path(tx, slug, Some(&real.to_string_lossy()))?;
        }
        Ok(())
    })?;

    // 2) Provenance the installer recorded survives the registration.
    record_upstream(ctx, slug, plan.upstream.as_ref())?;

    // 3) The link the client already reads becomes a known projection, booked against the client
    //    directory it sits in so `verify` judges it against the right scan paths.
    let managed_target = match target::find_for_path(ctx.db.conn(), &entry)? {
        Some(row) => row,
        None => ensure_default_target(ctx, &default_landing(&home_dir()?))?,
    };
    let link_name = format!(
        "{}@{}",
        plan.detail
            .client_id
            .clone()
            .unwrap_or_else(|| "heron".to_owned()),
        heron_platform::paths::project_key(&entry)
    );
    let entry_key = entry.to_string_lossy().to_string();
    let already_projected = projection::list_for_skill(ctx.db.conn(), slug)?
        .into_iter()
        .any(|p| p.path == entry_key);
    ctx.db.write(|tx| {
        if !already_projected {
            projection::insert(
                tx,
                &ProjectionRow {
                    slug: slug.to_owned(),
                    target_id: managed_target.id.clone(),
                    link_name,
                    path: entry_key.clone(),
                    mode: match ProjectionMode::resolve_auto(heron_core::PlatformKind::current()) {
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
        }
        detected::set_state(tx, &plan.detail.path, "adopted")?;
        event::finish(tx, event_id, false)?;
        Ok(())
    })?;

    Ok(AdoptReport {
        adopted: vec![slug.to_owned()],
        skipped: vec![],
        errors: vec![],
    })
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
