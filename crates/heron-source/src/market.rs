//! Marketplace adapters (design §9.5).
//!
//! Two capability levels, because the sites genuinely differ:
//!
//! * **Indexed** — we can search it and turn a result into an install plan
//!   (skills.sh resolves to `github.com/<owner>/<repo>` + `skills/<slug>`; ClawHub has a REST API).
//! * **Guided** — we can only open the site for the user and accept a pasted URL back
//!   (qoder, skillhub.cn, skillsmp).
//!
//! Nothing here executes remote content: adapters only produce plans.

use std::io::Read;
use std::time::Duration;

use heron_core::{ErrorCode, Result};
use serde::{Deserialize, Serialize};

/// Cache time-to-live for market responses.
pub const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Searchable and installable.
    Indexed,
    /// Browse in a browser; Heron accepts the URL back.
    Guided,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarketItem {
    pub market: String,
    /// Stable key inside the market (`owner/slug` or `owner/repo/skill`).
    pub key: String,
    pub name: String,
    pub owner: Option<String>,
    pub summary: String,
    pub version: Option<String>,
    pub downloads: Option<u64>,
    /// Direct link to the skill page.
    pub url: String,
}

/// What installing an item means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallPlan {
    /// Fetch from git: `heron install <url> --subpath <path>`.
    Git {
        url: String,
        ref_name: Option<String>,
        subpath: String,
    },
    /// Not installable by Heron; open the page instead.
    Guided { open_url: String, reason: String },
}

pub trait MarketAdapter {
    fn id(&self) -> &'static str;
    fn capability(&self) -> Capability;
    /// Search returns up to `limit` items.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<MarketItem>>;
    fn install_plan(&self, key: &str) -> Result<InstallPlan>;
}

/// Which markets exist and how they are reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub capability: Capability,
    pub base_url: &'static str,
    pub enabled_by_default: bool,
}

pub const MARKETS: &[MarketInfo] = &[
    MarketInfo {
        id: "skills_sh",
        name: "skills.sh",
        capability: Capability::Indexed,
        base_url: "https://www.skills.sh",
        enabled_by_default: true,
    },
    MarketInfo {
        id: "clawhub",
        name: "ClawHub",
        capability: Capability::Indexed,
        base_url: "https://clawhub.ai",
        enabled_by_default: true,
    },
    MarketInfo {
        id: "qoder",
        name: "Qoder",
        capability: Capability::Guided,
        base_url: "https://qoder.com/zh/marketplace",
        enabled_by_default: true,
    },
    MarketInfo {
        id: "skillhub_cn",
        name: "SkillHub",
        capability: Capability::Guided,
        base_url: "https://skillhub.cn",
        enabled_by_default: true,
    },
    MarketInfo {
        id: "modelscope",
        name: "ModelScope",
        // Browsable through a search deep-link, and every skill is a cloneable git repository,
        // so a pasted skill URL turns straight into an install plan.
        capability: Capability::Guided,
        base_url: "https://www.modelscope.cn",
        enabled_by_default: true,
    },
    MarketInfo {
        id: "skillsmp",
        name: "SkillsMP",
        capability: Capability::Guided,
        base_url: "https://skillsmp.com",
        enabled_by_default: false,
    },
];

pub fn info(id: &str) -> Option<&'static MarketInfo> {
    MARKETS.iter().find(|m| m.id == id)
}

// ---------------------------------------------------------------------------
// skills.sh: a static directory whose entries map to GitHub repositories.
// ---------------------------------------------------------------------------

pub struct SkillsSh {
    base: String,
}

impl Default for SkillsSh {
    fn default() -> Self {
        Self {
            base: "https://www.skills.sh".to_owned(),
        }
    }
}

impl SkillsSh {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    /// `/<owner>/<repo>/<skill>` -> (owner, repo, skill)
    pub fn parse_skill_url(&self, url: &str) -> Option<(String, String, String)> {
        let path = url.trim().trim_end_matches('/');
        let path = path.strip_prefix(&self.base).unwrap_or(path);
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.len() != 3 {
            return None;
        }
        Some((
            segments[0].to_owned(),
            segments[1].to_owned(),
            segments[2].to_owned(),
        ))
    }
}

impl MarketAdapter for SkillsSh {
    fn id(&self) -> &'static str {
        "skills_sh"
    }

    fn capability(&self) -> Capability {
        Capability::Indexed
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<MarketItem>> {
        let body = http_get(&format!("{}/sitemap-skills-1.xml", self.base))?;
        Ok(search_sitemap(&self.base, &body, query, limit))
    }

    fn install_plan(&self, key: &str) -> Result<InstallPlan> {
        // key = "owner/repo/skill"
        let segments: Vec<&str> = key.split('/').collect();
        if segments.len() != 3 {
            return Err(ErrorCode::SourceAmbiguous
                .msg()
                .with("key", key.to_owned())
                .with("reason", "expected_owner_repo_skill")
                .into());
        }
        Ok(InstallPlan::Git {
            url: format!("https://github.com/{}/{}.git", segments[0], segments[1]),
            ref_name: None,
            subpath: format!("skills/{}", segments[2]),
        })
    }
}

/// Pure sitemap search so it can be tested without the network.
pub fn search_sitemap(base: &str, xml: &str, query: &str, limit: usize) -> Vec<MarketItem> {
    let needle = query.trim().to_lowercase();
    let adapter = SkillsSh::new(base);
    let mut items = Vec::new();
    for loc in xml.split("<loc>").skip(1) {
        let Some(url) = loc.split("</loc>").next() else {
            continue;
        };
        let Some((owner, repo, skill)) = adapter.parse_skill_url(url) else {
            continue;
        };
        let haystack = format!("{owner}/{repo}/{skill}").to_lowercase();
        if !needle.is_empty() && !haystack.contains(&needle) {
            continue;
        }
        items.push(MarketItem {
            market: "skills_sh".to_owned(),
            key: format!("{owner}/{repo}/{skill}"),
            name: skill.clone(),
            owner: Some(owner),
            summary: format!("{repo} · {skill}"),
            version: None,
            downloads: None,
            url: url.to_owned(),
        });
        if items.len() >= limit {
            break;
        }
    }
    items
}

// ---------------------------------------------------------------------------
// ClawHub: a REST API publishing platform-hosted skills.
// ---------------------------------------------------------------------------

pub struct ClawHub {
    base: String,
}

impl Default for ClawHub {
    fn default() -> Self {
        Self {
            base: "https://clawhub.ai".to_owned(),
        }
    }
}

impl ClawHub {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }
}

#[derive(Debug, Deserialize)]
struct ClawSearchResponse {
    #[serde(default)]
    results: Vec<ClawSearchItem>,
}

#[derive(Debug, Deserialize)]
struct ClawSearchItem {
    #[serde(default)]
    slug: String,
    #[serde(default, rename = "displayName")]
    display_name: String,
    #[serde(default)]
    summary: String,
    #[serde(default, rename = "ownerHandle")]
    owner_handle: Option<String>,
    #[serde(default, rename = "canonicalUrl")]
    canonical_url: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
}

impl MarketAdapter for ClawHub {
    fn id(&self) -> &'static str {
        "clawhub"
    }

    fn capability(&self) -> Capability {
        Capability::Indexed
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<MarketItem>> {
        let body = http_get(&format!(
            "{}/api/v1/search?q={}&limit={}",
            self.base,
            urlencode(query),
            limit
        ))?;
        let parsed: ClawSearchResponse = serde_json::from_str(&body).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::CatalogParse
                    .msg()
                    .with("file", "clawhub.search")
                    .with("detail", e.to_string()),
            )
        })?;
        Ok(parsed
            .results
            .into_iter()
            .map(|r| {
                let owner = r.owner_handle.clone().unwrap_or_default();
                let key = if owner.is_empty() {
                    r.slug.clone()
                } else {
                    format!("{owner}/{}", r.slug)
                };
                MarketItem {
                    market: "clawhub".to_owned(),
                    url: format!(
                        "{}{}",
                        self.base,
                        r.canonical_url
                            .unwrap_or_else(|| format!("/{key}/skills/{}", r.slug))
                    ),
                    key,
                    name: if r.display_name.is_empty() {
                        r.slug
                    } else {
                        r.display_name
                    },
                    owner: r.owner_handle,
                    summary: r.summary,
                    version: r.version,
                    downloads: r.downloads,
                }
            })
            .collect())
    }

    fn install_plan(&self, key: &str) -> Result<InstallPlan> {
        // Ask the detail endpoint whether an upstream Git source exists.
        let body = http_get(&format!("{}/api/v1/packages/{key}", self.base)).ok();
        if let Some(source) = body.as_deref().and_then(extract_source_url) {
            return Ok(InstallPlan::Git {
                url: source,
                ref_name: None,
                subpath: String::new(),
            });
        }
        Ok(InstallPlan::Guided {
            open_url: format!("{}/{}", self.base, key),
            reason: "platform_hosted".to_owned(),
        })
    }
}

/// Pull a git URL out of a JSON document if one is present.
pub fn extract_source_url(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut stack = vec![&value];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::Object(map) => {
                for key in ["sourceUrl", "repository", "homepage"] {
                    if let Some(serde_json::Value::String(s)) = map.get(key) {
                        if s.starts_with("https://")
                            && (s.contains("github.") || s.ends_with(".git"))
                        {
                            return Some(s.clone());
                        }
                    }
                }
                stack.extend(map.values());
            }
            serde_json::Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    None
}

/// A guided market: browse in a browser, paste a URL back.
pub struct Guided {
    pub(crate) id: &'static str,
    pub(crate) base: String,
    pub(crate) search_path: &'static str,
}

impl Guided {
    pub fn new(id: &'static str, base: impl Into<String>, search_path: &'static str) -> Self {
        Self {
            id,
            base: base.into(),
            search_path,
        }
    }
}

impl MarketAdapter for Guided {
    fn id(&self) -> &'static str {
        self.id
    }

    fn capability(&self) -> Capability {
        Capability::Guided
    }

    fn search(&self, query: &str, _limit: usize) -> Result<Vec<MarketItem>> {
        let url = format!("{}{}{}", self.base, self.search_path, urlencode(query));
        Ok(vec![MarketItem {
            market: self.id.to_owned(),
            key: query.to_owned(),
            name: query.to_owned(),
            owner: None,
            summary: String::new(),
            version: None,
            downloads: None,
            url,
        }])
    }

    fn install_plan(&self, key: &str) -> Result<InstallPlan> {
        Ok(InstallPlan::Guided {
            open_url: format!("{}{}{}", self.base, self.search_path, urlencode(key)),
            reason: "manual_browse".to_owned(),
        })
    }
}

/// ModelScope skills are hosted as git repositories at `modelscope.cn/skills/<owner>/<repo>`.
/// There is no stable public search API, so browsing is a `?q=` deep-link, but a skill page URL
/// resolves straight to a clone — which makes ModelScope installable from a paste or a favourite.
pub struct ModelScope {
    base: String,
}

impl Default for ModelScope {
    fn default() -> Self {
        Self {
            base: "https://www.modelscope.cn".to_owned(),
        }
    }
}

impl ModelScope {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    /// `<owner>/<repo>` (optionally followed by `/skills/...`) or a full page URL.
    fn owner_repo(&self, key: &str) -> Option<(String, String)> {
        let raw = key.trim();
        let path = raw
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .strip_prefix("www.modelscope.cn")
            .or_else(|| raw.strip_prefix("modelscope.cn"))
            .unwrap_or(raw);
        let segments: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty() && *s != "skills" && !s.ends_with(".git"))
            .collect();
        if segments.len() < 2 {
            return None;
        }
        Some((segments[0].to_owned(), segments[1].to_owned()))
    }
}

impl MarketAdapter for ModelScope {
    fn id(&self) -> &'static str {
        "modelscope"
    }

    fn capability(&self) -> Capability {
        Capability::Guided
    }

    fn search(&self, query: &str, _limit: usize) -> Result<Vec<MarketItem>> {
        let url = format!("{}/skills?q={}", self.base, urlencode(query));
        Ok(vec![MarketItem {
            market: "modelscope".to_owned(),
            key: query.trim().to_owned(),
            name: query.trim().to_owned(),
            owner: None,
            summary: String::new(),
            version: None,
            downloads: None,
            url,
        }])
    }

    fn install_plan(&self, key: &str) -> Result<InstallPlan> {
        match self.owner_repo(key) {
            Some((owner, repo)) => Ok(InstallPlan::Git {
                url: format!("https://www.modelscope.cn/{owner}/{repo}.git"),
                ref_name: None,
                subpath: String::new(),
            }),
            None => Ok(InstallPlan::Guided {
                open_url: format!("{}/skills?q={}", self.base, urlencode(key)),
                reason: "modelscope_browse".to_owned(),
            }),
        }
    }
}

/// Resolve an adapter by id.
pub fn adapter(id: &str) -> Result<Box<dyn MarketAdapter>> {
    match id {
        "skills_sh" => Ok(Box::new(SkillsSh::default())),
        "clawhub" => Ok(Box::new(ClawHub::default())),
        "modelscope" => Ok(Box::new(ModelScope::default())),
        "qoder" => Ok(Box::new(Guided::new(
            "qoder",
            "https://qoder.com/zh/marketplace",
            "?q=",
        ))),
        "skillhub_cn" => Ok(Box::new(Guided::new(
            "skillhub_cn",
            "https://skillhub.cn",
            "/skills?q=",
        ))),
        "skillsmp" => Ok(Box::new(Guided::new(
            "skillsmp",
            "https://skillsmp.com",
            "/?q=",
        ))),
        other => Err(ErrorCode::MarketUnavailable
            .msg()
            .with("market", other.to_owned())
            .into()),
    }
}

fn urlencode(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_owned(),
            other => other
                .to_string()
                .bytes()
                .map(|b| format!("%{b:02X}"))
                .collect(),
        })
        .collect()
}

fn http_get(url: &str) -> Result<String> {
    let mut last: Option<String> = None;
    for attempt in 0..3 {
        match http_get_once(url) {
            Ok(body) => return Ok(body),
            Err(e) => {
                last = Some(e);
                if attempt < 2 {
                    std::thread::sleep(Duration::from_millis(250 * (attempt + 1) as u64));
                }
            }
        }
    }
    Err(heron_core::HeronError::from(
        ErrorCode::MarketUnavailable
            .msg()
            .with("url", url.to_owned())
            .with("detail", last.unwrap_or_default()),
    ))
}

/// ureq reads no proxy configuration of its own (our dependency does not enable
/// `proxy-from-env`), so the proxy Heron resolved has to be handed to it here — otherwise market
/// search and the marketplace manifests keep connecting directly and time out behind a system
/// proxy, exactly like `git fetch` did before.
fn http_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .redirects(5)
        .user_agent(concat!("heron/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = heron_platform::proxy::resolve() {
        // A spec ureq cannot speak (e.g. a bare SOCKS proxy it was built without) falls back to
        // the direct route rather than failing every request.
        if let Ok(parsed) = ureq::Proxy::new(&proxy.url) {
            builder = builder.proxy(parsed);
        }
    }
    builder.build()
}

fn http_get_once(url: &str) -> std::result::Result<String, String> {
    let handle = http_agent()
        .get(url)
        .set(
            "Accept",
            "application/json, application/xml, text/html;q=0.9, */*;q=0.8",
        )
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = String::new();
    handle
        .into_reader()
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITEMAP: &str = r#"<?xml version="1.0"?>
<urlset>
  <url><loc>https://www.skills.sh/vercel-labs/skills/find-skills</loc></url>
  <url><loc>https://www.skills.sh/anthropics/skills/wechat-mp</loc></url>
  <url><loc>https://www.skills.sh/owner</loc></url>
</urlset>"#;

    #[test]
    fn skills_sh_urls_are_parsed_into_three_segments() {
        let a = SkillsSh::default();
        let (o, r, s) = a
            .parse_skill_url("https://www.skills.sh/vercel-labs/skills/find-skills")
            .unwrap();
        assert_eq!(
            (o.as_str(), r.as_str(), s.as_str()),
            ("vercel-labs", "skills", "find-skills")
        );
        assert!(a.parse_skill_url("https://www.skills.sh/owner").is_none());
    }

    #[test]
    fn sitemap_search_filters_and_marks_installable() {
        let items = search_sitemap("https://www.skills.sh", SITEMAP, "wechat-mp", 10);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "anthropics/skills/wechat-mp");
        assert_eq!(items[0].name, "wechat-mp");

        let all = search_sitemap("https://www.skills.sh", SITEMAP, "", 10);
        assert_eq!(all.len(), 2, "owner pages are not skills");
    }

    #[test]
    fn skills_sh_install_plan_points_at_github() {
        let plan = SkillsSh::default()
            .install_plan("anthropics/skills/wechat-mp")
            .unwrap();
        assert_eq!(
            plan,
            InstallPlan::Git {
                url: "https://github.com/anthropics/skills.git".into(),
                ref_name: None,
                subpath: "skills/wechat-mp".into()
            }
        );
        assert!(SkillsSh::default().install_plan("bad-key").is_err());
    }

    #[test]
    fn clawhub_plan_is_guided_when_platform_hosted() {
        // The real API returns no sourceUrl for platform-hosted skills; simulate both cases.
        assert!(extract_source_url(r#"{"package":{"summary":"x"}}"#).is_none());
        assert_eq!(
            extract_source_url(r#"{"package":{"sourceUrl":"https://github.com/o/r"}}"#).as_deref(),
            Some("https://github.com/o/r")
        );
        assert!(
            extract_source_url(r#"{"package":{"repository":"https://gitlab.com/o/r"}}"#).is_none()
        );
    }

    #[test]
    fn guided_markets_only_open_a_browser() {
        let market = adapter("qoder").unwrap();
        assert_eq!(market.capability(), Capability::Guided);
        match market.install_plan("原型 设计").unwrap() {
            InstallPlan::Guided { open_url, .. } => {
                assert!(open_url.starts_with("https://qoder.com/zh/marketplace?q="));
                assert!(open_url.contains("%20"), "query is url-encoded: {open_url}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn modelscope_page_url_resolves_to_a_git_clone() {
        let m = ModelScope::default();
        assert_eq!(
            m.install_plan("https://www.modelscope.cn/skills/alibaba/dashscope")
                .unwrap(),
            InstallPlan::Git {
                url: "https://www.modelscope.cn/alibaba/dashscope.git".into(),
                ref_name: None,
                subpath: String::new()
            }
        );
        assert_eq!(
            m.install_plan("iic/my-skill").unwrap(),
            InstallPlan::Git {
                url: "https://www.modelscope.cn/iic/my-skill.git".into(),
                ref_name: None,
                subpath: String::new()
            }
        );
        // A free-text query is not a repo: fall back to the browse deep-link.
        match m.install_plan("原型 设计").unwrap() {
            InstallPlan::Guided { open_url, .. } => {
                assert!(open_url.starts_with("https://www.modelscope.cn/skills?q="));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn market_table_matches_design_decisions() {
        assert_eq!(MARKETS.len(), 6);
        let indexed: Vec<_> = MARKETS
            .iter()
            .filter(|m| m.capability == Capability::Indexed)
            .map(|m| m.id)
            .collect();
        assert_eq!(indexed, vec!["skills_sh", "clawhub"]);
        let disabled: Vec<_> = MARKETS
            .iter()
            .filter(|m| !m.enabled_by_default)
            .map(|m| m.id)
            .collect();
        assert_eq!(disabled, vec!["skillsmp"], "decision D1");
        for m in MARKETS {
            assert!(adapter(m.id).is_ok(), "{} must have an adapter", m.id);
        }
    }

    #[test]
    fn url_encoding_is_minimal_and_correct() {
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("wechat-mp"), "wechat-mp");
        assert_eq!(urlencode("原型"), "%E5%8E%9F%E5%9E%8B");
    }

    #[test]
    fn unknown_market_is_rejected() {
        let err = match adapter("nope") {
            Ok(_) => panic!("unknown market must be rejected"),
            Err(e) => e,
        };
        assert_eq!(err.code(), "E_MARKET_UNAVAILABLE");
    }
}
