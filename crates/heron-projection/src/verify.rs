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
    /// Would be `Visible`, but this client only rescans at session start and the projection
    /// has not been confirmed by an earlier verify — a running session cannot know it yet.
    RestartRequired,
    /// The projection is broken (dangling link / missing directory).
    Broken,
    Unknown,
}

impl Visibility {
    /// The stable token shared by the `visibility` table and the UI contract.
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Visible => "visible",
            Visibility::NotScanned => "not_scanned",
            Visibility::Shadowed { .. } => "shadowed",
            Visibility::RestartRequired => "restart_required",
            Visibility::Broken => "broken",
            Visibility::Unknown => "unknown",
        }
    }
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
    /// The client re-reads its skill directories only when a new session opens (`rescan = "session"`).
    pub rescans_on_session: bool,
    /// No earlier verify has confirmed this landing (row missing, or link newer than the last check).
    pub landing_unconfirmed: bool,
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
    if input.rescans_on_session && input.landing_unconfirmed {
        return Ok(Visibility::RestartRequired);
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
            rescans_on_session: false,
            landing_unconfirmed: false,
        }
    }

    #[test]
    fn visible_when_inside_a_scan_path() {
        let v = classify(&input(
            "/home/u/.heron/skills/wechat-mp",
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
            "/home/u/store/wechat-mp",
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
            "/home/u/.claude/skills/wechat-mp",
            &["/home/u/.claude/skills"],
            &["wechat-mp"],
            false,
        ))
        .unwrap();
        assert_eq!(v, Visibility::Broken);
    }

    #[test]
    fn session_rescan_client_reports_restart_until_confirmed() {
        let mut i = input(
            "/home/u/.claude/skills/wechat-mp",
            &["/home/u/.claude/skills"],
            &[],
            true,
        );
        i.rescans_on_session = true;
        i.landing_unconfirmed = true;
        assert_eq!(classify(&i).unwrap(), Visibility::RestartRequired);
        // Once an earlier verify confirmed the landing, the same input reads as visible.
        i.landing_unconfirmed = false;
        assert_eq!(classify(&i).unwrap(), Visibility::Visible);
        // A client that hot-reloads (watch) never owes the user a restart.
        let mut w = input(
            "/home/u/.agents/skills/wechat-mp",
            &["/home/u/.agents/skills"],
            &[],
            true,
        );
        w.rescans_on_session = false;
        w.landing_unconfirmed = true;
        assert_eq!(classify(&w).unwrap(), Visibility::Visible);
    }

    #[test]
    fn restart_never_outranks_broken_not_scanned_or_shadowed() {
        let mk = |path: &str, occupied: &[&str], resolves: bool| {
            let mut i = input(path, &["/home/u/.claude/skills"], occupied, resolves);
            i.rescans_on_session = true;
            i.landing_unconfirmed = true;
            i
        };
        assert_eq!(
            classify(&mk("/home/u/.claude/skills/wechat-mp", &[], false)).unwrap(),
            Visibility::Broken
        );
        assert_eq!(
            classify(&mk("/home/u/store/wechat-mp", &[], true)).unwrap(),
            Visibility::NotScanned
        );
        assert_eq!(
            classify(&mk(
                "/home/u/.claude/skills/wechat-mp",
                &["wechat-mp"],
                true
            ))
            .unwrap(),
            Visibility::Shadowed {
                by: "wechat-mp".to_owned()
            }
        );
    }

    #[test]
    fn path_comparison_is_case_insensitive() {
        assert!(is_scanned(
            Path::new("/Home/U/.Heron/Skills/wechat-mp"),
            &[PathBuf::from("/home/u/.heron/skills")]
        ));
    }

    #[test]
    fn sibling_prefix_is_not_a_match() {
        assert!(!is_scanned(
            Path::new("/home/u/.heron/skills-backup/wechat-mp"),
            &[PathBuf::from("/home/u/.heron/skills")]
        ));
    }
}
