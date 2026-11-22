//! Strongly typed identifiers for targets, sources and markets.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, Msg};

/// `<kind>:global` or `<kind>:project:<hash>` (implementation doc §3.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct TargetId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Project,
}

impl TargetId {
    pub fn global(client: &str) -> Result<Self, Msg> {
        let client = normalize_kind(client)?;
        Ok(TargetId(format!("{client}:global")))
    }

    /// `project_key` is a stable hash of the project root (caller provides it so that
    /// `heron-core` stays free of filesystem access).
    pub fn project(client: &str, project_key: &str) -> Result<Self, Msg> {
        let client = normalize_kind(client)?;
        if project_key.is_empty() || !project_key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ErrorCode::SlugInvalid.msg().with("reason", "project_key"));
        }
        Ok(TargetId(format!("{client}:project:{project_key}")))
    }

    pub fn parse(raw: &str) -> Result<Self, Msg> {
        let mut parts = raw.split(':');
        let client = parts.next().unwrap_or_default();
        let scope = parts.next().unwrap_or_default();
        let rest: Vec<&str> = parts.collect();
        match (scope, rest.as_slice()) {
            ("global", []) => Self::global(client),
            ("project", [key]) => Self::project(client, key),
            _ => Err(ErrorCode::TargetNotFound
                .msg()
                .with("target", raw.to_owned())),
        }
    }

    pub fn kind(&self) -> &str {
        self.0.split(':').next().unwrap_or_default()
    }

    pub fn scope(&self) -> Scope {
        if self.0.split(':').nth(1) == Some("global") {
            Scope::Global
        } else {
            Scope::Project
        }
    }

    pub fn project_key(&self) -> Option<&str> {
        let mut parts = self.0.splitn(3, ':');
        parts.next();
        if parts.next()? == "project" {
            parts.next()
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TargetId {
    type Error = Msg;
    fn try_from(value: String) -> Result<Self, Msg> {
        TargetId::parse(&value)
    }
}

impl From<TargetId> for String {
    fn from(value: TargetId) -> Self {
        value.0.clone()
    }
}

impl Serialize for TargetId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

macro_rules! simple_id {
    ($name:ident, $pattern:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(raw: impl AsRef<str>) -> Result<Self, Msg> {
                let raw = raw.as_ref().trim();
                if raw.is_empty()
                    || !raw.chars().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || c == '-'
                            || c == '_'
                            || c == '.'
                    })
                {
                    return Err(ErrorCode::SlugInvalid.msg().with("raw", raw.to_owned()));
                }
                Ok($name(raw.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = Msg;
            fn try_from(value: String) -> Result<Self, Msg> {
                $name::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.clone()
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
    };
}

simple_id!(ClientId, "client");
simple_id!(SourceId, "source");
simple_id!(MarketId, "market");

/// Built-in markets (design §9.5). Kept here so core and UI agree on the wire ids.
pub const BUILTIN_MARKETS: &[(&str, &str, bool)] = &[
    // (market id, display name, enabled by default)
    ("skills_sh", "skills.sh", true),
    ("clawhub", "ClawHub", true),
    ("qoder", "Qoder", true),
    ("skillhub_cn", "SkillHub", true),
    ("modelscope", "ModelScope", true),
    ("skillsmp", "SkillsMP", false),
];

fn normalize_kind(client: &str) -> Result<String, Msg> {
    let c = client.trim().to_ascii_lowercase();
    if c.is_empty()
        || !c
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(ErrorCode::TargetNotFound
            .msg()
            .with("target", client.to_owned()));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_target_roundtrip() {
        let t = TargetId::global("claude").unwrap();
        assert_eq!(t.as_str(), "claude:global");
        assert_eq!(t.kind(), "claude");
        assert_eq!(t.scope(), Scope::Global);
        assert_eq!(TargetId::parse("claude:global").unwrap(), t);
    }

    #[test]
    fn project_target_roundtrip() {
        let t = TargetId::project("claude", "a1b2c3d4").unwrap();
        assert_eq!(t.as_str(), "claude:project:a1b2c3d4");
        assert_eq!(t.scope(), Scope::Project);
        assert_eq!(t.project_key(), Some("a1b2c3d4"));
        assert_eq!(TargetId::parse("claude:project:a1b2c3d4").unwrap(), t);
    }

    #[test]
    fn malformed_targets_are_rejected() {
        assert!(TargetId::parse("claude").is_err());
        assert!(TargetId::parse("claude:project").is_err());
        assert!(TargetId::parse("claude:project:key:extra").is_err());
        assert!(TargetId::project("claude", "not-hex!").is_err());
        assert!(TargetId::global("bad client").is_err());
    }

    #[test]
    fn client_kind_is_case_insensitive() {
        assert_eq!(
            TargetId::global("Claude").unwrap().as_str(),
            "claude:global"
        );
    }

    #[test]
    fn simple_ids_validate_charset() {
        assert!(MarketId::new("skills_sh").is_ok());
        assert!(MarketId::new("SKILLS").is_err());
        assert!(SourceId::new("builtin-skills-sh").is_ok());
        assert!(SourceId::new("bad id").is_err());
    }

    #[test]
    fn builtin_market_table_is_consistent() {
        assert_eq!(BUILTIN_MARKETS.len(), 6);
        let disabled: Vec<_> = BUILTIN_MARKETS
            .iter()
            .filter(|m| !m.2)
            .map(|m| m.0)
            .collect();
        assert_eq!(
            disabled,
            vec!["skillsmp"],
            "only skillsmp is disabled by default (decision D1)"
        );
        for (id, _, _) in BUILTIN_MARKETS {
            assert!(MarketId::new(*id).is_ok());
        }
    }
}
