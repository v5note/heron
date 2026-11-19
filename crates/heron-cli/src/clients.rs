//! Built-in client matrix.
//!
//! In M1 these files are loaded from `resources/clients/*.toml` (implementation doc §5.2);
//! until then the same data is embedded here so the scanner has something correct to use.
//! Every entry carries its evidence level: "doc" means derived from public documentation,
//! "probe" would mean verified on a real machine.

use std::path::{Path, PathBuf};

use heron_scan::classify::ClientSpec;

struct BuiltinClient {
    id: &'static str,
    display_name: &'static str,
    priority: i64,
    /// `~/.<client>/skills`
    global_skills: &'static str,
    /// `~/.<client>/skills` for project scope, relative to the project root
    project_skills: &'static str,
    /// Legacy slash-command directory (D9)
    global_commands: Option<&'static str>,
    project_commands: Option<&'static str>,
}

const BUILTIN: &[BuiltinClient] = &[
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

/// Build the client specs, resolving `~` against `home`.
pub fn builtin_specs(home: &Path) -> Vec<ClientSpec> {
    BUILTIN
        .iter()
        .map(|c| ClientSpec {
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

/// Decision D10: Heron's own landing directory, never `~/.agents/skills`.
pub fn default_landing(home: &Path) -> PathBuf {
    home.join(heron_platform::dirs::DEFAULT_LANDING_DIRNAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_is_represented() {
        let specs = builtin_specs(Path::new("/home/u"));
        let ids: Vec<_> = specs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["agents", "claude", "codex", "cursor", "opencode"]);
        for id in &ids {
            assert!(
                heron_core::ClientId::new(*id).is_ok(),
                "{id} must be a valid client id"
            );
        }
    }

    #[test]
    fn agents_is_the_cross_client_location() {
        let specs = builtin_specs(Path::new("/home/u"));
        let agents = specs.iter().find(|s| s.id == "agents").unwrap();
        assert_eq!(
            agents.global_paths[0],
            PathBuf::from("/home/u/.agents/skills")
        );
        assert_eq!(agents.project_paths[0], PathBuf::from(".agents/skills"));
    }

    #[test]
    fn only_claude_has_legacy_commands() {
        let specs = builtin_specs(Path::new("/home/u"));
        let with_commands: Vec<_> = specs
            .iter()
            .filter(|s| !s.command_paths.is_empty())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(with_commands, vec!["claude"]);
        let claude = specs.iter().find(|s| s.id == "claude").unwrap();
        assert_eq!(
            claude.command_paths[0],
            PathBuf::from("/home/u/.claude/commands")
        );
    }

    #[test]
    fn default_landing_is_heron_owned() {
        let landing = default_landing(Path::new("/home/u"));
        assert_eq!(landing, PathBuf::from("/home/u/.heron/skills"));
        assert!(!landing.to_string_lossy().contains(".agents"));
    }
}
