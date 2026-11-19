//! Environment probing used by the first-run experience.

use std::path::PathBuf;
use std::process::Command;

use heron_core::PlatformKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInfo {
    pub path: PathBuf,
    pub version: String,
    pub major: u32,
    pub minor: u32,
}

impl GitInfo {
    /// `git merge-tree --write-tree` needs git >= 2.38 (implementation doc §4.6).
    pub fn supports_merge_tree(&self) -> bool {
        (self.major, self.minor) >= (2, 38)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub platform: PlatformKind,
    pub git: Option<GitInfo>,
    /// How to install git, chosen per platform (decision D7 — we guide, never silently degrade).
    pub install_hint: InstallHint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallHint {
    /// macOS: ships Command Line Tools.
    MacosXcode {
        command: String,
    },
    /// macOS with Homebrew available.
    MacosHomebrew,
    /// Windows: winget is present on Windows 10 1809+ by default.
    WindowsWinget {
        command: String,
    },
    WindowsDownload {
        url: String,
    },
}

pub const GIT_DOWNLOAD_URL: &str = "https://git-scm.com/downloads";

pub fn preflight() -> Preflight {
    let platform = PlatformKind::current();
    let git = detect_git("git");
    let install_hint = match platform {
        PlatformKind::Macos => InstallHint::MacosXcode {
            command: "xcode-select --install".to_owned(),
        },
        _ => InstallHint::WindowsWinget {
            command: "winget install --id Git.Git -e --source winget".to_owned(),
        },
    };
    Preflight {
        platform,
        git,
        install_hint,
    }
}

/// Run `<git_binary> --version` and parse the version.
pub fn detect_git(git_binary: &str) -> Option<GitInfo> {
    let mut cmd = Command::new(git_binary);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let version = parse_git_version(&raw)?;
    let (major, minor) = parse_major_minor(&version)?;
    Some(GitInfo {
        path: PathBuf::from(git_binary),
        version,
        major,
        minor,
    })
}

/// `git version 2.50.1 (Apple Git-155)` -> `2.50.1`
pub fn parse_git_version(stdout: &str) -> Option<String> {
    let token = stdout
        .split_whitespace()
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let cleaned: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apple_git_version() {
        let v = parse_git_version("git version 2.50.1 (Apple Git-155)").unwrap();
        assert_eq!(v, "2.50.1");
        assert_eq!(parse_major_minor(&v), Some((2, 50)));
    }

    #[test]
    fn parses_windows_git_version() {
        let v = parse_git_version("git version 2.45.1.windows.1").unwrap();
        assert_eq!(parse_major_minor(&v), Some((2, 45)));
    }

    #[test]
    fn garbage_input_is_rejected() {
        assert!(parse_git_version("command not found").is_none());
        assert!(parse_git_version("").is_none());
    }

    #[test]
    fn merge_tree_support_threshold() {
        let old = GitInfo {
            path: "git".into(),
            version: "2.37.0".into(),
            major: 2,
            minor: 37,
        };
        let new = GitInfo {
            path: "git".into(),
            version: "2.38.0".into(),
            major: 2,
            minor: 38,
        };
        assert!(!old.supports_merge_tree());
        assert!(new.supports_merge_tree());
    }

    #[test]
    fn preflight_never_panics_without_git() {
        let p = preflight();
        assert_eq!(p.platform, PlatformKind::current());
        match p.platform {
            PlatformKind::Macos => {
                assert!(matches!(p.install_hint, InstallHint::MacosXcode { .. }))
            }
            _ => assert!(matches!(p.install_hint, InstallHint::WindowsWinget { .. })),
        }
    }

    #[test]
    fn missing_git_binary_is_reported_as_none() {
        assert!(detect_git("heron-definitely-not-a-real-git-binary").is_none());
    }
}
