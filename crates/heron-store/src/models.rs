//! Row structs mirroring the schema. The service layer converts these into `heron-core::dto`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRow {
    pub id: String,
    pub display_name: String,
    pub global_paths: Vec<String>,
    pub project_paths: Vec<String>,
    pub link_modes: Vec<String>,
    /// Legacy slash-command directories (decision D9).
    pub global_commands: Vec<String>,
    pub project_commands: Vec<String>,
    pub priority: i64,
    pub native_scan: Option<String>,
    pub verified_at: Option<String>,
    pub verified_by: Option<String>,
    /// Tri-state presence verdict from the last scan: `None` = never detected, `Some(0)` = not on
    /// this machine, `Some(1)` = detected. Orthogonal to [`TargetRow::enabled`], which is a user
    /// switch that detection must never write.
    pub detected: Option<i64>,
    pub detected_at: Option<String>,
    /// Which signal matched, e.g. `dir:~/.claude`. Shown in the UI to make a misjudgement debuggable.
    pub detect_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillRow {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub status: String,
    pub origin: String,
    /// Authoritative content directory when it lives outside the store: a skill adopted from a link
    /// the user maintains elsewhere (adopt · 就地引用). `None` means `store/<slug>`.
    pub storage_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetRow {
    pub id: String,
    pub kind: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub path: String,
    pub enabled: bool,
    pub is_default: bool,
    pub discover: String,
}

/// Where a skill came from. Only `git` bindings can be serviced automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceBindingRow {
    pub slug: String,
    pub kind: String,
    pub url: Option<String>,
    #[serde(rename = "ref")]
    pub ref_name: Option<String>,
    pub subpath: Option<String>,
    pub pinned_commit: Option<String>,
    pub remote_version: Option<String>,
    pub content_hash: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectionRow {
    pub id: i64,
    pub slug: String,
    pub target_id: String,
    pub link_name: String,
    pub path: String,
    pub mode: String,
    pub version: Option<String>,
    pub content_hash: Option<String>,
    pub inode_kind: Option<String>,
    pub state: String,
    pub created_at: String,
}

/// One cached verify verdict per projection (the schema's `visibility` table).
#[derive(Debug, Clone)]
pub struct VisibilityRow {
    pub projection_id: i64,
    pub client_id: String,
    /// One of the five tokens `Visibility::as_str` produces; "unknown" never lands here.
    pub state: String,
    pub shadowed_by: Option<String>,
    pub checked_at: String,
}

impl Default for ProjectionRow {
    fn default() -> Self {
        Self {
            id: 0,
            slug: String::new(),
            target_id: String::new(),
            link_name: String::new(),
            path: String::new(),
            mode: String::new(),
            version: None,
            content_hash: None,
            inode_kind: None,
            // Matches the schema CHECK default: a freshly recorded projection is healthy.
            state: "ok".to_owned(),
            created_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub adapter: Option<String>,
    pub url: Option<String>,
    pub ref_name: Option<String>,
    pub subpath: Option<String>,
    pub path: Option<String>,
    pub priority: i64,
    pub trusted: bool,
    pub enabled: bool,
    pub capability: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_commit: Option<String>,
    pub last_probe_ok: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedRow {
    pub path: String,
    pub real_path: Option<String>,
    pub client_id: Option<String>,
    pub scope: Option<String>,
    pub project_path: Option<String>,
    pub observed_slug: Option<String>,
    pub content_hash: Option<String>,
    pub frontmatter_ok: Option<bool>,
    pub kind: String,
    pub last_seen_at: String,
    pub adopt_state: String,
    pub mtime_ns: Option<i64>,
    pub size_bytes: Option<i64>,
}

/// One row to upsert into `detected` during a scan.
#[derive(Debug, Clone, Serialize)]
pub struct DetectedUpsert {
    pub path: String,
    pub real_path: Option<String>,
    pub client_id: Option<String>,
    pub scope: Option<String>,
    pub project_path: Option<String>,
    pub observed_slug: Option<String>,
    pub content_hash: Option<String>,
    pub frontmatter_ok: Option<bool>,
    pub kind: String,
    pub mtime_ns: Option<i64>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub op: String,
    pub subject: Option<String>,
    pub detail: Option<String>,
    pub undoable: bool,
}

/// A user-favourited open-source skill (「来源与市场 → 我的收藏」). The `url` is what one-click
/// install feeds to the installer; `market` is an optional provenance tag for display only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FavoriteRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub description: String,
    pub market: Option<String>,
    pub created_at: String,
}
