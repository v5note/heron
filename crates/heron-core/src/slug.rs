//! Skill identity.
//!
//! A slug is derived from the frontmatter `name`, falling back to the directory name,
//! and is unique **case-insensitively** across the library (Windows semantics apply on
//! every platform so a library copied between macOS and Windows keeps working).

use std::fmt;

use serde::Deserialize;

use crate::error::{ErrorCode, Msg};

pub const MAX_SLUG_LEN: usize = 64;

/// Windows device names that cannot be used as directory names.
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlugPolicy {
    /// Reject anything that could not be created on every supported platform.
    Strict,
    /// Repair what can be repaired (transliterate-free: lowercase, collapse separators).
    Lenient,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Slug(String);

impl Slug {
    /// Derive a slug from a raw candidate (frontmatter name, directory name, or a
    /// legacy command file stem) and apply the given policy.
    pub fn derive(raw: &str, policy: SlugPolicy) -> Result<Self, Msg> {
        let normalized = normalize(raw);
        if normalized.is_empty() {
            return Err(ErrorCode::SlugInvalid
                .msg()
                .with("raw", raw.to_owned())
                .with("reason", "empty"));
        }

        let candidate = if normalized.len() > MAX_SLUG_LEN {
            match policy {
                SlugPolicy::Strict => {
                    return Err(ErrorCode::SlugInvalid
                        .msg()
                        .with("raw", raw.to_owned())
                        .with("reason", "too_long"));
                }
                SlugPolicy::Lenient => normalized[..MAX_SLUG_LEN].trim_end_matches('-').to_owned(),
            }
        } else {
            normalized
        };

        let slug = Slug(candidate);
        slug.validate_for_policy(policy)?;
        Ok(slug)
    }

    /// Wraps an already-normalized string. Fails when normalization would change it.
    pub fn parse_exact(raw: &str) -> Result<Self, Msg> {
        if normalize(raw) != raw {
            return Err(ErrorCode::SlugInvalid
                .msg()
                .with("raw", raw.to_owned())
                .with("reason", "not_normalized"));
        }
        Self::derive(raw, SlugPolicy::Strict)
    }

    fn validate_for_policy(&self, policy: SlugPolicy) -> Result<(), Msg> {
        if is_reserved(&self.0) {
            return Err(ErrorCode::ReservedName
                .msg()
                .with("slug", self.0.clone())
                .with("reason", "windows_device_name"));
        }
        let reasons = platform_hazards(&self.0);
        if reasons.is_empty() || policy == SlugPolicy::Lenient && !has_fatal_hazard(&self.0) {
            return Ok(());
        }
        Err(ErrorCode::SlugInvalid
            .msg()
            .with("raw", self.0.clone())
            .with("reason", reasons.join(",")))
    }

    /// Check that the slug can be used as a directory name on every supported platform.
    /// The library always validates against the strictest platform (Windows) so a
    /// library can be moved between machines.
    pub fn validate_platform(&self) -> Result<(), Msg> {
        let reasons = platform_hazards(&self.0);
        if reasons.is_empty() {
            Ok(())
        } else {
            Err(ErrorCode::SlugInvalid
                .msg()
                .with("raw", self.0.clone())
                .with("reason", reasons.join(",")))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Case-insensitive comparison; the only comparison used for uniqueness.
    pub fn eq_ci(&self, other: &Slug) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }

    pub fn key_ci(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Slug {
    type Error = Msg;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Slug::parse_exact(&value)
    }
}

impl From<Slug> for String {
    fn from(value: Slug) -> Self {
        value.0.clone()
    }
}

impl serde::Serialize for Slug {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

/// Lowercase, trim, replace any run of invalid characters with a single hyphen and
/// collapse hyphens at the edges. Unicode letters are allowed by the Agent Skills spec
/// only as ASCII, so non-ASCII letters are dropped here and reported by the validator.
fn normalize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_dash = false;
    for ch in raw.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | ' ' | '.') {
            Some('-')
        } else if ch.is_ascii() {
            None
        } else {
            // Non-ASCII: transliterate nothing, but keep the word boundary.
            Some('-')
        };
        match mapped {
            Some('-') if out.is_empty() || last_was_dash => {}
            Some('-') => {
                out.push('-');
                last_was_dash = true;
            }
            Some(c) => {
                out.push(c);
                last_was_dash = false;
            }
            None => {}
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn is_reserved(slug: &str) -> bool {
    WINDOWS_RESERVED.contains(&slug)
        || WINDOWS_RESERVED.contains(&slug.to_ascii_lowercase().as_str())
}

/// Windows-invalid characters plus trailing dot/space, which cannot be truncated away.
fn platform_hazards(slug: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if slug.is_empty() {
        out.push("empty");
    }
    if slug.len() > MAX_SLUG_LEN {
        out.push("too_long");
    }
    if slug.ends_with('.') || slug.ends_with(' ') {
        out.push("trailing_dot_or_space");
    }
    if slug
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        || slug.chars().any(|c| (c as u32) < 0x20)
    {
        out.push("windows_invalid_char");
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        out.push("outside_charset");
    }
    out
}

fn has_fatal_hazard(slug: &str) -> bool {
    slug.is_empty()
        || slug.ends_with('.')
        || slug.ends_with(' ')
        || slug.chars().any(|c| (c as u32) < 0x20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_spaces_underscores_and_case() {
        assert_eq!(
            Slug::derive("PDF Toolkit", SlugPolicy::Strict)
                .unwrap()
                .as_str(),
            "pdf-toolkit"
        );
        assert_eq!(
            Slug::derive("  Huashu_Design  ", SlugPolicy::Strict)
                .unwrap()
                .as_str(),
            "huashu-design"
        );
        assert_eq!(
            Slug::derive("a..b", SlugPolicy::Strict).unwrap().as_str(),
            "a-b"
        );
    }

    #[test]
    fn rejects_empty_and_reserved() {
        assert!(Slug::derive("   ", SlugPolicy::Strict).is_err());
        let err = Slug::derive("CON", SlugPolicy::Strict).unwrap_err();
        assert_eq!(err.code, "E_RESERVED_NAME");
    }

    #[test]
    fn truncates_in_lenient_mode_without_trailing_dash() {
        let long = "a".repeat(80);
        let strict = Slug::derive(&long, SlugPolicy::Strict);
        assert_eq!(strict.unwrap_err().code, "E_SLUG_INVALID");
        let lenient = Slug::derive(&long, SlugPolicy::Lenient).unwrap();
        assert_eq!(lenient.as_str().len(), MAX_SLUG_LEN);
        assert!(!lenient.as_str().ends_with('-'));
    }

    #[test]
    fn truncation_never_leaves_trailing_dash() {
        let raw = format!("{}-{}", "a".repeat(63), "b".repeat(10));
        let slug = Slug::derive(&raw, SlugPolicy::Lenient).unwrap();
        assert!(!slug.as_str().ends_with('-'));
    }

    #[test]
    fn case_insensitive_uniqueness() {
        let a = Slug::derive("PDF", SlugPolicy::Strict).unwrap();
        let b = Slug::derive("pdf", SlugPolicy::Strict).unwrap();
        assert!(a.eq_ci(&b));
        assert_eq!(a.key_ci(), b.key_ci());
    }

    #[test]
    fn non_ascii_becomes_word_boundary() {
        assert_eq!(
            Slug::derive("图表 design", SlugPolicy::Strict)
                .unwrap()
                .as_str(),
            "design"
        );
    }

    #[test]
    fn parse_exact_rejects_unnormalized_input() {
        assert!(Slug::parse_exact("PDF Toolkit").is_err());
        assert!(Slug::parse_exact("pdf-toolkit").is_ok());
    }

    #[test]
    fn serde_roundtrip_and_validation() {
        let s = Slug::derive("code-review", SlugPolicy::Strict).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"code-review\"");
        let back: Slug = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert!(serde_json::from_str::<Slug>("\"Bad Name\"").is_err());
    }

    #[test]
    fn reserved_check_is_case_insensitive() {
        assert!(is_reserved("con"));
        assert!(is_reserved("NUL"));
        assert!(!is_reserved("console"));
    }
}
