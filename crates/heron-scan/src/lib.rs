//! Scanning: controlled traversal, frontmatter parsing, classification and hashing.
//!
//! The scanner never mutates the file system. Everything it finds is a `Hit`; deciding what
//! to do with it (adopt, ignore, report) belongs to the service layer.

pub mod classify;
pub mod fingerprint;
pub mod parse;
pub mod rules;
pub mod walker;

pub use classify::{Classifier, ClientSpec, HitKind, Scope};
pub use fingerprint::{fingerprint_dir, Fingerprint, FingerprintOptions};
pub use parse::{parse_skill_md, Confidence, Frontmatter, ParseIssue};
pub use rules::IgnoreRules;
pub use walker::{scan, Hit, ScanOptions, ScanResult, ScanStats};

pub use heron_core::{HeronError, Msg, Result};
