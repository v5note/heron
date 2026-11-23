//! The client matrix, loaded from `resources/clients/*.toml` (design §7.2).
//!
//! The files ship inside the binary and are the only place client paths are written down. Each
//! entry states how it was verified, so the UI can be honest about unverified assumptions.

use std::path::{Path, PathBuf};

use heron_store::db::Db;
use heron_store::models::{ClientRow, TargetRow};
use heron_store::repos::{client, target};
use serde::Deserialize;

use crate::Result;

const BUILTIN_FILES: &[(&str, &str)] = &[
    (
        "agents",
        include_str!("../../../resources/clients/agents.toml"),
    ),
    (
        "claude",
        include_str!("../../../resources/clients/claude.toml"),
    ),
    (
        "codex",
        include_str!("../../../resources/clients/codex.toml"),
    ),
    (
        "cursor",
        include_str!("../../../resources/clients/cursor.toml"),
    ),
    (
        "opencode",
        include_str!("../../../resources/clients/opencode.toml"),
    ),
];

#[derive(Debug, Clone, Deserialize)]
pub struct ClientToml {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub priority: i64,
    pub global_skills: String,
    pub project_skills: String,
    #[serde(default)]
    pub global_commands: Option<String>,
    #[serde(default)]
    pub project_commands: Option<String>,
    #[serde(default)]
    pub link_modes: Vec<String>,
    #[serde(default)]
    pub verified_at: Option<String>,
    #[serde(default)]
    pub verified_by: Option<String>,
    #[serde(default)]
    pub native_scan: Option<NativeScanToml>,
    /// When the client notices new files in its skill dirs: `"session"` = it only rescans when a
    /// new session opens (so a fresh projection needs a restart); unset = treated as hot-reloading
    /// and never produces `restart_required`. Marked per client only with real evidence.
    #[serde(default)]
    pub rescan: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeScanToml {
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub evidence: String,
}

/// The built-in matrix, parsed once.
pub fn builtin() -> Result<Vec<ClientToml>> {
    BUILTIN_FILES
        .iter()
        .map(|(name, text)| {
            toml::from_str::<ClientToml>(text).map_err(|e| {
                heron_core::HeronError::from(
                    heron_core::ErrorCode::Internal
                        .msg()
                        .with("detail", format!("clients/{name}.toml: {e}")),
                )
            })
        })
        .collect()
}

pub fn specs(home: &Path) -> Result<Vec<heron_scan::classify::ClientSpec>> {
    Ok(builtin()?
        .into_iter()
        .map(|c| heron_scan::classify::ClientSpec {
            id: c.id,
            display_name: c.display_name,
            global_paths: vec![home.join(&c.global_skills)],
            project_paths: vec![PathBuf::from(&c.project_skills)],
            command_paths: c
                .global_commands
                .map(|p| vec![home.join(p)])
                .unwrap_or_default(),
            project_command_paths: c
                .project_commands
                .map(|p| vec![PathBuf::from(p)])
                .unwrap_or_default(),
            priority: c.priority,
        })
        .collect())
}

/// Seed the client matrix. Idempotent.
///
/// No target is marked default: projecting into a client directory is always an explicit choice.
/// Heron's own `~/.heron/skills` is the store, not a projection target, so it never appears here.
pub fn bootstrap(db: &mut Db, home: &Path) -> Result<()> {
    if db.kv_get("bootstrap_done")?.as_deref() == Some("1") {
        return Ok(());
    }
    let matrix = builtin()?;
    db.write(|tx| {
        for c in &matrix {
            client::upsert(
                tx,
                &ClientRow {
                    id: c.id.clone(),
                    display_name: c.display_name.clone(),
                    global_paths: vec![home.join(&c.global_skills).to_string_lossy().to_string()],
                    project_paths: vec![c.project_skills.clone()],
                    link_modes: c.link_modes.clone(),
                    global_commands: c
                        .global_commands
                        .as_ref()
                        .map(|p| vec![home.join(p).to_string_lossy().to_string()])
                        .unwrap_or_default(),
                    project_commands: c
                        .project_commands
                        .as_ref()
                        .map(|p| vec![p.clone()])
                        .unwrap_or_default(),
                    priority: c.priority,
                    native_scan: c.native_scan.as_ref().map(|n| {
                        serde_json::json!({ "supported": n.supported, "evidence": n.evidence })
                            .to_string()
                    }),
                    verified_at: c.verified_at.clone(),
                    verified_by: c.verified_by.clone(),
                },
            )?;
            target::upsert(
                tx,
                &TargetRow {
                    id: format!("{}:global", c.id),
                    kind: c.id.clone(),
                    scope: "global".into(),
                    project_path: None,
                    path: home.join(&c.global_skills).to_string_lossy().to_string(),
                    enabled: true,
                    is_default: false,
                    discover: "auto".into(),
                },
            )?;
        }
        Ok(())
    })?;
    db.kv_set("bootstrap_done", "1")?;
    Ok(())
}

/// Client specs as stored in the index (so user edits survive).
pub fn loaded_specs(db: &Db) -> Result<Vec<heron_scan::classify::ClientSpec>> {
    Ok(client::list(db.conn())?
        .into_iter()
        .map(|row| heron_scan::classify::ClientSpec {
            id: row.id,
            display_name: row.display_name,
            global_paths: row.global_paths.iter().map(PathBuf::from).collect(),
            project_paths: row.project_paths.iter().map(PathBuf::from).collect(),
            command_paths: row.global_commands.iter().map(PathBuf::from).collect(),
            project_command_paths: row.project_commands.iter().map(PathBuf::from).collect(),
            priority: row.priority,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_matrix_parses_from_toml() {
        let matrix = builtin().unwrap();
        assert_eq!(matrix.len(), 5);
        let claude = matrix.iter().find(|c| c.id == "claude").unwrap();
        assert_eq!(claude.global_skills, ".claude/skills"); // config values always use '/'
        assert_eq!(claude.global_commands.as_deref(), Some(".claude/commands"));
        assert_eq!(claude.verified_by.as_deref(), Some("doc"));
        assert_eq!(claude.rescan.as_deref(), Some("session"));
        // Only claude is evidence-backed as session-rescan; everyone else stays watch.
        for c in matrix.iter().filter(|c| c.id != "claude") {
            assert!(c.rescan.is_none(), "{} must opt in explicitly", c.id);
        }
        assert!(
            !claude.native_scan.as_ref().unwrap().supported,
            "D6: no injection until verified"
        );
        for c in &matrix {
            assert!(c.link_modes.iter().any(|m| m == "symlink"));
            assert!(!c.priority.eq(&-1));
        }
    }

    #[test]
    fn bootstrap_is_idempotent_and_leaves_no_default_target() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
        let home = Path::new("/home/u");
        bootstrap(&mut db, home).unwrap();
        bootstrap(&mut db, home).unwrap();

        let clients = client::list(db.conn()).unwrap();
        assert_eq!(clients.len(), 5, "only the built-in client matrix");
        assert!(!clients.iter().any(|c| c.id == "heron"));
        let targets = target::list(db.conn()).unwrap();
        assert_eq!(targets.len(), 5, "one global target per built-in client");
        assert!(targets.iter().any(|t| t.id == "claude:global"));
        assert!(
            !targets.iter().any(|t| t.id == "heron:global"),
            "the store is not a projection target"
        );
        assert!(
            target::get_default(db.conn()).unwrap().is_none(),
            "nothing is projected without an explicit --on"
        );
        assert_eq!(targets.iter().filter(|t| t.is_default).count(), 0);
    }
}
