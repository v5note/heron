//! Ignore rules for the walker.
//!
//! Defaults follow the design (§5.1) and are compiled once into a `globset`.
//!
//! `**/.git/**` is what keeps the store's per-skill repositories out of the results: the store is
//! scanned as an extra skill directory, and every entry under it owns a `.git`.

use globset::{Glob, GlobSet, GlobSetBuilder};
use heron_core::Result;

/// Directories that never contain user skills.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/node_modules/**",
    "**/.git/**",
    "**/target/**",
    "**/dist/**",
    "**/.venv/**",
    "**/vendor_imports/**",
    "**/.codex/vendor_imports/**",
    "**/.vscode/extensions/**",
];

#[derive(Debug, Clone)]
pub struct IgnoreRules {
    set: GlobSet,
    patterns: Vec<String>,
}

impl IgnoreRules {
    pub fn new(extra: &[String]) -> Result<Self> {
        let mut patterns: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| (*s).to_owned()).collect();
        patterns.extend(extra.iter().cloned());
        let mut builder = GlobSetBuilder::new();
        for pattern in &patterns {
            let glob = Glob::new(pattern).map_err(|e| {
                heron_core::ErrorCode::Internal
                    .msg()
                    .with("detail", format!("bad glob {pattern}: {e}"))
            })?;
            builder.add(glob);
        }
        let set = builder.build().map_err(|e| {
            heron_core::ErrorCode::Internal
                .msg()
                .with("detail", e.to_string())
        })?;
        Ok(Self { set, patterns })
    }

    /// A path is ignored when it or any ancestor matches an exclude pattern.
    pub fn is_ignored(&self, path: &std::path::Path) -> bool {
        if self.set.is_empty() {
            return false;
        }
        // Check the path itself and each ancestor so "**/.git/**" also hides ".git" itself.
        let mut current = Some(path);
        while let Some(p) = current {
            if self.set.is_match(p) {
                return true;
            }
            current = p.parent();
        }
        false
    }

    /// Ignored when the last component is one of the well-known noise files.
    pub fn is_noise_file(name: &str) -> bool {
        matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini")
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_excludes_hide_noise_directories() {
        let rules = IgnoreRules::new(&[]).unwrap();
        assert!(rules.is_ignored(Path::new("/p/node_modules/x/SKILL.md")));
        assert!(rules.is_ignored(Path::new("/p/.git/objects")));
        assert!(rules.is_ignored(Path::new("/home/x/.vscode/extensions/foo/.claude/skills/a")));
        assert!(!rules.is_ignored(Path::new("/home/x/.claude/skills/pdf")));
    }

    #[test]
    fn user_excludes_are_added() {
        let rules = IgnoreRules::new(&["/Volumes/**".to_owned()]).unwrap();
        assert!(rules.is_ignored(Path::new("/Volumes/external/skills/pdf")));
        assert_eq!(rules.patterns().len(), DEFAULT_EXCLUDES.len() + 1);
    }

    #[test]
    fn invalid_glob_is_reported() {
        assert!(IgnoreRules::new(&["[".to_owned()]).is_err());
    }

    #[test]
    fn noise_files_by_name() {
        assert!(IgnoreRules::is_noise_file(".DS_Store"));
        assert!(!IgnoreRules::is_noise_file("SKILL.md"));
    }
}
