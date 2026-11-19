//! Data transfer objects shared by the CLI, the Tauri commands and the Vue frontend.
//!
//! Rules (implementation doc §3.3):
//! - paths are strings with forward slashes, already display-ready;
//! - no database rows leak out of the service layer;
//! - every field name is stable wire format — the Vue `api/types.ts` mirrors this file.

use serde::{Deserialize, Serialize};

use crate::hash::Hash;
use crate::ids::{MarketId, Scope};
use crate::policy::ProjectionMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillStatus {
    Active,
    Trashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Store,
    Adopted,
    LegacyCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionState {
    Ok,
    Broken,
    Drifted,
    Modified,
    Shadowed,
}

/// Whether a client would actually load the projection (design §10.11.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    NotScanned,
    Shadowed { by: String },
    RestartRequired,
    Broken,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub host: String,
    pub path: String,
    pub ref_name: Option<String>,
    pub subpath: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetRef {
    pub id: String,
    pub kind: String,
    pub scope: Scope,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionView {
    pub target: TargetRef,
    pub mode: ProjectionMode,
    pub state: ProjectionState,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillFlags {
    pub has_local_changes: bool,
    pub upgradable: bool,
    pub legacy: bool,
    pub unmanaged: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub status: SkillStatus,
    pub origin: Origin,
    pub source: Option<SourceRef>,
    pub projections: Vec<ProjectionView>,
    pub flags: SkillFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub summary: String,
    pub author: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetEstimate {
    /// Sum of name/description tokens across skills, per target client.
    pub by_target: std::collections::BTreeMap<String, u32>,
    pub total: u32,
    /// True when the total crosses the advisory threshold (design risk R6).
    pub warns: bool,
}

impl BudgetEstimate {
    pub const WARN_THRESHOLD: u32 = 5_000;

    pub fn new(by_target: std::collections::BTreeMap<String, u32>) -> Self {
        let total = by_target.values().copied().sum();
        Self {
            by_target,
            total,
            warns: total > Self::WARN_THRESHOLD,
        }
    }
}

/// Rough token cost of a skill's catalog entry. Deliberately pessimistic (~3.5 chars/token).
pub fn estimate_tokens(name: &str, description: &str) -> u32 {
    let chars = (name.chars().count() + description.chars().count()) as f64;
    (chars / 3.5).ceil() as u32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReportDto {
    pub hits: usize,
    pub skills: usize,
    pub legacy_commands: usize,
    pub nested_suspects: usize,
    pub dirs_visited: usize,
    pub duration_ms: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFingerprint {
    pub hash: Hash,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketItem {
    pub market: MarketId,
    pub key: String,
    pub name: String,
    pub owner: Option<String>,
    pub summary: String,
    pub installable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimate_rounds_up() {
        // "pdf" (3) + "Extract text" (12) = 15 chars / 3.5 = 4.28 -> 5
        assert_eq!(estimate_tokens("pdf", "Extract text"), 5);
        assert_eq!(estimate_tokens("a", ""), 1);
    }

    #[test]
    fn budget_warns_over_threshold() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("claude".to_owned(), 4_000);
        m.insert("agents".to_owned(), 2_000);
        let b = BudgetEstimate::new(m);
        assert_eq!(b.total, 6_000);
        assert!(b.warns);

        let mut small = std::collections::BTreeMap::new();
        small.insert("claude".to_owned(), 100);
        assert!(!BudgetEstimate::new(small).warns);
    }

    #[test]
    fn visibility_serializes_as_tagged_object() {
        let v = Visibility::Shadowed {
            by: "code-review@plugin".into(),
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["state"], "shadowed");
        assert_eq!(json["by"], "code-review@plugin");
        assert_eq!(
            serde_json::to_value(Visibility::Visible).unwrap()["state"],
            "visible"
        );
    }

    #[test]
    fn skill_summary_is_serializable() {
        let s = SkillSummary {
            slug: "pdf".into(),
            name: "pdf".into(),
            description: "Extract text".into(),
            version: Some("1.4.0".into()),
            status: SkillStatus::Active,
            origin: Origin::Adopted,
            source: None,
            projections: vec![],
            flags: SkillFlags {
                upgradable: true,
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("\"legacy-command\""));
        assert!(json.contains("\"adopted\""));
        assert!(json.contains("\"has_local_changes\":false"));
    }
}
