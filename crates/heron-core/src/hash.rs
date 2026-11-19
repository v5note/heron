//! Content fingerprints.
//!
//! The algorithm is defined here (pure) so that every consumer — store, projection,
//! lockfile — hashes byte-identically (implementation doc §5.4).

use serde::Deserialize;

/// A normalized content fingerprint, rendered as `blake3:<hex>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Hash(String);

impl Hash {
    pub fn from_hex(hex: impl Into<String>) -> Self {
        let h = hex.into();
        if h.starts_with("blake3:") {
            Hash(h)
        } else {
            Hash(format!("blake3:{h}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn hex(&self) -> &str {
        self.0.strip_prefix("blake3:").unwrap_or(&self.0)
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Hash {
    type Error = crate::error::Msg;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.starts_with("blake3:") && value.len() > "blake3:".len() {
            Ok(Hash(value))
        } else {
            Err(crate::error::ErrorCode::Internal
                .msg()
                .with("detail", "invalid hash"))
        }
    }
}

impl From<Hash> for String {
    fn from(value: Hash) -> Self {
        value.0.clone()
    }
}

impl serde::Serialize for Hash {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

/// Files and directories that never contribute to a fingerprint.
pub const HASH_IGNORED_NAMES: &[&str] = &[
    ".git",
    ".DS_Store",
    "Thumbs.db",
    ".heron",
    "heron.skill.json",
];

/// Normalize a relative path for hashing: forward slashes, no leading `./`,
/// no trailing slash. Callers feed paths already relative to the skill root.
pub fn normalize_rel_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches("./").trim_start_matches('/');
    p.trim_end_matches('/').to_owned()
}

/// Normalize text content before hashing: strip a UTF-8 BOM and convert CRLF/CR to LF
/// so that a Windows checkout of the same commit hashes like a macOS checkout.
pub fn normalize_text_bytes(bytes: &[u8]) -> Vec<u8> {
    let body = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'\r' {
            out.push(b'\n');
            if i + 1 < body.len() && body[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        out.push(body[i]);
        i += 1;
    }
    out
}

/// Whether a relative path should be skipped by the fingerprint (and by the scanner).
pub fn is_ignored(rel_path: &str) -> bool {
    rel_path.split('/').any(|seg| {
        HASH_IGNORED_NAMES
            .iter()
            .any(|ignored| seg.eq_ignore_ascii_case(ignored))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_hex_prefixing() {
        let h = Hash::from_hex("abc123");
        assert_eq!(h.to_string(), "blake3:abc123");
        assert_eq!(h.hex(), "abc123");
        assert_eq!(Hash::from_hex("blake3:abc").to_string(), "blake3:abc");
    }

    #[test]
    fn hash_serde_roundtrip() {
        let h = Hash::from_hex("deadbeef");
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, "\"blake3:deadbeef\"");
        let back: Hash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
        assert!(serde_json::from_str::<Hash>("\"deadbeef\"").is_err());
    }

    #[test]
    fn path_normalization() {
        assert_eq!(normalize_rel_path("./a/b/"), "a/b");
        assert_eq!(normalize_rel_path("a\\b\\c"), "a/b/c");
        assert_eq!(normalize_rel_path("/abs"), "abs");
    }

    #[test]
    fn crlf_and_bom_are_normalized() {
        let a = normalize_text_bytes(b"line1\r\nline2\r\n");
        let b = normalize_text_bytes(b"line1\nline2\n");
        assert_eq!(a, b);
        let with_bom = normalize_text_bytes(b"\xEF\xBB\xBFhello");
        assert_eq!(with_bom, b"hello");
        // Lone CR also collapses to LF.
        assert_eq!(normalize_text_bytes(b"a\rb"), b"a\nb");
    }

    #[test]
    fn ignored_paths() {
        assert!(is_ignored(".git/config"));
        assert!(is_ignored("sub/.DS_Store"));
        assert!(is_ignored("heron.skill.json"));
        assert!(!is_ignored("SKILL.md"));
        assert!(!is_ignored("references/guide.md"));
    }
}
