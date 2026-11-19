//! Normalized content fingerprints (design §10.3).
//!
//! Rules baked in here so every consumer agrees byte-for-byte:
//! * relative paths with forward slashes, sorted;
//! * text bytes normalized (BOM stripped, CRLF/CR -> LF);
//! * noise files (.git, .DS_Store, Thumbs.db, .heron, heron.skill.json) skipped;
//! * links hashed as `L:<target string>`, so a link never causes recursion.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Hash, Result};
use serde::Serialize;
use walkdir::WalkDir;

use crate::rules::IgnoreRules;

#[derive(Debug, Clone, Serialize)]
pub struct Fingerprint {
    pub hash: Hash,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct FingerprintOptions {
    /// Also ignore user-configured globs (same set the scanner uses).
    pub extra_excludes: Vec<String>,
    /// Stop after this many files (safety valve for pathological folders).
    pub max_files: usize,
}

impl Default for FingerprintOptions {
    fn default() -> Self {
        Self {
            extra_excludes: Vec::new(),
            max_files: 10_000,
        }
    }
}

/// Hash the directory rooted at `root` (the directory containing `SKILL.md`).
pub fn fingerprint_dir(root: &Path, opts: &FingerprintOptions) -> Result<Fingerprint> {
    let rules = IgnoreRules::new(&opts.extra_excludes)?;
    let entries = collect(root, &rules)?;

    let mut hasher = blake3::Hasher::new();
    let mut bytes_total = 0u64;
    for entry in &entries {
        hasher.update(entry.rel_path.as_bytes());
        hasher.update(&[0]);
        match &entry.content {
            Content::Bytes(bytes) => {
                bytes_total += bytes.len() as u64;
                hasher.update(&(bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
            Content::LinkTarget(target) => {
                hasher.update(b"L:");
                hasher.update(target.as_bytes());
            }
        }
        hasher.update(&[0xff]);
    }
    Ok(Fingerprint {
        hash: Hash::from_hex(hasher.finalize().to_hex().to_string()),
        files: entries.len(),
        bytes: bytes_total,
    })
}

enum Content {
    Bytes(Vec<u8>),
    LinkTarget(String),
}

struct Entry {
    rel_path: String,
    content: Content,
}

fn collect(root: &Path, rules: &IgnoreRules) -> Result<Vec<Entry>> {
    if !root.is_dir() {
        return Err(ErrorCode::ScanRootMissing
            .msg()
            .with("path", root.display().to_string())
            .into());
    }
    let mut entries = Vec::new();
    for walk in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = walk.map_err(|e| {
            ErrorCode::Permission
                .msg()
                .with("detail", e.to_string())
                .with("path", root.display().to_string())
        })?;
        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = relative(path, root)?;
        if heron_core::hash::is_ignored(&rel) || rules.is_ignored(path) {
            continue;
        }
        if IgnoreRules::is_noise_file(entry.file_name().to_string_lossy().as_ref()) {
            continue;
        }
        let file_type = entry.file_type();
        if file_type.is_dir() {
            continue;
        }
        if file_type.is_symlink() {
            let target = std::fs::read_link(path)
                .map(|t| t.to_string_lossy().replace('\\', "/"))
                .map_err(|e| {
                    heron_core::HeronError::from(
                        ErrorCode::Permission
                            .msg()
                            .with("detail", e.to_string())
                            .with("path", path.display().to_string()),
                    )
                })?;
            entries.push(Entry {
                rel_path: rel,
                content: Content::LinkTarget(target),
            });
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let raw = std::fs::read(path).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("detail", e.to_string())
                    .with("path", path.display().to_string()),
            )
        })?;
        let normalized = if looks_binary(&raw) {
            raw
        } else {
            heron_core::hash::normalize_text_bytes(&raw)
        };
        entries.push(Entry {
            rel_path: rel,
            content: Content::Bytes(normalized),
        });
    }
    entries.sort_by(|a, b| a.rel_path.as_bytes().cmp(b.rel_path.as_bytes()));
    Ok(entries)
}

fn relative(path: &Path, root: &Path) -> Result<String> {
    let stripped = path.strip_prefix(root).unwrap_or(path);
    let mut s = stripped.to_string_lossy().replace('\\', "/");
    s = s.trim_start_matches('/').to_owned();
    if s.is_empty() {
        return Err(ErrorCode::Internal
            .msg()
            .with("detail", "empty relative path")
            .into());
    }
    Ok(s)
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// Convenience for tests and callers that only need the hex string.
pub fn fingerprint_hex(root: &Path) -> Result<String> {
    Ok(fingerprint_dir(root, &FingerprintOptions::default())?
        .hash
        .hex()
        .to_owned())
}

/// Helper used by the scanner to fingerprint a `PathBuf` without importing options everywhere.
pub fn fingerprint_default(root: &Path) -> Result<Hash> {
    Ok(fingerprint_dir(root, &FingerprintOptions::default())?.hash)
}

#[allow(dead_code)]
fn _assert_pathbuf(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn identical_trees_hash_identically_regardless_of_newlines() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(a.path(), "SKILL.md", "line1\nline2\n");
        write(b.path(), "SKILL.md", "line1\r\nline2\r\n");
        assert_eq!(
            fingerprint_hex(a.path()).unwrap(),
            fingerprint_hex(b.path()).unwrap()
        );
    }

    #[test]
    fn content_change_changes_hash() {
        let a = tempfile::tempdir().unwrap();
        let h1 = {
            write(a.path(), "SKILL.md", "v1");
            fingerprint_hex(a.path()).unwrap()
        };
        write(a.path(), "SKILL.md", "v2");
        assert_ne!(h1, fingerprint_hex(a.path()).unwrap());
    }

    #[test]
    fn path_rename_changes_hash() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "references/a.md", "x");
        let h1 = fingerprint_hex(a.path()).unwrap();
        fs::rename(
            a.path().join("references/a.md"),
            a.path().join("references/b.md"),
        )
        .unwrap();
        assert_ne!(h1, fingerprint_hex(a.path()).unwrap());
    }

    #[test]
    fn noise_is_ignored() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "SKILL.md", "x");
        let clean = fingerprint_hex(a.path()).unwrap();
        write(a.path(), ".DS_Store", "junk");
        write(a.path(), ".git/config", "junk");
        write(a.path(), "Thumbs.db", "junk");
        write(a.path(), "heron.skill.json", "{}");
        assert_eq!(clean, fingerprint_hex(a.path()).unwrap());
    }

    #[test]
    fn counts_files_and_bytes() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "SKILL.md", "abcd");
        write(a.path(), "scripts/run.py", "xy");
        let fp = fingerprint_dir(a.path(), &FingerprintOptions::default()).unwrap();
        assert_eq!(fp.files, 2);
        assert_eq!(fp.bytes, 6);
        assert!(fp.hash.as_str().starts_with("blake3:"));
    }

    #[test]
    fn missing_root_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let err =
            fingerprint_dir(&tmp.path().join("nope"), &FingerprintOptions::default()).unwrap_err();
        assert_eq!(err.code(), "E_SCAN_ROOT_MISSING");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_hashed_by_target_and_never_followed() {
        use std::os::unix::fs::symlink;
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "SKILL.md", "x");
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.txt", "huge");
        symlink(outside.path(), a.path().join("loop")).unwrap();
        let fp = fingerprint_dir(a.path(), &FingerprintOptions::default()).unwrap();
        assert_eq!(
            fp.files, 2,
            "the link counts as one entry, its target is not walked"
        );
        let h1 = fp.hash;
        // Same target string => same hash; different target => different hash.
        fs::remove_file(a.path().join("loop")).unwrap();
        symlink("/tmp/other", a.path().join("loop")).unwrap();
        assert_ne!(
            h1,
            fingerprint_dir(a.path(), &FingerprintOptions::default())
                .unwrap()
                .hash
        );
    }
}
