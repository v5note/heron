//! Version comparison.
//!
//! Skills rarely carry a version. The design (ADR-5, §10.4) therefore uses a layered
//! scheme: catalog version → semver git tag → `metadata.version` → `0.0.0+<short sha>`.
//! The fallback is intentionally *never* silently equal to a real release.

use std::cmp::Ordering;
use std::fmt;

use semver::Version as SemVer;
use serde::{Deserialize, Serialize};

use crate::policy::UpgradeLane;

/// Shortest useful commit prefix used in fallback versions.
pub const SHORT_SHA_LEN: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Version {
    /// A real semantic version coming from a tag, catalog entry or metadata.
    Semver { value: String },
    /// No version anywhere: `0.0.0+<short sha>`.
    Fallback { sha: String },
    /// Something is present but is not semver (e.g. `metadata.version = "42"`).
    Opaque { raw: String },
}

impl Version {
    /// Parse whatever a source declared. `sha_hint` feeds the fallback variant.
    pub fn parse(raw: &str, sha_hint: Option<&str>) -> Self {
        let trimmed = raw.trim().trim_start_matches('v');
        if let Ok(v) = SemVer::parse(trimmed) {
            return Version::Semver {
                value: v.to_string(),
            };
        }
        match sha_hint {
            Some(sha) if !sha.is_empty() => Version::Fallback {
                sha: short_sha(sha).to_owned(),
            },
            _ => Version::Opaque {
                raw: raw.trim().to_owned(),
            },
        }
    }

    /// The no-version case: only a commit sha is known.
    pub fn from_sha(sha: &str) -> Self {
        Version::Fallback {
            sha: short_sha(sha).to_owned(),
        }
    }

    /// Version of a skill that has never been released.
    pub const ZERO: &'static str = "0.0.0";

    pub fn semver(&self) -> Option<SemVer> {
        match self {
            Version::Semver { value } => SemVer::parse(value).ok(),
            Version::Fallback { sha } => SemVer::parse(&format!("{}+{sha}", Version::ZERO)).ok(),
            Version::Opaque { .. } => None,
        }
    }

    /// Whether `self` should be offered as an upgrade over `installed` under `lane`.
    ///
    /// Rules:
    /// - identical wire form -> never upgradable (this is what prevents false "outdated").
    /// - any fallback (no real version upstream) -> never upgradable automatically; git
    ///   commit drift is reported separately by the service layer.
    /// - opaque versions are only compared when string-equal.
    pub fn is_upgrade_from(&self, installed: &Version, lane: UpgradeLane) -> bool {
        if self == installed {
            return false;
        }
        match (self.semver(), installed.semver()) {
            (Some(new), Some(old)) if !self.is_fallback() && !installed.is_fallback() => match lane
            {
                UpgradeLane::Exact => false,
                UpgradeLane::SameMajor => new.major == old.major && new > old,
                UpgradeLane::Any => new > old,
            },
            _ => false,
        }
    }

    pub fn is_fallback(&self) -> bool {
        matches!(self, Version::Fallback { .. })
    }

    pub fn as_tag(&self) -> String {
        match self {
            Version::Semver { value } => format!("v{value}"),
            Version::Fallback { sha } => format!("v{}+{sha}", Version::ZERO),
            Version::Opaque { raw } => format!("v-opaque-{}", sanitize(raw)),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Version::Semver { value } => f.write_str(value),
            Version::Fallback { sha } => write!(f, "{}+{sha}", Version::ZERO),
            Version::Opaque { raw } => f.write_str(raw),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// Total order used only for sorting listings; `is_upgrade_from` is the authority.
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.semver(), other.semver()) {
            (Some(a), Some(b)) => a.cmp(&b).then_with(|| self.rank().cmp(&other.rank())),
            _ => self
                .rank()
                .cmp(&other.rank())
                .then_with(|| self.to_string().cmp(&other.to_string())),
        }
    }
}

impl Version {
    fn rank(&self) -> u8 {
        match self {
            Version::Semver { .. } => 0,
            Version::Fallback { .. } => 1,
            Version::Opaque { .. } => 2,
        }
    }
}

fn short_sha(sha: &str) -> &str {
    if sha.len() > SHORT_SHA_LEN {
        &sha[..SHORT_SHA_LEN]
    } else {
        sha
    }
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_semver_with_optional_v_prefix() {
        assert_eq!(Version::parse("v1.2.3", None).to_string(), "1.2.3");
        assert_eq!(Version::parse("2.0.0", None).to_string(), "2.0.0");
    }

    #[test]
    fn falls_back_to_short_sha() {
        let v = Version::parse("", Some("0123456789abcdef"));
        assert_eq!(v.to_string(), "0.0.0+0123456");
        assert!(v.is_fallback());
    }

    #[test]
    fn opaque_versions_are_preserved() {
        let v = Version::parse("42", None);
        assert!(matches!(v, Version::Opaque { .. }));
        assert_eq!(v.to_string(), "42");
        assert_eq!(v.as_tag(), "v-opaque-42");
    }

    #[test]
    fn same_major_lane_blocks_major_bumps() {
        let new = Version::parse("2.0.0", None);
        let old = Version::parse("1.4.0", None);
        assert!(!new.is_upgrade_from(&old, UpgradeLane::SameMajor));
        assert!(new.is_upgrade_from(&old, UpgradeLane::Any));
    }

    #[test]
    fn patch_upgrade_allowed_in_same_major() {
        let new = Version::parse("1.5.0", None);
        let old = Version::parse("1.4.0", None);
        assert!(new.is_upgrade_from(&old, UpgradeLane::SameMajor));
    }

    #[test]
    fn identical_versions_are_never_upgradable() {
        let v = Version::parse("1.4.0", None);
        assert!(!v.is_upgrade_from(&v, UpgradeLane::Any));
    }

    #[test]
    fn exact_lane_never_upgrades() {
        let new = Version::parse("1.5.0", None);
        let old = Version::parse("1.4.0", None);
        assert!(!new.is_upgrade_from(&old, UpgradeLane::Exact));
    }

    #[test]
    fn fallback_alongside_semver_is_not_an_upgrade() {
        // Upstream lost its version: do not pretend it is newer or older.
        let new = Version::from_sha("abcdef1234");
        let old = Version::parse("1.4.0", None);
        assert!(!new.is_upgrade_from(&old, UpgradeLane::Any));
        assert!(!old.is_upgrade_from(&new, UpgradeLane::Any));
    }

    #[test]
    fn downgrade_is_never_an_upgrade() {
        let new = Version::parse("1.3.0", None);
        let old = Version::parse("1.4.0", None);
        assert!(!new.is_upgrade_from(&old, UpgradeLane::Any));
    }

    #[test]
    fn tag_formatting() {
        assert_eq!(Version::parse("1.0.0", None).as_tag(), "v1.0.0");
        assert_eq!(Version::from_sha("deadbeef00").as_tag(), "v0.0.0+deadbee");
    }

    #[test]
    fn serde_roundtrip() {
        for v in [
            Version::parse("1.2.3", None),
            Version::from_sha("abcdef1234"),
            Version::parse("42", None),
        ] {
            let json = serde_json::to_string(&v).unwrap();
            let back: Version = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }
}
