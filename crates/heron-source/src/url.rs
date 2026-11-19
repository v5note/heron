//! Source parsing (design §9.2, implementation doc §6.1).
//!
//! The rule that matters most: **never guess silently**. When a URL could denote more than one
//! repository boundary (GitLab subgroups), we return candidates and let the user pick.

use std::path::{Path, PathBuf};

use heron_core::Result;
use heron_git::RemoteUrl;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceKind {
    /// A git remote plus the exact location of one skill in it.
    Git {
        url: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_name: Option<String>,
        subpath: Option<String>,
    },
    /// A directory on this machine.
    Local { path: PathBuf },
    /// An archive we cannot consume yet (Phase 3).
    Archive { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedSource {
    pub original: String,
    pub kind: SourceKind,
    /// The skill directory inside the repository, when the input points at one.
    pub skill_subpath: Option<String>,
    /// Present when the input is ambiguous; the user must choose.
    pub candidates: Vec<String>,
    /// Machine-readable notes (`ref_assumed_default`, `gitlab_subgroup_possible`, …).
    pub notes: Vec<String>,
}

impl ParsedSource {
    pub fn is_ambiguous(&self) -> bool {
        !self.candidates.is_empty()
    }
}

/// Parse whatever the user pasted.
pub fn parse_source(input: &str) -> Result<ParsedSource> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(heron_core::ErrorCode::SourceAmbiguous
            .msg()
            .with("input", input)
            .with("reason", "empty")
            .into());
    }

    if raw.ends_with(".zip") || raw.ends_with(".tar.gz") || raw.ends_with(".tgz") {
        return Ok(ParsedSource {
            original: raw.to_owned(),
            kind: SourceKind::Archive {
                url: raw.to_owned(),
            },
            skill_subpath: None,
            candidates: vec![],
            notes: vec!["archive_not_supported_yet".to_owned()],
        });
    }

    let as_path = PathBuf::from(raw);
    if raw.starts_with("./") || raw.starts_with("../") {
        return Ok(local_source(raw, &as_path));
    }
    if as_path.is_dir() {
        // A git repository on disk is a *source* (you can install and track updates from it);
        // a plain directory is something to adopt.
        if as_path.join("HEAD").is_file() || as_path.join(".git").is_dir() {
            let remote = RemoteUrl::parse(raw)?;
            return Ok(ParsedSource {
                original: raw.to_owned(),
                kind: SourceKind::Git {
                    url: remote.url,
                    ref_name: None,
                    subpath: None,
                },
                skill_subpath: None,
                candidates: vec![],
                notes: vec!["local_git_repository".to_owned()],
            });
        }
        return Ok(local_source(raw, &as_path));
    }

    if let Some(rest) = raw.strip_prefix("github:") {
        return parse_github_prefixed(raw, rest);
    }

    if let Some(parsed) = parse_tree_link(raw)? {
        return Ok(parsed);
    }

    if is_gitlab_like(raw) {
        return parse_gitlab(raw);
    }

    let remote = RemoteUrl::parse(raw)?;
    Ok(ParsedSource {
        original: raw.to_owned(),
        kind: SourceKind::Git {
            url: remote.url,
            ref_name: None,
            subpath: None,
        },
        skill_subpath: None,
        candidates: vec![],
        notes: vec!["ref_assumed_default".to_owned()],
    })
}

fn local_source(raw: &str, path: &Path) -> ParsedSource {
    let skill_subpath = if path.join("SKILL.md").is_file() {
        Some(String::new())
    } else {
        None
    };
    ParsedSource {
        original: raw.to_owned(),
        kind: SourceKind::Local {
            path: path.to_path_buf(),
        },
        skill_subpath,
        candidates: vec![],
        notes: vec![],
    }
}

/// `github:owner/repo#main:skills/pdf`
fn parse_github_prefixed(raw: &str, rest: &str) -> Result<ParsedSource> {
    let (repo_part, ref_and_path) = match rest.split_once('#') {
        Some((repo, tail)) => (repo, Some(tail)),
        None => (rest, None),
    };
    let remote = RemoteUrl::parse(repo_part)?;
    let (ref_name, subpath) = match ref_and_path {
        Some(tail) => match tail.split_once(':') {
            Some((r, p)) => (Some(r.to_owned()), Some(p.trim_matches('/').to_owned())),
            None => (Some(tail.to_owned()), None),
        },
        None => (None, None),
    };
    Ok(ParsedSource {
        original: raw.to_owned(),
        kind: SourceKind::Git {
            url: remote.url,
            ref_name,
            subpath: subpath.clone(),
        },
        skill_subpath: subpath,
        candidates: vec![],
        notes: vec![],
    })
}

fn parse_tree_link(raw: &str) -> Result<Option<ParsedSource>> {
    let marker = ["/tree/", "/blob/"]
        .iter()
        .find(|m| raw.contains(**m))
        .copied();
    let Some(marker) = marker else {
        return Ok(None);
    };

    let (repo_url, tail) = raw.split_once(marker).unwrap();
    let segments: Vec<&str> = tail.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Ok(None);
    }

    let ref_name = segments[0].to_owned();
    let mut path_segments = segments[1..].to_vec();
    if marker == "/blob/" && path_segments.last() == Some(&"SKILL.md") {
        path_segments.pop();
    }
    let subpath = if path_segments.is_empty() {
        None
    } else {
        Some(path_segments.join("/"))
    };

    let remote = RemoteUrl::parse(repo_url)?;
    Ok(Some(ParsedSource {
        original: raw.to_owned(),
        kind: SourceKind::Git {
            url: remote.url,
            ref_name: Some(ref_name),
            subpath: subpath.clone(),
        },
        skill_subpath: subpath,
        candidates: vec![],
        notes: vec!["ref_from_url".to_owned()],
    }))
}

fn is_gitlab_like(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.starts_with("gitlab:") || lower.contains("gitlab.com") || lower.contains("gitlab.")
}

/// GitLab allows deep subgroups, so the URL alone cannot tell us where the repository ends.
fn parse_gitlab(raw: &str) -> Result<ParsedSource> {
    let stripped = raw.strip_prefix("gitlab:").unwrap_or(raw);
    let without_git = stripped.trim_end_matches('/').trim_end_matches(".git");
    let (host, path) = match without_git.split_once("://") {
        Some((_, rest)) => match rest.split_once('/') {
            Some((h, p)) => (h.to_owned(), p.to_owned()),
            None => (rest.to_owned(), String::new()),
        },
        None => (
            "gitlab.com".to_owned(),
            without_git.trim_start_matches('/').to_owned(),
        ),
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(heron_core::ErrorCode::SourceAmbiguous
            .msg()
            .with("input", raw)
            .with("reason", "gitlab_path_too_short")
            .into());
    }

    let candidates: Vec<String> = (2..=segments.len())
        .map(|end| format!("https://{host}/{}", segments[..end].join("/")))
        .collect();

    let preferred = candidates.last().cloned().unwrap_or_default();
    Ok(ParsedSource {
        original: raw.to_owned(),
        kind: SourceKind::Git {
            url: format!("{preferred}.git"),
            ref_name: None,
            subpath: None,
        },
        skill_subpath: None,
        candidates,
        notes: vec!["gitlab_subgroup_possible".to_owned()],
    })
}

/// When a remote is available, disambiguate candidates by asking it.
pub fn disambiguate(candidates: &[String], probe: impl Fn(&str) -> bool) -> Option<String> {
    candidates.iter().find(|c| probe(c)).cloned()
}

/// A short human label for a parsed source.
pub fn describe(parsed: &ParsedSource) -> String {
    match &parsed.kind {
        SourceKind::Git {
            url,
            ref_name,
            subpath,
        } => format!(
            "{url} @ {} {}",
            ref_name.as_deref().unwrap_or("<default>"),
            subpath.as_deref().unwrap_or("<repo root>")
        ),
        SourceKind::Local { path } => format!("local {}", path.display()),
        SourceKind::Archive { url } => format!("archive {url}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_shorthand_uses_default_ref() {
        let p = parse_source("anthropics/skills").unwrap();
        assert_eq!(
            p.kind,
            SourceKind::Git {
                url: "https://github.com/anthropics/skills.git".into(),
                ref_name: None,
                subpath: None
            }
        );
        assert!(p.notes.contains(&"ref_assumed_default".to_owned()));
    }

    #[test]
    fn tree_link_carries_ref_and_subpath() {
        let p = parse_source("https://github.com/anthropics/skills/tree/main/skills/pdf").unwrap();
        assert_eq!(
            p.kind,
            SourceKind::Git {
                url: "https://github.com/anthropics/skills.git".into(),
                ref_name: Some("main".into()),
                subpath: Some("skills/pdf".into())
            }
        );
        assert_eq!(p.skill_subpath.as_deref(), Some("skills/pdf"));
    }

    #[test]
    fn blob_link_to_skill_md_points_at_the_directory() {
        let p = parse_source("https://github.com/o/r/blob/v1.2.0/skills/logo/SKILL.md").unwrap();
        assert_eq!(p.skill_subpath.as_deref(), Some("skills/logo"));
        match p.kind {
            SourceKind::Git { ref_name, .. } => assert_eq!(ref_name.as_deref(), Some("v1.2.0")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn gitlab_url_reports_candidates_instead_of_guessing() {
        let p = parse_source("https://gitlab.com/team/sub/repo").unwrap();
        assert!(p.is_ambiguous());
        assert_eq!(
            p.candidates,
            vec![
                "https://gitlab.com/team/sub".to_owned(),
                "https://gitlab.com/team/sub/repo".to_owned()
            ]
        );
        assert!(p.notes.contains(&"gitlab_subgroup_possible".to_owned()));
    }

    #[test]
    fn gitlab_prefixed_form_is_accepted() {
        let p = parse_source("gitlab:group/sub/repo").unwrap();
        assert!(p.is_ambiguous());
        assert!(matches!(p.kind, SourceKind::Git { .. }));
    }

    #[test]
    fn github_prefixed_form_supports_ref_and_subpath() {
        let p = parse_source("github:owner/repo#main:skills/pdf").unwrap();
        assert_eq!(
            p.kind,
            SourceKind::Git {
                url: "https://github.com/owner/repo.git".into(),
                ref_name: Some("main".into()),
                subpath: Some("skills/pdf".into())
            }
        );
    }

    #[test]
    fn local_directories_are_recognised() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SKILL.md"), "x").unwrap();
        let p = parse_source(tmp.path().to_str().unwrap()).unwrap();
        assert!(matches!(p.kind, SourceKind::Local { .. }));
        assert_eq!(p.skill_subpath.as_deref(), Some(""));
    }

    #[test]
    fn archives_are_flagged_not_misparsed() {
        let p = parse_source("https://example.com/skills.zip").unwrap();
        assert!(matches!(p.kind, SourceKind::Archive { .. }));
        assert!(p.notes.contains(&"archive_not_supported_yet".to_owned()));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_source("   ").is_err());
        assert!(parse_source("oneword").is_err());
    }

    #[test]
    fn describe_is_readable() {
        let p = parse_source("https://github.com/o/r/tree/main/skills/pdf").unwrap();
        assert_eq!(describe(&p), "https://github.com/o/r.git @ main skills/pdf");
    }
}
