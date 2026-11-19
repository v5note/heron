//! Remote URL normalization and ref resolution.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;

/// A normalized remote. Two spellings of the same remote must produce the same cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteUrl {
    /// URL passed to git (never contains a trailing `.git` duplicate or user credentials).
    pub url: String,
}

impl RemoteUrl {
    /// `owner/repo` -> https GitHub. Anything else is validated as a URL or SSH shorthand.
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(heron_core::ErrorCode::SourceAmbiguous
                .msg()
                .with("input", raw)
                .with("reason", "empty")
                .into());
        }

        // GitHub shorthand: exactly two segments, no scheme, no colon, no path prefix.
        if !trimmed.contains(':') && !trimmed.contains("///") {
            let parts: Vec<&str> = trimmed.trim_end_matches(".git").split('/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                return Ok(Self {
                    url: format!("https://github.com/{}/{}.git", parts[0], parts[1]),
                });
            }
        }

        if trimmed.starts_with("file://") || trimmed.starts_with("git://") {
            return Ok(Self {
                url: trimmed.trim_end_matches('/').to_owned(),
            });
        }
        if trimmed.starts_with("https://")
            || trimmed.starts_with("http://")
            || trimmed.starts_with("ssh://")
        {
            let cleaned = strip_credentials(trimmed.trim_end_matches('/'));
            // Hosting UIs omit the `.git` suffix; git accepts both, but one canonical spelling
            // keeps cache keys and stored bindings comparable.
            let canonical = if cleaned.ends_with(".git") {
                cleaned
            } else {
                format!("{cleaned}.git")
            };
            return Ok(Self { url: canonical });
        }
        // scp-like SSH: git@host:group/repo.git
        if trimmed.contains('@') && trimmed.contains(':') && !trimmed.contains("://") {
            return Ok(Self {
                url: trimmed.to_owned(),
            });
        }
        if trimmed.starts_with('/') || PathBuf::from(trimmed).is_absolute() {
            // Local path: keep as a plain path; git understands it.
            return Ok(Self {
                url: trimmed.to_owned(),
            });
        }

        Err(heron_core::ErrorCode::SourceAmbiguous
            .msg()
            .with("input", raw)
            .with("reason", "not_a_remote")
            .into())
    }

    /// Cache directory name: stable across equivalent spellings.
    pub fn cache_key(&self) -> String {
        let normalized = self
            .url
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_lowercase();
        short_hash(&normalized)
    }
}

fn strip_credentials(url: &str) -> String {
    // https://user:token@host/path -> https://host/path  (credentials come from git helpers)
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(at) = rest.find('@') {
            let host_and_path = &rest[at + 1..];
            return format!("{}{}", &url[..scheme_end + 3], host_and_path);
        }
    }
    url.to_owned()
}

/// FNV-1a based short hash; matches the platform helper so cache paths are stable.
fn short_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefKind {
    Branch,
    Tag,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRef {
    pub name: String,
    pub kind: RefKind,
    pub commit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_shorthand_is_normalized() {
        let r = RemoteUrl::parse("anthropics/skills").unwrap();
        assert_eq!(r.url, "https://github.com/anthropics/skills.git");
        let r2 = RemoteUrl::parse("anthropics/skills.git").unwrap();
        assert_eq!(r.url, r2.url);
        assert_eq!(r.cache_key(), r2.cache_key());
    }

    #[test]
    fn https_urls_keep_their_shape_but_lose_credentials() {
        let r = RemoteUrl::parse("https://user:token@gitlab.example.com/team/skills.git").unwrap();
        assert_eq!(r.url, "https://gitlab.example.com/team/skills.git");
        let no_suffix = RemoteUrl::parse("https://gitlab.example.com/team/skills").unwrap();
        assert_eq!(no_suffix.url, r.url, "one canonical spelling per remote");
    }

    #[test]
    fn file_urls_are_valid_remotes() {
        let r = RemoteUrl::parse("file:///tmp/repo.git").unwrap();
        assert_eq!(r.url, "file:///tmp/repo.git", "file URLs are used verbatim");
    }

    #[test]
    fn ssh_shorthand_is_accepted() {
        let r = RemoteUrl::parse("git@github.com:owner/repo.git").unwrap();
        assert_eq!(r.url, "git@github.com:owner/repo.git");
    }

    #[test]
    fn local_paths_are_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let r = RemoteUrl::parse(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(r.url, tmp.path().to_str().unwrap());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(RemoteUrl::parse("").is_err());
        assert!(RemoteUrl::parse("just one word").is_err());
    }

    #[test]
    fn equivalent_spellings_share_a_cache_key() {
        let a = RemoteUrl::parse("https://github.com/a/b").unwrap();
        let b = RemoteUrl::parse("https://github.com/a/b.git/").unwrap();
        assert_eq!(a.cache_key(), b.cache_key());
    }
}
