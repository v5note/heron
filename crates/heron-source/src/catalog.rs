//! Discovering which skills a repository offers (design §9.3).
//!
//! Priority: `.claude-plugin/marketplace.json` (compatible with the Claude ecosystem) →
//! `heron.source.toml` (ours) → `.claude-plugin/plugin.json` (a single plugin naming its own
//! skills) → a bounded scan for `SKILL.md`. The first three are *authored* catalogs — the author
//! has already told us which directories are skills — so they outrank the scan, which is only a
//! safety net to keep any Git repository usable on day one.

use std::collections::BTreeMap;

use heron_core::{ErrorCode, Result};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    MarketplaceJson,
    HeronToml,
    PluginJson,
    ScanFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogItem {
    pub slug: String,
    pub path: String,
    pub version: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalog {
    pub source: CatalogSource,
    pub items: Vec<CatalogItem>,
}

/// Files in the repository, as `(path, content)` for text files we care about and `(path, None)`
/// for the rest. The caller (service layer) obtains these from the git tree.
pub struct RepoFiles<'a> {
    pub files: &'a [String],
    pub read: &'a dyn Fn(&str) -> Option<String>,
}

impl<'a> RepoFiles<'a> {
    fn has(&self, path: &str) -> bool {
        self.files.iter().any(|f| f == path)
    }
    fn read(&self, path: &str) -> Option<String> {
        (self.read)(path)
    }
}

/// Load the catalog, trying authored catalogs first and falling back to a scan.
pub fn load(files: &RepoFiles<'_>) -> Result<Catalog> {
    if files.has(".claude-plugin/marketplace.json") {
        if let Some(text) = files.read(".claude-plugin/marketplace.json") {
            if let Ok(items) = parse_marketplace(&text, files) {
                if !items.is_empty() {
                    return Ok(Catalog {
                        source: CatalogSource::MarketplaceJson,
                        items,
                    });
                }
            }
        }
    }
    if files.has("heron.source.toml") {
        if let Some(text) = files.read("heron.source.toml") {
            if let Ok(items) = parse_heron_toml(&text) {
                if !items.is_empty() {
                    return Ok(Catalog {
                        source: CatalogSource::HeronToml,
                        items,
                    });
                }
            }
        }
    }
    if files.has(".claude-plugin/plugin.json") {
        if let Some(text) = files.read(".claude-plugin/plugin.json") {
            if let Ok(items) = parse_plugin(&text, files) {
                if !items.is_empty() {
                    return Ok(Catalog {
                        source: CatalogSource::PluginJson,
                        items,
                    });
                }
            }
        }
    }
    Ok(Catalog {
        source: CatalogSource::ScanFallback,
        items: scan_for_skills(files),
    })
}

/// A `.claude-plugin/plugin.json` manifest: one plugin that names its own skill directories.
///
/// This is the single-plugin counterpart of `marketplace.json`, and it is the difference between
/// installing a repository and guessing at one. `mouse-lin/finesse-skill` ships five copies of
/// `SKILL.md` — the real `skills/finesse-ui` plus four per-client projections — while its manifest
/// states plainly that the one that matters is `./skills/finesse-ui`.
///
/// Entries that point at a path with no `SKILL.md` are dropped rather than errored: a manifest may
/// well list `commands/`-style directories we have no business treating as skills.
pub fn parse_plugin(text: &str, files: &RepoFiles<'_>) -> Result<Vec<CatalogItem>> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::CatalogParse
                .msg()
                .with("file", "plugin.json")
                .with("detail", e.to_string()),
        )
    })?;
    let name = value
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default();
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let description = value
        .get("description")
        .and_then(|d| d.as_str())
        .map(|s| s.to_owned());
    let Some(entries) = value.get("skills").and_then(|s| s.as_array()) else {
        return Ok(Vec::new());
    };
    let mut items = Vec::new();
    for entry in entries {
        let Some(relative) = entry.as_str().and_then(normalize_plugin_path) else {
            continue;
        };
        let probe = if relative.is_empty() {
            "SKILL.md".to_owned()
        } else {
            format!("{relative}/SKILL.md")
        };
        if !files.has(&probe) {
            continue;
        }
        let slug = relative
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .or(if name.is_empty() { None } else { Some(name) })
            .unwrap_or_default()
            .to_owned();
        if slug.is_empty() {
            continue;
        }
        items.push(CatalogItem {
            slug,
            path: relative,
            version: version.clone(),
            description: description.clone(),
        });
    }
    Ok(items)
}

/// `./skills/finesse-ui` → `skills/finesse-ui`. Repository-root paths collapse to `""`; anything
/// absolute or climbing out of the checkout is rejected rather than resolved.
fn normalize_plugin_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let relative = trimmed
        .strip_prefix("./")
        .or_else(|| trimmed.strip_prefix('/'))
        .unwrap_or(trimmed);
    let relative = relative.trim_end_matches('/');
    if relative == "." {
        return Some(String::new());
    }
    if relative.is_empty() || relative.starts_with("..") || relative.contains("/../") {
        return None;
    }
    Some(relative.to_owned())
}

/// A `.claude-plugin/marketplace.json` catalog. Each plugin may hold a `skills/` subtree.
pub fn parse_marketplace(text: &str, files: &RepoFiles<'_>) -> Result<Vec<CatalogItem>> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::CatalogParse
                .msg()
                .with("file", "marketplace.json")
                .with("detail", e.to_string()),
        )
    })?;
    let mut items = Vec::new();
    let Some(plugins) = value.get("plugins").and_then(|p| p.as_array()) else {
        return Ok(items);
    };
    for plugin in plugins {
        let name = plugin
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        let version = plugin
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        let description = plugin
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_owned());

        let base = match plugin.get("source") {
            Some(serde_json::Value::String(relative)) if relative.starts_with("./") => relative
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_owned(),
            // Object sources point at *other* repositories: out of scope for this catalog.
            Some(serde_json::Value::Object(_)) => continue,
            _ => continue,
        };

        // `<base>/skills/<slug>/SKILL.md`
        for path in files
            .files
            .iter()
            .filter(|f| f.starts_with(&format!("{base}/skills/")) && f.ends_with("/SKILL.md"))
        {
            let skill_dir = path.trim_end_matches("/SKILL.md");
            let slug = skill_dir.rsplit('/').next().unwrap_or_default().to_owned();
            if slug.is_empty() {
                continue;
            }
            items.push(CatalogItem {
                slug,
                path: skill_dir.to_owned(),
                version: version.clone(),
                description: description.clone().or_else(|| Some(name.to_owned())),
            });
        }
    }
    Ok(items)
}

/// Our own `heron.source.toml`.
pub fn parse_heron_toml(text: &str) -> Result<Vec<CatalogItem>> {
    let value: toml::Value = toml::from_str(text).map_err(|e| {
        heron_core::HeronError::from(
            ErrorCode::CatalogParse
                .msg()
                .with("file", "heron.source.toml")
                .with("detail", e.to_string()),
        )
    })?;
    let mut items = Vec::new();
    let Some(list) = value.get("skill").and_then(|s| s.as_array()) else {
        return Ok(items);
    };
    for entry in list {
        let path = entry
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        let slug = entry
            .get("slug")
            .and_then(|s| s.as_str())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_owned());
        items.push(CatalogItem {
            slug,
            path: path.trim_end_matches('/').to_owned(),
            version: entry
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
            description: entry
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_owned()),
        });
    }
    Ok(items)
}

/// Bounded scan: every directory that directly contains a `SKILL.md`, ignoring noise.
///
/// Projections lose to sources. An author who commits `.trae/skills/x/`, `.claude/skills/x/` and
/// `skills/x/` side by side has checked in *copies* of one skill, one per client directory — and
/// projection is Heron's own job, not something to install. So a dot-directory hit is only ever
/// used when the repository has nothing else, which keeps a `.claude/skills/`-only repository
/// installable instead of erroring that no skill exists.
pub fn scan_for_skills(files: &RepoFiles<'_>) -> Vec<CatalogItem> {
    const IGNORED: &[&str] = &[
        "node_modules/",
        "vendor/",
        "target/",
        "dist/",
        ".github/",
        "tests/fixtures/",
    ];
    let mut sources: BTreeMap<String, ()> = BTreeMap::new();
    let mut projections: BTreeMap<String, ()> = BTreeMap::new();
    for path in files.files.iter() {
        if !path.ends_with("/SKILL.md") && path != "SKILL.md" {
            continue;
        }
        if IGNORED.iter().any(|prefix| path.contains(prefix)) {
            continue;
        }
        let dir = path
            .trim_end_matches("SKILL.md")
            .trim_end_matches('/')
            .to_owned();
        if dir.matches('/').count() > 5 {
            continue;
        }
        if dir
            .split('/')
            .next()
            .is_some_and(|head| head.starts_with('.'))
        {
            projections.insert(dir, ());
        } else {
            sources.insert(dir, ());
        }
    }
    let by_dir = if sources.is_empty() {
        projections
    } else {
        sources
    };
    by_dir
        .into_keys()
        .map(|path| {
            let slug = if path.is_empty() {
                "root".to_owned()
            } else {
                path.rsplit('/').next().unwrap_or(&path).to_owned()
            };
            CatalogItem {
                slug,
                path,
                version: None,
                description: None,
            }
        })
        .collect()
}

/// Find the skill directory an input refers to.
///
/// Accepts: an explicit subpath, a directory, or a `SKILL.md` file path. When the input is the
/// repository root and it contains exactly one skill, that skill is used.
pub fn resolve_skill_dir(
    requested: Option<&str>,
    catalog: &Catalog,
    skill_md: &dyn Fn(&str) -> bool,
) -> Result<String> {
    if let Some(sub) = requested.filter(|s| !s.is_empty()) {
        let normalized = sub.trim_matches('/');
        let trimmed = normalized.strip_suffix("/SKILL.md").unwrap_or(normalized);
        if skill_md(trimmed) || catalog.items.iter().any(|i| i.path == trimmed) {
            return Ok(trimmed.to_owned());
        }
        return Err(heron_core::ErrorCode::RefNotFound
            .msg()
            .with("ref", trimmed.to_owned())
            .with("detail", "no SKILL.md at that path")
            .into());
    }

    match catalog.items.len() {
        0 => Err(heron_core::ErrorCode::Frontmatter
            .msg()
            .with("detail", "no SKILL.md found in this repository")
            .into()),
        1 => Ok(catalog.items[0].path.clone()),
        _ => Err(heron_core::ErrorCode::SourceAmbiguous
            .msg()
            .with("reason", "multiple_skills")
            .with("count", catalog.items.len().to_string())
            .with(
                "examples",
                catalog
                    .items
                    .iter()
                    .take(5)
                    .map(|i| i.path.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[&str], contents: &[(&str, &str)]) -> (Vec<String>, BTreeMap<String, String>) {
        let owned: Vec<String> = list.iter().map(|s| (*s).to_owned()).collect();
        let map: BTreeMap<String, String> = contents
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        (owned, map)
    }

    #[test]
    fn heron_toml_catalog_is_read() {
        let text = r#"
schema = 1
name = "Team"

[[skill]]
slug = "code-review"
path = "skills/code-review"
version = "1.2.0"

[[skill]]
path = "skills/wechat-mp"
"#;
        let items = parse_heron_toml(text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].slug, "code-review");
        assert_eq!(items[0].version.as_deref(), Some("1.2.0"));
        assert_eq!(
            items[1].slug, "wechat-mp",
            "slug defaults to the last path segment"
        );
    }

    #[test]
    fn marketplace_json_exposes_relative_plugins() {
        let text = r#"{
            "name": "demo",
            "owner": {"name": "x"},
            "plugins": [
                {"name": "formatter", "source": "./plugins/formatter", "version": "2.0.0"},
                {"name": "external", "source": {"source": "github", "repo": "o/r"}},
                {"name": "root-plug", "source": "."}
            ]
        }"#;
        let (list, contents) = files(
            &["plugins/formatter/skills/format/SKILL.md", "other/SKILL.md"],
            &[],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let items = parse_marketplace(text, &repo).unwrap();
        assert_eq!(
            items.len(),
            1,
            "object sources and non-skill dirs are skipped: {items:?}"
        );
        assert_eq!(items[0].slug, "format");
        assert_eq!(items[0].path, "plugins/formatter/skills/format");
        assert_eq!(items[0].version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn fallback_scan_finds_root_and_nested_skills() {
        let (list, contents) = files(
            &[
                "skills/wechat-mp/SKILL.md",
                "skills/tdd/SKILL.md",
                "SKILL.md",
                "node_modules/x/SKILL.md",
                "tests/fixtures/y/SKILL.md",
            ],
            &[],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let items = scan_for_skills(&repo);
        let slugs: Vec<_> = items.iter().map(|i| i.slug.as_str()).collect();
        assert!(slugs.contains(&"wechat-mp"));
        assert!(slugs.contains(&"tdd"));
        assert!(slugs.contains(&"root"));
        assert!(!slugs.contains(&"x"), "node_modules is ignored");
        assert!(!slugs.contains(&"y"), "test fixtures are ignored");
    }

    /// A `RepoFiles` over `list`, with `contents` readable. Owns both sides so the borrow checks.
    fn repo_of(
        list: &[&str],
        contents: &[(&str, &str)],
    ) -> (Vec<String>, BTreeMap<String, String>) {
        files(list, contents)
    }

    fn paths(items: &[CatalogItem]) -> Vec<&str> {
        items.iter().map(|i| i.path.as_str()).collect()
    }

    #[test]
    fn scan_prefers_the_source_over_committed_client_projections() {
        // The real mouse-lin/finesse-skill: one `skills/` directory the author maintains, four
        // copies projected into client directories and committed alongside it.
        let (list, contents) = repo_of(
            &[
                "skills/finesse-ui/SKILL.md",
                ".trae/skills/finesse-ui/SKILL.md",
                ".trae-cn/skills/finesse-ui/SKILL.md",
                ".doubao/skills/finesse-ui/SKILL.md",
                ".codebuddy/skills/finesse-ui/SKILL.md",
            ],
            &[],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        assert_eq!(
            paths(&scan_for_skills(&repo)),
            vec!["skills/finesse-ui"],
            "dot-directory copies must not make a single-skill repository look ambiguous"
        );
        assert_eq!(
            resolve_skill_dir(None, &load(&repo).unwrap(), &|_| false).unwrap(),
            "skills/finesse-ui",
            "the bare repository URL now installs without a subpath"
        );
    }

    #[test]
    fn scan_falls_back_to_projections_when_the_repository_has_nothing_else() {
        // A `.claude/skills/`-only repository is still a repository: dropping its only candidate
        // would turn "install this" into "no SKILL.md found in this repository".
        let (list, contents) = repo_of(&[".claude/skills/only/SKILL.md", "README.md"], &[]);
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        assert_eq!(paths(&scan_for_skills(&repo)), vec![".claude/skills/only"]);
    }

    #[test]
    fn root_skill_is_never_treated_as_a_projection() {
        let (list, contents) = repo_of(&["SKILL.md", "package.json"], &[]);
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        assert_eq!(paths(&scan_for_skills(&repo)), vec![""]);
    }

    #[test]
    fn plugin_json_names_the_canonical_skill() {
        let manifest = r#"{
            "name": "finesse-ui",
            "description": "Never-cheap interfaces.",
            "version": "0.22.0",
            "skills": ["./skills/finesse-ui"]
        }"#;
        let (list, contents) = repo_of(
            &[
                ".claude-plugin/plugin.json",
                "skills/finesse-ui/SKILL.md",
                ".trae/skills/finesse-ui/SKILL.md",
            ],
            &[(".claude-plugin/plugin.json", manifest)],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let catalog = load(&repo).unwrap();
        assert_eq!(catalog.source, CatalogSource::PluginJson);
        assert_eq!(paths(&catalog.items), vec!["skills/finesse-ui"]);
        assert_eq!(catalog.items[0].slug, "finesse-ui");
        assert_eq!(catalog.items[0].version.as_deref(), Some("0.22.0"));
        assert_eq!(
            catalog.items[0].description.as_deref(),
            Some("Never-cheap interfaces.")
        );
    }

    #[test]
    fn plugin_json_drops_missing_paths_and_rejects_escapes() {
        let manifest = r#"{
            "name": "p",
            "skills": ["./skills/real", "./skills/ghost", "/etc/passwd", "../../outside", "./"]
        }"#;
        let (list, contents) = repo_of(
            &[".claude-plugin/plugin.json", "skills/real/SKILL.md"],
            &[(".claude-plugin/plugin.json", manifest)],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let catalog = load(&repo).unwrap();
        assert_eq!(
            paths(&catalog.items),
            vec!["skills/real"],
            "only the declared path that really holds a SKILL.md survives"
        );
    }

    #[test]
    fn plugin_json_without_skills_yields_to_the_scan() {
        let (list, contents) = repo_of(
            &[
                ".claude-plugin/plugin.json",
                "skills/solo/SKILL.md",
                "docs/x/SKILL.md",
            ],
            &[(
                ".claude-plugin/plugin.json",
                r#"{"name":"p","description":"d"}"#,
            )],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let catalog = load(&repo).unwrap();
        assert_eq!(catalog.source, CatalogSource::ScanFallback);
        assert_eq!(paths(&catalog.items), vec!["docs/x", "skills/solo"]);
    }

    #[test]
    fn plugin_json_never_outranks_heron_source_toml() {
        let (list, contents) = repo_of(
            &[
                ".claude-plugin/plugin.json",
                "heron.source.toml",
                "skills/from-toml/SKILL.md",
            ],
            &[
                (
                    ".claude-plugin/plugin.json",
                    r#"{"name":"p","skills":["./skills/from-plugin"]}"#,
                ),
                (
                    "heron.source.toml",
                    "[[skill]]\npath = \"skills/from-toml\"\n",
                ),
            ],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let catalog = load(&repo).unwrap();
        assert_eq!(catalog.source, CatalogSource::HeronToml);
        assert_eq!(paths(&catalog.items), vec!["skills/from-toml"]);
    }

    #[test]
    fn catalog_priority_prefers_authored_files() {
        let (list, contents) = files(
            &[
                ".claude-plugin/marketplace.json",
                "plugins/p/skills/s/SKILL.md",
            ],
            &[(
                ".claude-plugin/marketplace.json",
                r#"{"name":"d","owner":{"name":"x"},"plugins":[{"name":"p","source":"./plugins/p"}]}"#,
            )],
        );
        let map = contents;
        let read = move |p: &str| map.get(p).cloned();
        let repo = RepoFiles {
            files: &list,
            read: &read,
        };
        let catalog = load(&repo).unwrap();
        assert_eq!(catalog.source, CatalogSource::MarketplaceJson);
        assert_eq!(catalog.items[0].slug, "s");
    }

    #[test]
    fn resolve_skill_dir_handles_subpath_and_ambiguity() {
        let catalog = Catalog {
            source: CatalogSource::ScanFallback,
            items: vec![
                CatalogItem {
                    slug: "wechat-mp".into(),
                    path: "skills/wechat-mp".into(),
                    version: None,
                    description: None,
                },
                CatalogItem {
                    slug: "tdd".into(),
                    path: "skills/tdd".into(),
                    version: None,
                    description: None,
                },
            ],
        };
        let exists = |p: &str| p == "skills/wechat-mp" || p == "skills/tdd";
        assert_eq!(
            resolve_skill_dir(Some("skills/wechat-mp"), &catalog, &exists).unwrap(),
            "skills/wechat-mp"
        );
        assert_eq!(
            resolve_skill_dir(Some("skills/wechat-mp/SKILL.md"), &catalog, &exists).unwrap(),
            "skills/wechat-mp",
            "a file path resolves to its directory"
        );
        let err = resolve_skill_dir(None, &catalog, &exists).unwrap_err();
        assert_eq!(err.code(), "E_SOURCE_AMBIGUOUS");
        assert_eq!(
            resolve_skill_dir(Some("skills/nope"), &catalog, &exists)
                .unwrap_err()
                .code(),
            "E_REF_NOT_FOUND"
        );
    }
}
