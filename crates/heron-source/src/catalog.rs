//! Discovering which skills a repository offers (design §9.3).
//!
//! Priority: `.claude-plugin/marketplace.json` (compatible with the Claude ecosystem) →
//! `heron.source.toml` (ours) → a bounded scan for `SKILL.md`. The first two are *authored*
//! catalogs; the scan is a safety net so any Git repository is usable on day one.

use std::collections::BTreeMap;

use heron_core::{ErrorCode, Result};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    MarketplaceJson,
    HeronToml,
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
    Ok(Catalog {
        source: CatalogSource::ScanFallback,
        items: scan_for_skills(files),
    })
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
pub fn scan_for_skills(files: &RepoFiles<'_>) -> Vec<CatalogItem> {
    const IGNORED: &[&str] = &[
        "node_modules/",
        "vendor/",
        "target/",
        "dist/",
        ".github/",
        "tests/fixtures/",
    ];
    let mut by_dir: BTreeMap<String, ()> = BTreeMap::new();
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
        by_dir.insert(dir, ());
    }
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
path = "skills/pdf"
"#;
        let items = parse_heron_toml(text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].slug, "code-review");
        assert_eq!(items[0].version.as_deref(), Some("1.2.0"));
        assert_eq!(
            items[1].slug, "pdf",
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
                "skills/pdf/SKILL.md",
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
        assert!(slugs.contains(&"pdf"));
        assert!(slugs.contains(&"tdd"));
        assert!(slugs.contains(&"root"));
        assert!(!slugs.contains(&"x"), "node_modules is ignored");
        assert!(!slugs.contains(&"y"), "test fixtures are ignored");
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
                    slug: "pdf".into(),
                    path: "skills/pdf".into(),
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
        let exists = |p: &str| p == "skills/pdf" || p == "skills/tdd";
        assert_eq!(
            resolve_skill_dir(Some("skills/pdf"), &catalog, &exists).unwrap(),
            "skills/pdf"
        );
        assert_eq!(
            resolve_skill_dir(Some("skills/pdf/SKILL.md"), &catalog, &exists).unwrap(),
            "skills/pdf",
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
