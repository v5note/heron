//! The built-in client matrix, materialized into the index on first run.
//!
//! Until `resources/clients/*.toml` ships (M2), the same data lives here with an explicit
//! evidence level. Every path carries a `verified_by` value so the UI can be honest about it.

use std::path::{Path, PathBuf};

use heron_store::db::Db;
use heron_store::models::{ClientRow, TargetRow};
use heron_store::repos::{client, target};

use crate::Result;

pub struct BuiltinClient {
    pub id: &'static str,
    pub display_name: &'static str,
    pub priority: i64,
    pub global_skills: &'static str,
    pub project_skills: &'static str,
    pub global_commands: Option<&'static str>,
    pub project_commands: Option<&'static str>,
}

pub const BUILTIN: &[BuiltinClient] = &[
    BuiltinClient {
        id: "agents",
        display_name: "Agents (.agents)",
        priority: 10,
        global_skills: ".agents/skills",
        project_skills: ".agents/skills",
        global_commands: None,
        project_commands: None,
    },
    BuiltinClient {
        id: "claude",
        display_name: "Claude Code",
        priority: 20,
        global_skills: ".claude/skills",
        project_skills: ".claude/skills",
        global_commands: Some(".claude/commands"),
        project_commands: Some(".claude/commands"),
    },
    BuiltinClient {
        id: "codex",
        display_name: "Codex",
        priority: 30,
        global_skills: ".codex/skills",
        project_skills: ".codex/skills",
        global_commands: None,
        project_commands: None,
    },
    BuiltinClient {
        id: "cursor",
        display_name: "Cursor",
        priority: 40,
        global_skills: ".cursor/skills",
        project_skills: ".cursor/skills",
        global_commands: None,
        project_commands: None,
    },
    BuiltinClient {
        id: "opencode",
        display_name: "opencode",
        priority: 50,
        global_skills: ".config/opencode/skills",
        project_skills: ".opencode/skills",
        global_commands: None,
        project_commands: None,
    },
];

/// Heron's own landing directory (decision D10).
pub fn default_landing(home: &Path) -> PathBuf {
    home.join(heron_platform::dirs::DEFAULT_LANDING_DIRNAME)
}

pub fn specs(home: &Path) -> Vec<heron_scan::classify::ClientSpec> {
    BUILTIN
        .iter()
        .map(|c| heron_scan::classify::ClientSpec {
            id: c.id.to_owned(),
            display_name: c.display_name.to_owned(),
            global_paths: vec![home.join(c.global_skills)],
            project_paths: vec![PathBuf::from(c.project_skills)],
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
        .collect()
}

/// Seed the client matrix and the default landing target. Idempotent.
pub fn bootstrap(db: &mut Db, home: &Path) -> Result<()> {
    if db.kv_get("bootstrap_done")?.as_deref() == Some("1") {
        return Ok(());
    }
    let all = specs(home);
    db.write(|tx| {
        for spec in &all {
            client::upsert(
                tx,
                &ClientRow {
                    id: spec.id.clone(),
                    display_name: spec.display_name.clone(),
                    global_paths: plain(spec.global_paths.iter()),
                    project_paths: plain(spec.project_paths.iter()),
                    link_modes: vec!["symlink".into(), "junction".into(), "copy".into()],
                    global_commands: plain(spec.command_paths.iter()),
                    project_commands: plain(spec.project_command_paths.iter()),
                    priority: spec.priority,
                    native_scan: None,
                    verified_at: Some("2026-11-19".into()),
                    verified_by: Some("doc".into()),
                },
            )?;
        }
        client::upsert(
            tx,
            &ClientRow {
                id: "heron".into(),
                display_name: "Heron landing".into(),
                global_paths: vec![default_landing(home).to_string_lossy().to_string()],
                project_paths: vec![".heron/skills".into()],
                link_modes: vec!["symlink".into(), "junction".into(), "copy".into()],
                global_commands: vec![],
                project_commands: vec![],
                priority: 0,
                native_scan: None,
                verified_at: Some("2026-11-19".into()),
                verified_by: Some("design".into()),
            },
        )?;
        // Client-native directories are targets too, so users can ask for them explicitly.
        for spec in &all {
            if let Some(first) = spec.global_paths.first() {
                target::upsert(
                    tx,
                    &TargetRow {
                        id: format!("{}:global", spec.id),
                        kind: spec.id.clone(),
                        scope: "global".into(),
                        project_path: None,
                        path: first.to_string_lossy().to_string(),
                        enabled: true,
                        is_default: false,
                        discover: "auto".into(),
                    },
                )?;
            }
        }
        target::upsert(
            tx,
            &TargetRow {
                id: "heron:global".into(),
                kind: "heron".into(),
                scope: "global".into(),
                project_path: None,
                path: default_landing(home).to_string_lossy().to_string(),
                enabled: true,
                is_default: true,
                discover: "manual".into(),
            },
        )?;
        Ok(())
    })?;
    db.kv_set("bootstrap_done", "1")?;
    Ok(())
}

fn plain<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> Vec<String> {
    paths.map(|p| p.to_string_lossy().to_string()).collect()
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
    fn bootstrap_is_idempotent_and_seeds_heron_landing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
        let home = Path::new("/home/u");
        bootstrap(&mut db, home).unwrap();
        bootstrap(&mut db, home).unwrap();

        let clients = client::list(db.conn()).unwrap();
        assert_eq!(
            clients.len(),
            BUILTIN.len() + 1,
            "five clients plus the heron landing"
        );
        let landing = target::get_default(db.conn()).unwrap().unwrap();
        assert_eq!(landing.path, "/home/u/.heron/skills");
        assert_eq!(landing.kind, "heron");
    }

    #[test]
    fn stored_specs_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
        bootstrap(&mut db, Path::new("/home/u")).unwrap();
        let specs = loaded_specs(&db).unwrap();
        let claude = specs.iter().find(|s| s.id == "claude").unwrap();
        assert_eq!(
            claude.global_paths[0],
            PathBuf::from("/home/u/.claude/skills")
        );
        assert_eq!(
            claude.command_paths[0],
            PathBuf::from("/home/u/.claude/commands")
        );
    }
}
