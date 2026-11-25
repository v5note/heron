//! Deciding *what* a hit is: which client location it lives in, which project, and whether
//! it is a skill, a legacy command file or a nested-suspect (design §5.1, D8, D9).

use std::path::{Path, PathBuf};

use heron_core::{Slug, SlugPolicy};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Project,
}

impl Scope {
    /// The `detected.scope` spelling: the database and the GUI events share it.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HitKind {
    Skill,
    /// `.claude/commands/*.md` — taken over as a skill (decision D9).
    LegacyCommand,
    /// A skills directory nested inside another skills directory (decision D8: report only).
    NestedSuspect,
}

impl HitKind {
    /// The `detected.kind` spelling: the database and the GUI events share it.
    pub fn as_str(self) -> &'static str {
        match self {
            HitKind::Skill => "skill",
            HitKind::LegacyCommand => "legacy-command",
            HitKind::NestedSuspect => "nested-suspect",
        }
    }
}

/// One client's scan locations, loaded from `resources/clients/*.toml`.
#[derive(Debug, Clone)]
pub struct ClientSpec {
    pub id: String,
    pub display_name: String,
    /// Absolute paths for `~/.<client>/skills`.
    pub global_paths: Vec<PathBuf>,
    /// Relative paths, e.g. `.claude/skills`.
    pub project_paths: Vec<PathBuf>,
    /// `~/.<client>/commands` style legacy command directories.
    pub command_paths: Vec<PathBuf>,
    pub project_command_paths: Vec<PathBuf>,
    pub priority: i64,
}

/// Project roots the scanner is allowed to descend into (auto-discovered + registered).
#[derive(Debug, Clone, Default)]
pub struct Classifier {
    clients: Vec<ClientSpec>,
    projects: Vec<PathBuf>,
    /// Skill directories this app projected into a client's skills folder. They live in a scanned
    /// location, but they are content the library already took over — not discoveries.
    projections: Vec<PathBuf>,
    /// The directory names a client owns, e.g. `.claude`. Derived from the declared locations
    /// instead of being listed here, so a client added to the matrix is covered by nested-suspect
    /// detection on the same day it is added to the scan.
    client_dirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: HitKind,
    pub client: Option<String>,
    pub scope: Scope,
    pub project: Option<PathBuf>,
    pub slug: Option<String>,
}

impl Classifier {
    pub fn new(clients: Vec<ClientSpec>, projects: Vec<PathBuf>) -> Self {
        let projects = projects
            .into_iter()
            .map(|p| heron_platform::paths::normalize(&p))
            .collect();
        // The owner directory of a client is the one that holds `skills`/`commands`: in a
        // repository the first segment of a declared location (`.claude/skills` -> `.claude`), on
        // a machine the segment before the final one (`/home/u/.claude/skills` -> `.claude`).
        // Derived rather than listed, so a client joins nested-suspect detection the day it joins
        // the matrix.
        let mut client_dirs: Vec<String> = clients
            .iter()
            .flat_map(|c| {
                c.project_paths
                    .iter()
                    .chain(c.project_command_paths.iter())
                    .filter_map(|rel| rel.components().next())
                    .map(|first| first.as_os_str().to_string_lossy().into_owned())
                    .chain(
                        c.global_paths
                            .iter()
                            .chain(c.command_paths.iter())
                            // Only the documented last segment names an owner: this must not turn
                            // a bare `/opt` into a "client directory".
                            .filter(|abs| {
                                abs.file_name()
                                    .is_some_and(|leaf| leaf == "skills" || leaf == "commands")
                            })
                            .filter_map(|abs| abs.parent().map(|owner| owner.to_path_buf()))
                            .filter_map(|owner| {
                                owner.file_name().map(|n| n.to_string_lossy().into_owned())
                            }),
                    )
            })
            // An absolute or `..`-prefixed declaration names no directory inside a repo.
            .filter(|name| {
                !name.is_empty()
                    && name != "."
                    && name != ".."
                    && !name.contains('/')
                    && !name.contains('\\')
                    && !Path::new(&name).has_root()
            })
            .collect();
        client_dirs.sort();
        client_dirs.dedup();
        Self {
            clients,
            projects,
            projections: Vec::new(),
            client_dirs,
        }
    }

    /// Tell the classifier which skill directories Heron wrote itself, so a scan of `.claude/skills`
    /// or `.agents/skills` does not present a projection as a skill waiting to be taken over.
    pub fn with_projections(mut self, projections: Vec<PathBuf>) -> Self {
        self.projections = projections
            .into_iter()
            .map(|p| heron_platform::paths::normalize(&p))
            .collect();
        self
    }

    pub fn clients(&self) -> &[ClientSpec] {
        &self.clients
    }

    pub fn projects(&self) -> &[PathBuf] {
        &self.projects
    }

    /// Classify a `SKILL.md` path or a legacy command file path.
    pub fn classify(&self, path: &Path, is_command_file: bool) -> Option<Classification> {
        let norm = heron_platform::paths::normalize(path);
        // Both a `SKILL.md` and a legacy `commands/*.md` live directly inside their directory.
        let dir = norm.parent()?.to_path_buf();

        // A directory the library projected is content it owns already. Reporting it again would
        // put the user's own copy back in front of them as something to take over.
        if self
            .projections
            .iter()
            .any(|p| heron_platform::paths::same_path_ci(p, &dir))
        {
            return None;
        }

        // The path we compare against is the *skills directory*, not the item inside it.
        let skills_dir = self.skills_dir_of(&dir, is_command_file)?;

        for client in &self.clients {
            // 1) project scope first: a project path is more specific than a global one.
            for project in &self.projects {
                let project_targets = if is_command_file {
                    &client.project_command_paths
                } else {
                    &client.project_paths
                };
                for rel in project_targets {
                    let expected = heron_platform::paths::normalize(&project.join(rel));
                    if heron_platform::paths::same_path_ci(&expected, &skills_dir) {
                        return Some(Classification {
                            kind: if is_command_file {
                                HitKind::LegacyCommand
                            } else {
                                HitKind::Skill
                            },
                            client: Some(client.id.clone()),
                            scope: Scope::Project,
                            project: Some(project.clone()),
                            slug: None,
                        });
                    }
                }
            }
            // 2) global scope.
            let globals = if is_command_file {
                &client.command_paths
            } else {
                &client.global_paths
            };
            for expected in globals {
                if heron_platform::paths::same_path_ci(expected, &skills_dir) {
                    return Some(Classification {
                        kind: if is_command_file {
                            HitKind::LegacyCommand
                        } else {
                            HitKind::Skill
                        },
                        client: Some(client.id.clone()),
                        scope: Scope::Global,
                        project: None,
                        slug: None,
                    });
                }
            }
        }
        None
    }

    /// Walk up from an item path until we find a directory that equals one of the
    /// client's declared locations (global or project). Returns that location.
    fn skills_dir_of(&self, item_dir: &Path, is_command_file: bool) -> Option<PathBuf> {
        let mut probe = Some(item_dir.to_path_buf());
        while let Some(dir) = probe {
            let candidates: Vec<PathBuf> = self
                .clients
                .iter()
                .flat_map(|c| {
                    let globals = if is_command_file {
                        &c.command_paths
                    } else {
                        &c.global_paths
                    };
                    let rel = if is_command_file {
                        &c.project_command_paths
                    } else {
                        &c.project_paths
                    };
                    let mut out: Vec<PathBuf> = globals.clone();
                    for r in rel {
                        for project in &self.projects {
                            out.push(project.join(r));
                        }
                    }
                    out
                })
                .collect();
            if candidates
                .iter()
                .any(|c| heron_platform::paths::same_path_ci(c, &dir))
            {
                return Some(dir);
            }
            if dir.parent().is_none() {
                break;
            }
            probe = dir.parent().map(|p| p.to_path_buf());
        }
        None
    }

    /// Nested-suspect detection (D8).
    ///
    /// A skill is "nested" when, looking at its ancestors, we find a *client* directory
    /// (`.claude`, `.agents`, …) whose `skills` subdirectory is not itself a declared scan
    /// root. That is exactly the shape produced when a whole project tree was copied into a
    /// skill: `~/.agents/skills/<x>/.agents/skills/<y>/SKILL.md`.
    ///
    /// Nested skills are reported only; the user decides (decision D8).
    pub fn is_nested_suspect(&self, skill_dir: &Path) -> bool {
        let mut depth = 0;
        let mut current = skill_dir.parent();
        while let Some(dir) = current {
            depth += 1;
            if depth > 8 {
                return false;
            }
            let is_client_dir = dir.file_name().is_some_and(|n| {
                let name = n.to_string_lossy();
                self.client_dirs.iter().any(|c| c == name.as_ref())
            });
            if is_client_dir {
                let skills_dir = dir.join("skills");
                if skills_dir.is_dir() && !self.is_declared_root(&skills_dir) {
                    return true;
                }
            }
            current = dir.parent();
        }
        false
    }

    /// Is this directory one of the scan roots configured for a client?
    ///
    /// A skills dir is a legitimate root when it equals a client's global path, or when it is
    /// `<project>/<relative project path>` for one of the known projects.
    fn is_declared_root(&self, dir: &Path) -> bool {
        if self.clients.iter().any(|c| {
            c.global_paths
                .iter()
                .any(|p| heron_platform::paths::same_path_ci(p, dir))
        }) {
            return true;
        }
        self.clients.iter().any(|c| {
            c.project_paths.iter().any(|rel| {
                self.projects
                    .iter()
                    .any(|project| heron_platform::paths::same_path_ci(&project.join(rel), dir))
            })
        })
    }

    /// Slug for a hit: legacy commands use the file stem (subdirectories become `dir:name`).
    pub fn slug_for(&self, path: &Path, classification: &Classification) -> Option<String> {
        let dir = path.parent()?;
        match classification.kind {
            HitKind::LegacyCommand => {
                let stem = path.file_stem()?.to_string_lossy().to_string();
                let base = self.base_dir_for(dir, classification)?;
                let rel = dir.strip_prefix(&base).ok()?;
                let prefix: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let raw = if prefix.is_empty() {
                    stem
                } else {
                    prefix.join("-") + "-" + &stem
                };
                Slug::derive(&raw, SlugPolicy::Lenient)
                    .ok()
                    .map(|s| s.as_str().to_owned())
            }
            _ => {
                let name = dir.file_name()?.to_string_lossy().to_string();
                Slug::derive(&name, SlugPolicy::Lenient)
                    .ok()
                    .map(|s| s.as_str().to_owned())
            }
        }
    }

    fn base_dir_for(&self, dir: &Path, classification: &Classification) -> Option<PathBuf> {
        let client = self
            .clients
            .iter()
            .find(|c| Some(&c.id) == classification.client.as_ref())?;
        let candidates = match classification.scope {
            Scope::Global => client.command_paths.clone(),
            Scope::Project => {
                let project = classification.project.as_ref()?;
                client
                    .project_command_paths
                    .iter()
                    .map(|rel| project.join(rel))
                    .collect()
            }
        };
        candidates
            .into_iter()
            .find(|c| heron_platform::paths::same_path_ci(c, dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ClientSpec {
        ClientSpec {
            id: "claude".into(),
            display_name: "Claude Code".into(),
            global_paths: vec![PathBuf::from("/home/u/.claude/skills")],
            project_paths: vec![PathBuf::from(".claude/skills")],
            command_paths: vec![PathBuf::from("/home/u/.claude/commands")],
            project_command_paths: vec![PathBuf::from(".claude/commands")],
            priority: 20,
        }
    }

    fn classifier() -> Classifier {
        Classifier::new(vec![spec()], vec![PathBuf::from("/work/proj")])
    }

    #[test]
    fn classifies_global_project_and_commands() {
        let c = classifier();

        let global = c
            .classify(
                Path::new("/home/u/.claude/skills/wechat-mp/SKILL.md"),
                false,
            )
            .unwrap();
        assert_eq!(global.kind, HitKind::Skill);
        assert_eq!(global.scope, Scope::Global);
        assert_eq!(global.client.as_deref(), Some("claude"));

        let project = c
            .classify(Path::new("/work/proj/.claude/skills/tdd/SKILL.md"), false)
            .unwrap();
        assert_eq!(project.scope, Scope::Project);
        assert_eq!(project.project.as_deref(), Some(Path::new("/work/proj")));

        let cmd = c
            .classify(Path::new("/home/u/.claude/commands/review.md"), true)
            .unwrap();
        assert_eq!(cmd.kind, HitKind::LegacyCommand);
        assert_eq!(cmd.scope, Scope::Global);

        let pcmd = c
            .classify(Path::new("/work/proj/.claude/commands/git/commit.md"), true)
            .unwrap();
        assert_eq!(pcmd.kind, HitKind::LegacyCommand);
        assert_eq!(pcmd.scope, Scope::Project);
    }

    #[test]
    fn a_projected_directory_is_not_a_discovery() {
        let c =
            classifier().with_projections(vec![PathBuf::from("/home/u/.claude/skills/wechat-mp")]);

        // The library wrote this one, so it is already taken over.
        assert!(c
            .classify(
                Path::new("/home/u/.claude/skills/wechat-mp/SKILL.md"),
                false
            )
            .is_none());
        // Everything else in the same skills directory is still a discovery.
        assert!(c
            .classify(Path::new("/home/u/.claude/skills/novel/SKILL.md"), false)
            .is_some());
    }

    #[test]
    fn unrelated_paths_are_not_classified() {
        let c = classifier();
        assert!(c
            .classify(Path::new("/tmp/random/skills/x/SKILL.md"), false)
            .is_none());
        assert!(c
            .classify(Path::new("/home/u/.agents/skills/x/SKILL.md"), false)
            .is_none());
    }

    #[test]
    fn slugs_have_no_trailing_or_leading_dashes() {
        let c = classifier();
        let cls = c
            .classify(Path::new("/home/u/.claude/skills/My_Skill/SKILL.md"), false)
            .unwrap();
        assert_eq!(
            c.slug_for(Path::new("/home/u/.claude/skills/My_Skill/SKILL.md"), &cls)
                .as_deref(),
            Some("my-skill")
        );
    }

    #[test]
    fn legacy_command_slug_uses_relative_path() {
        let c = classifier();
        let cls = c
            .classify(Path::new("/work/proj/.claude/commands/review.md"), true)
            .unwrap();
        assert_eq!(
            c.slug_for(Path::new("/work/proj/.claude/commands/review.md"), &cls)
                .as_deref(),
            Some("review")
        );
    }

    #[test]
    fn detects_nested_skills_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".agents/skills");
        let nested = root.join("huashu/.agents/skills/inner");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("plain/references")).unwrap();

        // A classifier whose declared root is this temp skills dir: the inner one is not declared.
        let c = Classifier::new(
            vec![ClientSpec {
                global_paths: vec![root.clone()],
                ..spec()
            }],
            vec![],
        );
        assert!(
            c.is_nested_suspect(&nested),
            "an undeclared inner skills dir means a copied tree"
        );
        assert!(!c.is_nested_suspect(&root.join("plain")));
    }

    /// The guard for the derivation: a client that only ever declared a *global* location is as
    /// well covered by nested-suspect detection as one that declared a project location. Before
    /// this was derived, both lists were hand-kept in sync here and in the matrix.
    #[test]
    fn owner_dirs_come_from_every_declared_location() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path();
        for owner in [".codex", "opencode", ".claude"] {
            std::fs::create_dir_all(proj.join("skill").join(owner).join("skills")).unwrap();
        }
        std::fs::create_dir_all(proj.join("skill").join("vendor").join("skills")).unwrap();

        let c = Classifier::new(
            vec![
                // Declares nothing inside a repository: only a global location can name its owner.
                ClientSpec {
                    id: "codex".into(),
                    global_paths: vec![PathBuf::from("/home/u/.codex/skills")],
                    project_paths: vec![],
                    command_paths: vec![PathBuf::from("/home/u/.codex/commands")],
                    project_command_paths: vec![],
                    ..spec()
                },
                // A nested layout: the owner is the segment before the leaf, not the first one.
                ClientSpec {
                    id: "opencode".into(),
                    global_paths: vec![PathBuf::from("/home/u/.config/opencode/skills")],
                    project_paths: vec![PathBuf::from(".config/opencode/skills")],
                    ..spec()
                },
            ],
            vec![],
        );
        let suspect = |owner: &str| {
            c.is_nested_suspect(&proj.join("skill").join(owner).join("skills").join("inner"))
        };
        for owner in [".codex", "opencode", ".claude"] {
            assert!(suspect(owner), "{owner} is declared by a client");
        }
        // A directory no client declared is an ordinary nested skill tree, not a suspect.
        assert!(!suspect("vendor"));
    }
}
