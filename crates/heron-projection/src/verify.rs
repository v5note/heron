//! Visibility: would a client actually load this projection? (design §10.11.1)
//!
//! Verification is offline and read-only. It answers four distinct questions that users
//! routinely conflate: *is it there*, *is it in a scanned place*, *is it shadowed*, and
//! *does the running session know about it*.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    /// The projection exists but lives outside every scan path of that client.
    NotScanned,
    /// A same-named skill from a higher-priority source (a plugin) wins.
    Shadowed {
        by: String,
    },
    /// Present and correct, but the client only rescans on session start.
    RestartRequired,
    /// The projection is broken (dangling link / missing directory).
    Broken,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct VerifyInput {
    /// Absolute path of the projection.
    pub path: PathBuf,
    /// Directories this client scans (global absolute + project-relative resolved).
    pub scan_paths: Vec<PathBuf>,
    /// Names claimed by higher-priority sources (plugins) in the same client.
    pub occupied_names: Vec<String>,
    /// True when the link/dir resolves.
    pub resolves: bool,
}

/// Decide visibility for one projection of one client.
pub fn classify(input: &VerifyInput) -> Result<Visibility> {
    if !input.resolves {
        return Ok(Visibility::Broken);
    }
    if !is_scanned(&input.path, &input.scan_paths) {
        return Ok(Visibility::NotScanned);
    }
    if let Some(name) = input
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
    {
        if let Some(by) = input.occupied_names.iter().find(|n| **n == name) {
            return Ok(Visibility::Shadowed { by: by.clone() });
        }
    }
    Ok(Visibility::Visible)
}

/// A projection is scanned when one of its ancestors is a declared scan path.
pub fn is_scanned(path: &Path, scan_paths: &[PathBuf]) -> bool {
    let mut current = Some(path);
    while let Some(dir) = current {
        if scan_paths
            .iter()
            .any(|scan| heron_platform::paths::same_path_ci(scan, dir))
        {
            return true;
        }
        current = dir.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(path: &str, scan: &[&str], occupied: &[&str], resolves: bool) -> VerifyInput {
        VerifyInput {
            path: PathBuf::from(path),
            scan_paths: scan.iter().map(PathBuf::from).collect(),
            occupied_names: occupied.iter().map(|s| (*s).to_owned()).collect(),
            resolves,
        }
    }

    #[test]
    fn visible_when_inside_a_scan_path() {
        let v = classify(&input(
            "/home/u/.heron/skills/pdf",
            &["/home/u/.heron/skills"],
            &[],
            true,
        ))
        .unwrap();
        assert_eq!(v, Visibility::Visible);
    }

    #[test]
    fn not_scanned_outside_every_path() {
        let v = classify(&input(
            "/home/u/store/pdf",
            &["/home/u/.heron/skills"],
            &[],
            true,
        ))
        .unwrap();
        assert_eq!(v, Visibility::NotScanned);
    }

    #[test]
    fn shadowed_by_a_plugin_of_the_same_name() {
        let v = classify(&input(
            "/home/u/.claude/skills/code-review",
            &["/home/u/.claude/skills"],
            &["code-review"],
            true,
        ))
        .unwrap();
        assert_eq!(
            v,
            Visibility::Shadowed {
                by: "code-review".to_owned()
            }
        );
    }

    #[test]
    fn broken_wins_over_everything() {
        let v = classify(&input(
            "/home/u/.claude/skills/pdf",
            &["/home/u/.claude/skills"],
            &["pdf"],
            false,
        ))
        .unwrap();
        assert_eq!(v, Visibility::Broken);
    }

    #[test]
    fn path_comparison_is_case_insensitive() {
        assert!(is_scanned(
            Path::new("/Home/U/.Heron/Skills/pdf"),
            &[PathBuf::from("/home/u/.heron/skills")]
        ));
    }

    #[test]
    fn sibling_prefix_is_not_a_match() {
        assert!(!is_scanned(
            Path::new("/home/u/.heron/skills-backup/pdf"),
            &[PathBuf::from("/home/u/.heron/skills")]
        ));
    }
}
