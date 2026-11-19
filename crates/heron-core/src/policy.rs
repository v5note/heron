//! Policies that shape behaviour without touching IO.

use serde::{Deserialize, Serialize};

use crate::platform_kind::PlatformKind;

/// How far an upgrade may move (`heron upgrade` / `outdated`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PinPolicy {
    /// Never upgrade automatically and never report as outdated.
    Exact,
    /// Upgrade within the same major version (default).
    #[default]
    Semver,
    /// Track the head of the configured ref.
    None,
}

impl PinPolicy {
    pub fn lane(self) -> UpgradeLane {
        match self {
            PinPolicy::Exact => UpgradeLane::Exact,
            PinPolicy::Semver => UpgradeLane::SameMajor,
            PinPolicy::None => UpgradeLane::Any,
        }
    }

    pub fn is_pinned(self) -> bool {
        matches!(self, PinPolicy::Exact)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeLane {
    Exact,
    SameMajor,
    Any,
}

/// What to do when a projection target already holds a different copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    /// Ask the user (default).
    #[default]
    Ask,
    /// Keep whatever is already on disk.
    KeepOld,
    /// Replace it after taking a backup.
    Overwrite,
    /// Install under a suffixed name instead.
    SideBySide,
}

/// How the store is materialized into a target directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionMode {
    /// Physical copy; works with every scanner, needs resync.
    Copy,
    /// Symbolic link (macOS/Linux default; needs Developer Mode on Windows).
    Symlink,
    /// NTFS directory junction (Windows default; no privileges needed).
    Junction,
    /// A tiny SKILL.md that points at the store copy.
    Pointer,
}

impl ProjectionMode {
    /// Platform default (design ADR-7 / implementation doc §3.1).
    pub const fn resolve_auto(platform: PlatformKind) -> Self {
        match platform {
            PlatformKind::Windows => ProjectionMode::Junction,
            PlatformKind::Macos | PlatformKind::Linux => ProjectionMode::Symlink,
        }
    }

    /// Parse a user-supplied mode; `auto` resolves against the given platform.
    pub fn resolve(self, platform: PlatformKind) -> Self {
        match self {
            ProjectionMode::Copy
            | ProjectionMode::Symlink
            | ProjectionMode::Junction
            | ProjectionMode::Pointer => self,
            #[allow(unreachable_patterns)]
            _ => Self::resolve_auto(platform),
        }
    }

    pub fn is_link(self) -> bool {
        matches!(self, ProjectionMode::Symlink | ProjectionMode::Junction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionModePreference {
    #[default]
    Auto,
    Copy,
    Symlink,
    Junction,
    Pointer,
}

impl ProjectionModePreference {
    pub fn resolve(self, platform: PlatformKind) -> ProjectionMode {
        match self {
            ProjectionModePreference::Auto => ProjectionMode::resolve_auto(platform),
            ProjectionModePreference::Copy => ProjectionMode::Copy,
            ProjectionModePreference::Symlink => ProjectionMode::Symlink,
            ProjectionModePreference::Junction => ProjectionMode::Junction,
            ProjectionModePreference::Pointer => ProjectionMode::Pointer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_mode_follows_platform() {
        assert_eq!(
            ProjectionMode::resolve_auto(PlatformKind::Windows),
            ProjectionMode::Junction
        );
        assert_eq!(
            ProjectionMode::resolve_auto(PlatformKind::Macos),
            ProjectionMode::Symlink
        );
        assert_eq!(
            ProjectionMode::resolve_auto(PlatformKind::Linux),
            ProjectionMode::Symlink
        );
    }

    #[test]
    fn preference_auto_resolves_but_explicit_does_not() {
        let p = PlatformKind::Windows;
        assert_eq!(
            ProjectionModePreference::Auto.resolve(p),
            ProjectionMode::Junction
        );
        assert_eq!(
            ProjectionModePreference::Copy.resolve(p),
            ProjectionMode::Copy
        );
        assert_eq!(
            ProjectionModePreference::Symlink.resolve(p),
            ProjectionMode::Symlink
        );
    }

    #[test]
    fn pin_lanes() {
        assert_eq!(PinPolicy::Exact.lane(), UpgradeLane::Exact);
        assert_eq!(PinPolicy::Semver.lane(), UpgradeLane::SameMajor);
        assert_eq!(PinPolicy::None.lane(), UpgradeLane::Any);
        assert!(PinPolicy::Exact.is_pinned());
    }

    #[test]
    fn link_modes() {
        assert!(ProjectionMode::Symlink.is_link());
        assert!(ProjectionMode::Junction.is_link());
        assert!(!ProjectionMode::Copy.is_link());
        assert!(!ProjectionMode::Pointer.is_link());
    }

    #[test]
    fn serde_names_are_stable_wire_values() {
        assert_eq!(
            serde_json::to_string(&PinPolicy::Semver).unwrap(),
            "\"semver\""
        );
        assert_eq!(
            serde_json::to_string(&ConflictPolicy::SideBySide).unwrap(),
            "\"side-by-side\""
        );
        assert_eq!(
            serde_json::to_string(&ProjectionMode::Junction).unwrap(),
            "\"junction\""
        );
    }
}
