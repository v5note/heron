//! `SKILL.md` frontmatter parsing with a lenient fallback.
//!
//! Level 1: standard YAML. Level 2: a line-oriented scalar extractor that tolerates the
//! malformed frontmatter seen in the wild (unquoted colons, block scalars, trailing spaces).
//! The scanner must never fail because a third-party skill has sloppy metadata.

use std::collections::BTreeMap;

use serde::Serialize;

/// Limit for the frontmatter block; anything beyond is treated as body text.
pub const FRONTMATTER_SCAN_LIMIT: usize = 64 * 1024;

/// Agent Skills limits (spec, §5.3 of the implementation doc).
pub const MAX_NAME_LEN: usize = 64;
pub const MAX_DESCRIPTION_LEN: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Parsed by a real YAML parser.
    Full,
    /// Recovered by the scalar extractor.
    Partial,
    /// No usable frontmatter.
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParseIssue {
    pub code: &'static str,
    pub field: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub allowed_tools: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub raw_yaml: String,
    pub confidence: Confidence,
}

impl Frontmatter {
    pub fn is_ok(&self) -> bool {
        matches!(self.confidence, Confidence::Full | Confidence::Partial)
    }

    /// `metadata.version` of any spelling (`version`, `Version`).
    pub fn metadata_version(&self) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("version"))
            .map(|(_, v)| v.as_str())
    }

    /// Spec validation. Issues are advisory: they never block adoption (implementation §5.3).
    pub fn validate(&self) -> Vec<ParseIssue> {
        let mut issues = Vec::new();
        match self.name.as_deref() {
            None => issues.push(issue("E_FRONTMATTER", Some("name"), "missing")),
            Some(name) => {
                if name.chars().count() > MAX_NAME_LEN {
                    issues.push(issue("E_FRONTMATTER", Some("name"), "too_long"));
                }
                if !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    issues.push(issue("W_FRONTMATTER", Some("name"), "charset"));
                }
                if name.starts_with('-') || name.ends_with('-') {
                    issues.push(issue("W_FRONTMATTER", Some("name"), "edge_hyphen"));
                }
            }
        }
        match self.description.as_deref() {
            None => issues.push(issue("E_FRONTMATTER", Some("description"), "missing")),
            Some(d) if d.trim().is_empty() => {
                issues.push(issue("E_FRONTMATTER", Some("description"), "empty"))
            }
            Some(d) if d.chars().count() > MAX_DESCRIPTION_LEN => {
                issues.push(issue("W_FRONTMATTER", Some("description"), "too_long"))
            }
            Some(_) => {}
        }
        if self.metadata_version().is_none() {
            issues.push(issue("W_FRONTMATTER", Some("metadata.version"), "missing"));
        }
        issues
    }
}

fn issue(code: &'static str, field: Option<&str>, detail: &str) -> ParseIssue {
    ParseIssue {
        code,
        field: field.map(str::to_owned),
        detail: detail.to_owned(),
    }
}

/// Parse the frontmatter of a `SKILL.md`. Returns the parsed metadata and never fails:
/// malformed input degrades to `Confidence::None`.
pub fn parse_skill_md(bytes: &[u8]) -> (Frontmatter, Vec<ParseIssue>) {
    let head = &bytes[..bytes.len().min(FRONTMATTER_SCAN_LIMIT)];
    let text = heron_core::hash::normalize_text_bytes(head);
    let text = String::from_utf8_lossy(&text).into_owned();

    let Some((yaml, _body)) = split_frontmatter(&text) else {
        return (
            Frontmatter {
                confidence: Confidence::None,
                ..Default::default()
            },
            vec![issue("E_FRONTMATTER", None, "no_frontmatter")],
        );
    };

    if let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str::<serde_yaml::Value>(&yaml) {
        let fm = from_mapping(&map, &yaml, Confidence::Full);
        let issues = fm.validate();
        return (fm, issues);
    }

    let fm = extract_scalars(&yaml);
    let issues = fm.validate();
    (fm, issues)
}

/// Split `---\n...\n---\n` into (yaml, body).
pub fn split_frontmatter(text: &str) -> Option<(String, String)> {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = trimmed.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut yaml = String::new();
    let mut rest = String::new();
    let mut closed = false;
    for line in lines {
        if !closed {
            if line.trim_end() == "---" {
                closed = true;
                continue;
            }
            yaml.push_str(line);
        } else {
            rest.push_str(line);
        }
    }
    if !closed {
        return None;
    }
    Some((yaml, rest))
}

fn from_mapping(map: &serde_yaml::Mapping, raw: &str, confidence: Confidence) -> Frontmatter {
    let get_str = |key: &str| -> Option<String> {
        map.get(serde_yaml::Value::String(key.to_owned()))
            .and_then(scalar_to_string)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    let mut metadata = BTreeMap::new();
    if let Some(serde_yaml::Value::Mapping(m)) =
        map.get(serde_yaml::Value::String("metadata".to_owned()))
    {
        for (k, v) in m {
            if let (Some(k), Some(v)) = (k.as_str(), scalar_to_string(v)) {
                metadata.insert(k.to_owned(), v.trim().to_owned());
            }
        }
    }
    Frontmatter {
        name: get_str("name"),
        description: get_str("description"),
        license: get_str("license"),
        compatibility: get_str("compatibility"),
        allowed_tools: get_str("allowed-tools"),
        metadata,
        raw_yaml: raw.to_owned(),
        confidence,
    }
}

fn scalar_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Level-2 parser: top-level `key: value` pairs, quoted or folded block scalars.
fn extract_scalars(yaml: &str) -> Frontmatter {
    let mut fm = Frontmatter {
        raw_yaml: yaml.to_owned(),
        confidence: Confidence::Partial,
        ..Default::default()
    };
    let mut current_metadata_key: Option<String> = None;
    let mut lines = yaml.lines().peekable();

    while let Some(line) = lines.next() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = split_key_value(trimmed) else {
            continue;
        };

        // Nested keys (metadata: <subkey>: value)
        if indent > 0 {
            if let Some(owner) = &current_metadata_key {
                if owner == "metadata" {
                    let value = read_scalar_value(&mut lines, indent, raw_value);
                    fm.metadata.insert(key.to_owned(), value);
                }
            }
            continue;
        }

        if key == "metadata" && raw_value.is_empty() {
            current_metadata_key = Some("metadata".to_owned());
            continue;
        }
        current_metadata_key = None;

        let value = read_scalar_value(&mut lines, indent, raw_value);
        match key {
            "name" => fm.name = non_empty(value),
            "description" => fm.description = non_empty(value),
            "license" => fm.license = non_empty(value),
            "compatibility" => fm.compatibility = non_empty(value),
            "allowed-tools" => fm.allowed_tools = non_empty(value),
            _ => {}
        }
    }
    fm
}

fn non_empty(v: String) -> Option<String> {
    if v.trim().is_empty() {
        None
    } else {
        Some(v.trim().to_owned())
    }
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) && !key.contains('-') {
        // tolerate quoted keys and hyphenated keys, reject prose like "Note : x"
    }
    Some((key, value.trim()))
}

/// Read a scalar that may continue on the following lines (quoted, `|`, `>`).
fn read_scalar_value<'a, I>(
    lines: &mut std::iter::Peekable<I>,
    key_indent: usize,
    first: &str,
) -> String
where
    I: Iterator<Item = &'a str>,
{
    if first.starts_with('"') || first.starts_with('\'') {
        let quote = first.chars().next().unwrap();
        let mut buf = first[1..].to_owned();
        while !ends_with_quote(&buf, quote) {
            match lines.next() {
                Some(next) => {
                    buf.push(' ');
                    buf.push_str(next.trim());
                }
                None => break,
            }
        }
        return buf.trim_end_matches(quote).trim().to_owned();
    }
    if first.starts_with('|') || first.starts_with('>') {
        let mut parts = Vec::new();
        while let Some(next) = lines.peek() {
            let indented = next.len() - next.trim_start().len();
            if next.trim().is_empty() || indented > key_indent {
                let line = lines.next().unwrap().trim();
                if !line.is_empty() {
                    parts.push(line.to_owned());
                }
            } else {
                break;
            }
        }
        let joiner = if first.starts_with('|') { "\n" } else { " " };
        return parts.join(joiner);
    }
    first
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned()
}

fn ends_with_quote(s: &str, quote: char) -> bool {
    s.ends_with(quote) && !s.ends_with(&format!("\\{quote}"))
}

/// Convenience: parse a file, mapping IO errors to a `Msg`.
pub fn parse_file(
    path: &std::path::Path,
) -> std::result::Result<(Frontmatter, Vec<ParseIssue>), heron_core::HeronError> {
    let bytes = heron_platform::atomic::read_head(path, FRONTMATTER_SCAN_LIMIT)
        .map_err(heron_core::HeronError::from)?;
    Ok(parse_skill_md(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "---\nname: wechat-mp\ndescription: Publish articles to WeChat\nmetadata:\n  version: \"1.4.0\"\n---\n\n# WeChat MP\n";

    #[test]
    fn parses_standard_frontmatter() {
        let (fm, issues) = parse_skill_md(GOOD.as_bytes());
        assert_eq!(fm.confidence, Confidence::Full);
        assert_eq!(fm.name.as_deref(), Some("wechat-mp"));
        assert_eq!(fm.metadata_version(), Some("1.4.0"));
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert!(fm.is_ok());
    }

    #[test]
    fn handles_crlf_and_bom() {
        let raw = "\u{feff}---\r\nname: tdd\r\ndescription: write tests first\r\n---\r\n";
        let (fm, _) = parse_skill_md(raw.as_bytes());
        assert_eq!(fm.name.as_deref(), Some("tdd"));
        assert_eq!(fm.description.as_deref(), Some("write tests first"));
    }

    #[test]
    fn falls_back_to_scalar_extraction() {
        // Unquoted colon inside the value breaks strict YAML for some parsers.
        let raw = "---\nname: review\ndescription: Review code: find bugs\n---\nbody\n";
        let (fm, _) = parse_skill_md(raw.as_bytes());
        assert!(matches!(
            fm.confidence,
            Confidence::Full | Confidence::Partial
        ));
        assert_eq!(fm.name.as_deref(), Some("review"));
        assert!(fm.description.unwrap().contains("Review code"));
    }

    #[test]
    fn folded_and_literal_blocks() {
        let raw = "---\nname: x\ndescription: >\n  line one\n  line two\n---\n";
        let (fm, _) = parse_skill_md(raw.as_bytes());
        assert_eq!(fm.description.as_deref(), Some("line one line two"));
    }

    #[test]
    fn missing_frontmatter_is_reported_not_fatal() {
        let (fm, issues) = parse_skill_md(b"# just markdown\n");
        assert_eq!(fm.confidence, Confidence::None);
        assert!(issues.iter().any(|i| i.detail == "no_frontmatter"));
        assert!(!fm.is_ok());
    }

    #[test]
    fn unterminated_frontmatter_is_none() {
        let (fm, _) = parse_skill_md(b"---\nname: x\ndescription: y\n");
        assert_eq!(fm.confidence, Confidence::None);
    }

    #[test]
    fn validation_flags_missing_and_long_fields() {
        let fm = Frontmatter {
            confidence: Confidence::Full,
            ..Default::default()
        };
        let codes: Vec<_> = fm
            .validate()
            .iter()
            .map(|i| (i.code, i.field.clone()))
            .collect();
        assert!(codes
            .iter()
            .any(|(c, f)| *c == "E_FRONTMATTER" && f.as_deref() == Some("name")));
        assert!(codes
            .iter()
            .any(|(_, f)| f.as_deref() == Some("metadata.version")));

        let long = "x".repeat(MAX_DESCRIPTION_LEN + 1);
        let fm = Frontmatter {
            name: Some("ok".into()),
            description: Some(long),
            confidence: Confidence::Full,
            ..Default::default()
        };
        assert!(fm
            .validate()
            .iter()
            .any(|i| i.code == "W_FRONTMATTER" && i.detail == "too_long"));
    }

    #[test]
    fn metadata_version_is_case_insensitive() {
        let raw = "---\nname: a\ndescription: b\nmetadata:\n  Version: 42\n---\n";
        let (fm, _) = parse_skill_md(raw.as_bytes());
        assert_eq!(fm.metadata_version(), Some("42"));
    }
}
