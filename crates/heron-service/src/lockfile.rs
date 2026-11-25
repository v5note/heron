//! `heron.lock`: the reproducible skill set for a project (design §7.5).
//!
//! The lock records *what* and *from where*, never absolute paths: another machine (or another
//! OS) must be able to reproduce the same skills. Every entry carries the content hash observed
//! at export time so an import can report drift instead of silently accepting it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Hash};
use serde::{Deserialize, Serialize};

use crate::{Ctx, Result};

pub const LOCK_FILE_NAME: &str = "heron.lock";
pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,
    #[serde(rename = "generatedBy")]
    pub generated_by: String,
    #[serde(default)]
    pub targets: Vec<LockTarget>,
    #[serde(default)]
    pub skills: BTreeMap<String, LockSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockTarget {
    pub kind: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockSkill {
    /// `git` or `local`
    pub source: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "contentHash")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    Missing,
    ContentChanged,
    SourceChanged,
    Untracked,
}

#[derive(Debug, Clone, Serialize)]
pub struct Drift {
    pub slug: String,
    pub kind: DriftKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub installed: Vec<String>,
    pub up_to_date: Vec<String>,
    pub drift: Vec<Drift>,
}

/// Build the lock from what is currently installed.
pub fn export(ctx: &Ctx) -> Result<LockFile> {
    let mut lock = LockFile {
        lockfile_version: LOCK_VERSION,
        generated_by: format!("heron/{}", env!("CARGO_PKG_VERSION")),
        targets: Vec::new(),
        skills: BTreeMap::new(),
    };

    for target in heron_store::repos::target::list(ctx.db.conn())? {
        if target.scope == "project" {
            lock.targets.push(LockTarget {
                kind: target.kind.clone(),
                scope: target.scope.clone(),
            });
        }
    }

    for row in heron_store::repos::skill::list(ctx.db.conn(), false)? {
        let dir = ctx.content_path(&row);
        if !dir.is_dir() {
            continue;
        }
        let meta = heron_vcs::SkillRepo::open(&dir).read_metadata()?;
        let hash = heron_scan::fingerprint_dir(&dir, &heron_scan::FingerprintOptions::default())
            .ok()
            .map(|f| f.hash.to_string());
        let entry = match meta.as_ref().and_then(|m| m.source.clone()) {
            Some(src) => LockSkill {
                source: src.url,
                ref_name: src.ref_name,
                subpath: src.subpath,
                commit: meta.as_ref().map(|m| m.baseline.commit.clone()),
                version: meta.as_ref().map(|m| m.baseline.version.clone()),
                content_hash: hash,
                projection: None,
            },
            None => LockSkill {
                source: "local".to_owned(),
                ref_name: None,
                subpath: None,
                commit: None,
                version: None,
                content_hash: hash,
                projection: None,
            },
        };
        lock.skills.insert(row.slug, entry);
    }
    Ok(lock)
}

/// Write the lock next to a project (`dir/heron.lock`).
pub fn write(target: &Path, lock: &LockFile) -> Result<PathBuf> {
    let path = if target.is_dir() {
        target.join(LOCK_FILE_NAME)
    } else {
        target.to_path_buf()
    };
    let json = serde_json::to_vec_pretty(lock).map_err(|e| {
        heron_core::HeronError::from(ErrorCode::Internal.msg().with("detail", e.to_string()))
    })?;
    heron_platform::atomic::write_atomic(&path, &json)?;
    Ok(path)
}

pub fn read(path: &Path) -> Result<LockFile> {
    let text = heron_platform::atomic::read_to_string_lenient(path)?;
    let lock: LockFile = serde_json::from_str(&text).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::CatalogParse
                .msg()
                .with("file", LOCK_FILE_NAME)
                .with("detail", e.to_string()),
        )
    })?;
    if lock.lockfile_version > LOCK_VERSION {
        return Err(ErrorCode::Internal
            .msg()
            .with("detail", "lockfile_newer_than_heron")
            .with("found", lock.lockfile_version.to_string())
            .into());
    }
    Ok(lock)
}

/// Compare the lock with what is installed locally (no network).
pub fn diff(ctx: &Ctx, lock: &LockFile) -> Result<Vec<Drift>> {
    let mut drift = Vec::new();
    for (slug, entry) in &lock.skills {
        let dir = ctx.content_path_of(slug);
        if !dir.is_dir() {
            drift.push(Drift {
                slug: slug.clone(),
                kind: DriftKind::Missing,
            });
            continue;
        }
        let meta = heron_vcs::SkillRepo::open(&dir).read_metadata()?;
        let local_source = meta.as_ref().and_then(|m| m.source.clone()).map(|s| s.url);
        if entry.source != "local" && local_source.as_deref() != Some(entry.source.as_str()) {
            drift.push(Drift {
                slug: slug.clone(),
                kind: DriftKind::SourceChanged,
            });
            continue;
        }
        if let Some(expected) = entry.content_hash.as_deref() {
            let actual =
                heron_scan::fingerprint_dir(&dir, &heron_scan::FingerprintOptions::default())?;
            if actual.hash.to_string() != expected {
                drift.push(Drift {
                    slug: slug.clone(),
                    kind: DriftKind::ContentChanged,
                });
            }
        }
    }
    Ok(drift)
}

/// Reproduce a lock: install whatever is missing, report what drifts.
pub fn apply(ctx: &mut Ctx, lock: &LockFile) -> Result<ImportReport> {
    let mut report = ImportReport {
        installed: Vec::new(),
        up_to_date: Vec::new(),
        drift: Vec::new(),
    };

    for (slug, entry) in &lock.skills {
        let dir = ctx.content_path_of(slug);
        if dir.is_dir() {
            match entry.content_hash.as_deref() {
                Some(expected) => {
                    let actual = heron_scan::fingerprint_dir(
                        &dir,
                        &heron_scan::FingerprintOptions::default(),
                    )?;
                    if actual.hash.to_string() == expected {
                        report.up_to_date.push(slug.clone());
                    } else {
                        report.drift.push(Drift {
                            slug: slug.clone(),
                            kind: DriftKind::ContentChanged,
                        });
                    }
                }
                None => report.up_to_date.push(slug.clone()),
            }
            continue;
        }

        if entry.source == "local" {
            report.drift.push(Drift {
                slug: slug.clone(),
                kind: DriftKind::Missing,
            });
            continue;
        }

        let request = crate::install::InstallRequest {
            input: entry.source.clone(),
            ref_override: entry.ref_name.clone(),
            subpath_override: entry.subpath.clone(),
            as_slug: Some(slug.clone()),
        };
        match crate::install::install(ctx, &request) {
            Ok(_) => report.installed.push(slug.clone()),
            Err(_) => report.drift.push(Drift {
                slug: slug.clone(),
                kind: DriftKind::Missing,
            }),
        }
    }
    Ok(report)
}

/// Hash helper for callers that only need the string form.
pub fn content_hash(dir: &Path) -> Result<Hash> {
    Ok(heron_scan::fingerprint_dir(dir, &heron_scan::FingerprintOptions::default())?.hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_roundtrips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut skills = BTreeMap::new();
        skills.insert(
            "wechat-mp".to_owned(),
            LockSkill {
                source: "https://github.com/anthropics/skills.git".into(),
                ref_name: Some("main".into()),
                subpath: Some("skills/wechat-mp".into()),
                commit: Some("abc1234".into()),
                version: Some("1.0.0".into()),
                content_hash: Some("blake3:deadbeef".into()),
                projection: None,
            },
        );
        let lock = LockFile {
            lockfile_version: LOCK_VERSION,
            generated_by: "heron/test".into(),
            targets: vec![LockTarget {
                kind: "claude".into(),
                scope: "project".into(),
            }],
            skills,
        };
        let path = write(tmp.path(), &lock).unwrap();
        assert!(path.ends_with(LOCK_FILE_NAME));
        let back = read(&path).unwrap();
        assert_eq!(
            back.skills["wechat-mp"].source,
            lock.skills["wechat-mp"].source
        );
        assert_eq!(back.targets[0].kind, "claude");
    }

    #[test]
    fn newer_lockfiles_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        std::fs::write(
            &path,
            r#"{"lockfileVersion": 99, "generatedBy": "future", "skills": {}}"#,
        )
        .unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.code(), "E_INTERNAL");
    }
}
