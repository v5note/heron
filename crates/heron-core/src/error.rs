//! Structured errors.
//!
//! Rust never emits natural language to the user. Every failure is a `Msg` with a stable
//! code and named parameters; the Vue frontend renders `i18n.locales.<lang>.errors.<code>`.
//! Adding a code requires adding both locales (implementation doc §9.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

/// A stable, localizable message.
///
/// `code` is owned so the type can cross the IPC boundary in both directions; producers
/// pass `&'static str` literals from `ErrorCode::as_str()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Msg {
    pub code: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    pub severity: Severity,
    #[serde(default = "default_retryable")]
    pub retryable: bool,
}

fn default_retryable() -> bool {
    false
}

impl Msg {
    /// Build a message from a static code. Retryability is decided by the code table below.
    pub fn new(code: &'static str) -> Self {
        let mut params = BTreeMap::new();
        params.insert("code".to_owned(), code.to_owned());
        Self {
            code: code.to_owned(),
            params,
            severity: severity_of(code),
            retryable: is_retryable(code),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }

    pub fn info(code: &'static str) -> Self {
        let mut m = Self::new(code);
        m.severity = Severity::Info;
        m
    }

    pub fn warn(code: &'static str) -> Self {
        let mut m = Self::new(code);
        m.severity = Severity::Warn;
        m
    }

    /// Stable wire code (e.g. `E_GIT_MISSING`).
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn display_code(&self) -> &str {
        &self.code
    }
}

/// Every error code in the product, grouped by domain (implementation doc §10.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ErrorCode {
    // environment
    GitMissing,
    GitUnsupported,
    WebviewMissing,
    Permission,
    // scan
    ScanLimit,
    ScanRootMissing,
    Frontmatter,
    // slug / conflicts
    SlugConflict,
    SlugInvalid,
    ReservedName,
    // links
    LinkPrivilege,
    LinkTargetMissing,
    SafetyRefused,
    // projection
    Drifted,
    NotManaged,
    TargetNotFound,
    // git / network
    Network,
    Auth,
    RefNotFound,
    CloneTimeout,
    // sources / markets
    SourceAmbiguous,
    CatalogParse,
    MarketNoGitSource,
    MarketUnavailable,
    // upgrade
    MergeConflict,
    UpstreamGone,
    Pinned,
    DirtyWorktree,
    // native scan
    NativeScanDisabled,
    NativeScanUnsupported,
    NativeScanDrift,
    // system
    DbCorrupt,
    Cancelled,
    Internal,
}

impl ErrorCode {
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::GitMissing,
        ErrorCode::GitUnsupported,
        ErrorCode::WebviewMissing,
        ErrorCode::Permission,
        ErrorCode::ScanLimit,
        ErrorCode::ScanRootMissing,
        ErrorCode::Frontmatter,
        ErrorCode::SlugConflict,
        ErrorCode::SlugInvalid,
        ErrorCode::ReservedName,
        ErrorCode::LinkPrivilege,
        ErrorCode::LinkTargetMissing,
        ErrorCode::SafetyRefused,
        ErrorCode::Drifted,
        ErrorCode::NotManaged,
        ErrorCode::TargetNotFound,
        ErrorCode::Network,
        ErrorCode::Auth,
        ErrorCode::RefNotFound,
        ErrorCode::CloneTimeout,
        ErrorCode::SourceAmbiguous,
        ErrorCode::CatalogParse,
        ErrorCode::MarketNoGitSource,
        ErrorCode::MarketUnavailable,
        ErrorCode::MergeConflict,
        ErrorCode::UpstreamGone,
        ErrorCode::Pinned,
        ErrorCode::DirtyWorktree,
        ErrorCode::NativeScanDisabled,
        ErrorCode::NativeScanUnsupported,
        ErrorCode::NativeScanDrift,
        ErrorCode::DbCorrupt,
        ErrorCode::Cancelled,
        ErrorCode::Internal,
    ];

    /// Stable wire string, e.g. `E_GIT_MISSING`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::GitMissing => "E_GIT_MISSING",
            ErrorCode::GitUnsupported => "E_GIT_UNSUPPORTED",
            ErrorCode::WebviewMissing => "E_WEBVIEW_MISSING",
            ErrorCode::Permission => "E_PERMISSION",
            ErrorCode::ScanLimit => "E_SCAN_LIMIT",
            ErrorCode::ScanRootMissing => "E_SCAN_ROOT_MISSING",
            ErrorCode::Frontmatter => "E_FRONTMATTER",
            ErrorCode::SlugConflict => "E_SLUG_CONFLICT",
            ErrorCode::SlugInvalid => "E_SLUG_INVALID",
            ErrorCode::ReservedName => "E_RESERVED_NAME",
            ErrorCode::LinkPrivilege => "E_LINK_PRIVILEGE",
            ErrorCode::LinkTargetMissing => "E_LINK_TARGET_MISSING",
            ErrorCode::SafetyRefused => "E_SAFETY_REFUSED",
            ErrorCode::Drifted => "E_DRIFTED",
            ErrorCode::NotManaged => "E_NOT_MANAGED",
            ErrorCode::TargetNotFound => "E_TARGET_NOT_FOUND",
            ErrorCode::Network => "E_NETWORK",
            ErrorCode::Auth => "E_AUTH",
            ErrorCode::RefNotFound => "E_REF_NOT_FOUND",
            ErrorCode::CloneTimeout => "E_CLONE_TIMEOUT",
            ErrorCode::SourceAmbiguous => "E_SOURCE_AMBIGUOUS",
            ErrorCode::CatalogParse => "E_CATALOG_PARSE",
            ErrorCode::MarketNoGitSource => "E_MARKET_NO_GIT_SOURCE",
            ErrorCode::MarketUnavailable => "E_MARKET_UNAVAILABLE",
            ErrorCode::MergeConflict => "E_MERGE_CONFLICT",
            ErrorCode::UpstreamGone => "E_UPSTREAM_GONE",
            ErrorCode::Pinned => "E_PINNED",
            ErrorCode::DirtyWorktree => "E_DIRTY_WORKTREE",
            ErrorCode::NativeScanDisabled => "E_NATIVE_SCAN_DISABLED",
            ErrorCode::NativeScanUnsupported => "E_NATIVE_SCAN_UNSUPPORTED",
            ErrorCode::NativeScanDrift => "E_NATIVE_SCAN_DRIFT",
            ErrorCode::DbCorrupt => "E_DB_CORRUPT",
            ErrorCode::Cancelled => "E_CANCELLED",
            ErrorCode::Internal => "E_INTERNAL",
        }
    }

    /// Codes users can sensibly retry without changing anything.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::Network
                | ErrorCode::CloneTimeout
                | ErrorCode::MarketUnavailable
                | ErrorCode::Cancelled
                | ErrorCode::Permission
        )
    }

    pub fn msg(self) -> Msg {
        Msg::new(self.as_str())
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn severity_of(code: &str) -> Severity {
    match code {
        "W_LINK_PRIVILEGE" | "W_SCAN_LIMIT" | "W_MARKET_NO_GIT_SOURCE" => Severity::Warn,
        _ => Severity::Error,
    }
}

fn is_retryable(code: &str) -> bool {
    matches!(
        code,
        "E_NETWORK" | "E_CLONE_TIMEOUT" | "E_MARKET_UNAVAILABLE" | "E_CANCELLED" | "E_PERMISSION"
    ) || severity_of(code) != Severity::Error
}

#[derive(Debug, thiserror::Error)]
pub enum HeronError {
    #[error("{0:?}")]
    Msg(Msg),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl HeronError {
    pub fn code(&self) -> &str {
        match self {
            HeronError::Msg(m) => &m.code,
            HeronError::Io(_) => "E_INTERNAL",
        }
    }

    pub fn as_msg(&self) -> Msg {
        match self {
            HeronError::Msg(m) => m.clone(),
            HeronError::Io(e) => Msg::new("E_INTERNAL").with("detail", e.to_string()),
        }
    }
}

impl std::fmt::Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.params.is_empty() {
            write!(f, " {self:?}")?;
        }
        Ok(())
    }
}

impl From<Msg> for HeronError {
    fn from(value: Msg) -> Self {
        HeronError::Msg(value)
    }
}

impl From<ErrorCode> for Msg {
    fn from(value: ErrorCode) -> Self {
        value.msg()
    }
}

impl From<ErrorCode> for HeronError {
    fn from(value: ErrorCode) -> Self {
        HeronError::Msg(value.msg())
    }
}

pub type Result<T, E = HeronError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_unique_wire_string() {
        let mut seen = std::collections::HashSet::new();
        for code in ErrorCode::ALL {
            let s = code.as_str();
            assert!(s.starts_with("E_"), "{s} must be prefixed with E_");
            assert!(seen.insert(s), "duplicate error code {s}");
        }
    }

    #[test]
    fn msg_carries_code_as_param_for_frontend() {
        let m = ErrorCode::MergeConflict.msg().with("file", "SKILL.md");
        assert_eq!(m.code, "E_MERGE_CONFLICT");
        assert_eq!(m.params.get("file").map(String::as_str), Some("SKILL.md"));
        assert_eq!(
            m.params.get("code").map(String::as_str),
            Some("E_MERGE_CONFLICT")
        );
        assert_eq!(m.severity, Severity::Error);
        assert!(!m.retryable);
    }

    #[test]
    fn network_errors_are_retryable() {
        assert!(ErrorCode::Network.msg().retryable);
        assert!(!ErrorCode::MergeConflict.msg().retryable);
    }

    #[test]
    fn serde_roundtrip() {
        let m = ErrorCode::SlugConflict.msg().with("slug", "pdf");
        let json = serde_json::to_string(&m).unwrap();
        let back: Msg = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }
}
