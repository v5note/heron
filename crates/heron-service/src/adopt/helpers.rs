//! In-place live-bind adoption, landing target booking, and filesystem/hashing helpers.

use std::path::{Path, PathBuf};

use heron_core::{Hash, ProjectionMode};
use heron_scan::FingerprintOptions;
use heron_store::models::{DetectedRow, ProjectionRow, SkillRow, TargetRow};
use heron_store::repos::{detected, event, projection, skill, target};

use super::{description_of, entry_path, record_upstream, AdoptPlan, AdoptReport, EntryState};
use crate::{Ctx, Result};

/// Adopt · 就地引用: register content where it already is, changing nothing on disk.
///
/// The user's own link keeps pointing at their own directory; the library records that this slug's
/// authoritative content *is* that directory (`skill.storage_path`) and that the entry the client
/// reads is an existing projection of it.
pub(super) fn live_bind(ctx: &mut Ctx, plan: &AdoptPlan, slug: &str) -> Result<AdoptReport> {
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
    // 就地引用 keeps the user's own directory as the content, so that is the file to read from.
    let description = description_of(&real.join("SKILL.md"));
    ctx.db.write(|tx| {
        if skill::get(tx.conn(), slug)?.is_none() {
            skill::insert(
                tx,
                &SkillRow {
                    slug: slug.to_owned(),
                    display_name: display_name.clone(),
                    description,
                    tags: vec![],
                    status: "active".to_owned(),
                    origin: "adopted".to_owned(),
                    storage_path: Some(real.to_string_lossy().to_string()),
                    subpath: None,
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
    let booking = booking_client(ctx, &plan.detail, &entry)?;
    let managed_target = match target::find_for_path(ctx.db.conn(), &entry)? {
        Some(row) => row,
        None => ensure_target_for_dir(ctx, &entry, &booking)?,
    };
    // Same rule as the copy placement: the prefix names the client that owns the directory the
    // link sits in, which is the target the row is booked against.
    let link_name = format!(
        "{}@{}",
        managed_target.kind,
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

/// Register the directory a link actually sits in as a target, reusing it when it is already known.
///
/// There is no default target any more (the store is not a projection destination), so an entry
/// found in a directory the client matrix does not know has to be booked against *that* directory:
/// `verify` then judges it against the right paths and `unproject --on` finds the link.
///
/// A landing point is only worth registering if some client actually reads it. The scanner reports
/// the *resolved* directory, so an entry living behind somebody else's link comes back as a path
/// inside their own checkout (`~/.agents/huashu-motion/skills`, say) — a place no client scans, that
/// disappears with the third-party repository and leaves a broken link behind as its projection. So
/// when the directory is not one of the client's declared skill roots, the entry is booked against
/// the landing point that client really has.
pub(super) fn ensure_target_for_dir(
    ctx: &mut Ctx,
    dest_link: &Path,
    client_kind: &str,
) -> Result<TargetRow> {
    let dir = dest_link
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dest_link.to_path_buf());
    if let Some(existing) = target::find_for_path(ctx.db.conn(), dest_link)? {
        return Ok(existing);
    }
    if !is_declared_skill_root(ctx, client_kind, &dir)? {
        if let Some(global) = target::get(ctx.db.conn(), &format!("{client_kind}:global"))? {
            return Ok(global);
        }
    }
    let row = TargetRow {
        id: format!("{client_kind}:{}", heron_platform::paths::project_key(&dir)),
        kind: client_kind.to_owned(),
        scope: "global".to_owned(),
        project_path: None,
        path: dir.to_string_lossy().to_string(),
        enabled: true,
        is_default: false,
        discover: "manual".to_owned(),
    };
    ctx.db.write(|tx| target::upsert(tx, &row))?;
    Ok(row)
}

/// Which client's directory a projection of this landing point should be booked against.
///
/// A row discovered in a client's skills directory names that client. A row that lives behind a
/// user-made link out of one names none — the content is elsewhere, so the GUI files it under
///「其他」— but the link itself is still the only evidence of which client the user wired up, and
/// that is the target worth booking against, because it is where the client will read the skill
/// from afterwards.
pub(super) fn booking_client(ctx: &Ctx, row: &DetectedRow, landing: &Path) -> Result<String> {
    if let Some(kind) = row.client_id.as_deref() {
        if kind != heron_scan::classify::CUSTOM_CLIENT_ID {
            return Ok(kind.to_owned());
        }
    }
    Ok(declared_link_owner(ctx, landing)?.unwrap_or_else(|| "custom".to_owned()))
}

/// The client that has a skills directory containing a link named after `landing` and resolving to
/// it. Read-only: it probes what is already on disk and changes nothing.
fn declared_link_owner(ctx: &Ctx, landing: &Path) -> Result<Option<String>> {
    let Some(name) = landing
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return Ok(None);
    };
    let landing = landing
        .canonicalize()
        .unwrap_or_else(|_| landing.to_path_buf());
    for spec in crate::clients::loaded_specs(&ctx.db)? {
        for root in &spec.global_paths {
            let candidate = root.join(&name);
            if !heron_platform::link::probe(&candidate)?.kind.is_link() {
                continue;
            }
            if candidate.canonicalize().ok().as_deref() == Some(landing.as_path()) {
                return Ok(Some(spec.id));
            }
        }
    }
    Ok(None)
}

/// Is `dir` somewhere `client_kind` genuinely reads skills: its global root, or a project's?
///
/// Unknown clients (`custom`) have no declared roots and therefore no claim to one, which leaves
/// their directory registered as its own landing point — the only place that link can live.
fn is_declared_skill_root(ctx: &Ctx, client_kind: &str, dir: &Path) -> Result<bool> {
    let specs = crate::clients::loaded_specs(&ctx.db)?;
    let Some(spec) = specs.into_iter().find(|s| s.id == client_kind) else {
        return Ok(false);
    };
    if spec
        .global_paths
        .iter()
        .any(|g| heron_platform::paths::same_path_ci(g, dir))
    {
        return Ok(true);
    }
    Ok(spec
        .project_paths
        .iter()
        .any(|p| ends_with_components(dir, p)))
}

/// Does `path` end with the components of a relative skill root (`<repo>/.claude/skills`)?
fn ends_with_components(path: &Path, suffix: &Path) -> bool {
    let tail: Vec<_> = suffix.components().collect();
    if tail.is_empty() {
        return false;
    }
    let all: Vec<_> = path.components().collect();
    all.len() >= tail.len() && all[all.len() - tail.len()..] == tail
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
pub(super) fn single_file_hash(path: &Path) -> Result<Hash> {
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
