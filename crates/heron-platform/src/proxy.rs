//! The proxy Heron's network calls should use.
//!
//! macOS and Windows keep their proxy in a per-user system setting that `git` and `ureq` never
//! read — they only look at environment variables and `http.proxy`. Behind a system proxy (the
//! usual shape on restricted networks) a `git fetch` therefore connects directly, stalls for
//! minutes and dies with `curl 56 Recv failure`. We resolve the effective proxy once here and
//! hand it to both callers.
//!
//! Precedence, highest first:
//!
//! 1. [`set_override`] — the value typed in Settings.
//! 2. Proxy environment variables — `git` reads these itself.
//! 3. `http.proxy` / `https.proxy` in the user's global git config — `git` reads this itself.
//! 4. The platform system proxy (macOS `scutil --proxy`).
//!
//! [`for_git`] only returns the layers git cannot see on its own (1 and 4) so that injecting
//! `-c http.proxy` never overrides a `no_proxy` list the caller's own config relies on.
//!
//! Not supported: PAC scripts (`ProxyAutoConfigEnable`), and the system proxy on Windows/Linux —
//! there we rely on env, git config or the Settings override.

use std::process::Command;
use std::sync::{OnceLock, RwLock};

/// Where the effective proxy came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Typed in Settings.
    Override,
    /// `HTTPS_PROXY` & friends.
    Env,
    /// `http.proxy` in the user's git config.
    GitConfig,
    /// The operating system's proxy setting.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    /// A URL git and ureq both accept: `http://host:port` or `socks5://host:port`.
    pub url: String,
    pub source: Source,
}

static OVERRIDE: RwLock<Option<String>> = RwLock::new(None);
static SYSTEM: OnceLock<Option<Proxy>> = OnceLock::new();

/// Applies the Settings value for the whole process. `None` (or empty) clears it.
pub fn set_override(value: Option<&str>) {
    let normalized = value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    let mut slot = OVERRIDE.write().unwrap_or_else(|e| e.into_inner());
    *slot = normalized;
}

/// The value typed in Settings, if any.
pub fn override_value() -> Option<String> {
    OVERRIDE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The effective proxy for callers that read no proxy configuration of their own (ureq).
pub fn resolve() -> Option<Proxy> {
    if let Some(url) = override_value() {
        return Some(Proxy {
            url,
            source: Source::Override,
        });
    }
    if let Some(url) = env_proxy() {
        return Some(Proxy {
            url,
            source: Source::Env,
        });
    }
    if let Some(url) = from_git_config() {
        return Some(Proxy {
            url,
            source: Source::GitConfig,
        });
    }
    system().cloned()
}

/// The proxy that must be passed to `git` explicitly, because its own configuration cannot see
/// it. `Env` and `GitConfig` are left alone: git honours them (including `no_proxy`) already.
pub fn for_git() -> Option<String> {
    resolve()
        .filter(|p| matches!(p.source, Source::Override | Source::System))
        .map(|p| p.url)
}

fn system() -> Option<&'static Proxy> {
    SYSTEM.get_or_init(detect_system).as_ref()
}

#[cfg(target_os = "macos")]
fn detect_system() -> Option<Proxy> {
    let mut cmd = Command::new("scutil");
    cmd.arg("--proxy");
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_scutil(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(target_os = "macos"))]
fn detect_system() -> Option<Proxy> {
    None
}

/// `HTTPS_PROXY` > `https_proxy` > `ALL_PROXY` > `all_proxy` > `HTTP_PROXY` > `http_proxy`.
fn env_proxy() -> Option<String> {
    pick_env(|key| std::env::var_os(key))
}

fn pick_env(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Some(value) = lookup(key) {
            let value = value.to_string_lossy().trim().to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// The user's own git config, which our fetches still read (`GIT_CONFIG_NOSYSTEM` only drops the
/// system file). Probing it keeps Heron from silently replacing a hand-tuned proxy.
fn from_git_config() -> Option<String> {
    for key in ["http.proxy", "https.proxy"] {
        let mut cmd = Command::new("git");
        cmd.args(["config", "--global", "--get", key]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Turns `scutil --proxy` output into a proxy URL. PAC-only configurations yield `None` on
/// purpose: there is no way to evaluate the script from here.
pub fn parse_scutil(text: &str) -> Option<Proxy> {
    let settings = parse_dictionary(text);
    let scheme_proxy = |kind: &str| -> Option<String> {
        let enabled = settings.get(&format!("{kind}Enable")).map(|v| v == "1")?;
        if !enabled {
            return None;
        }
        let host = settings.get(&format!("{kind}Proxy"))?.trim().to_owned();
        if host.is_empty() || host.eq_ignore_ascii_case("(null)") {
            return None;
        }
        let port = settings
            .get(&format!("{kind}Port"))
            .and_then(|p| p.trim().parse::<u16>().ok())
            .unwrap_or(0);
        Some(endpoint(&host, port, kind == "SOCKS"))
    };
    // HTTPS traffic is what Heron fetches, so the HTTPS entry wins over the plain HTTP one.
    scheme_proxy("HTTPS")
        .or_else(|| scheme_proxy("HTTP"))
        .or_else(|| scheme_proxy("SOCKS"))
        .map(|url| Proxy {
            url,
            source: Source::System,
        })
}

/// `HTTPProxy : 127.0.0.1` pairs. Values keep their literal text.
fn parse_dictionary(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric()) {
            map.insert(key.to_owned(), value.trim().to_owned());
        }
    }
    map
}

/// Git accepts `socks5://` and `socks5h://`; curl (hence `http.proxy`) wants a scheme, and a
/// missing port falls back to the usual listener for each kind.
fn endpoint(host: &str, port: u16, socks: bool) -> String {
    let host = host
        .trim()
        .trim_start_matches("socks5://")
        .trim_start_matches("socks5h://")
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_owned();
    let default_port = if socks { 1080 } else { 3128 };
    let port = if port == 0 { default_port } else { port };
    let scheme = if socks { "socks5h" } else { "http" };
    if host.contains(':') && !host.starts_with('[') {
        // IPv6 literal, already bracketed by whoever wrote it or a bare address.
        format!("{scheme}://[{host}]:{port}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    const MAC: &str = r#"<dictionary> {
  ExcludeSimpleHostnames : 0
  FTPPassive : 1
  HTTPEnable : 1
  HTTPPort : 7897
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7897
  HTTPSProxy : 127.0.0.1
  ProxyAutoConfigEnable : 0
  SOCKSEnable : 0
}"#;

    #[test]
    fn scutil_https_entry_wins() {
        let p = parse_scutil(MAC).unwrap();
        assert_eq!(p.url, "http://127.0.0.1:7897");
        assert_eq!(p.source, Source::System);
    }

    #[test]
    fn scutil_http_only_and_socks_fallback() {
        let http = "HTTPEnable : 1\nHTTPPort : 8118\nHTTPProxy : proxy.internal\n";
        assert_eq!(
            parse_scutil(http).unwrap().url,
            "http://proxy.internal:8118"
        );
        let socks = "SOCKSEnable : 1\nSOCKSPort : 7897\nSOCKSProxy : 127.0.0.1\n";
        assert_eq!(parse_scutil(socks).unwrap().url, "socks5h://127.0.0.1:7897");
    }

    #[test]
    fn scutil_disabled_or_empty_yields_nothing() {
        assert!(parse_scutil("HTTPEnable : 0\nHTTPPort : 7897\nHTTPProxy : 127.0.0.1\n").is_none());
        assert!(parse_scutil("ProxyAutoConfigEnable : 1\n").is_none());
        assert!(parse_scutil("").is_none());
        assert!(parse_scutil("SOCKSEnable : 1\nSOCKSProxy : (null)\n").is_none());
    }

    #[test]
    fn missing_port_falls_back_to_the_scheme_default() {
        assert_eq!(
            parse_scutil("HTTPEnable : 1\nHTTPProxy : proxy.internal\n")
                .unwrap()
                .url,
            "http://proxy.internal:3128"
        );
        assert_eq!(endpoint("::1", 0, true), "socks5h://[::1]:1080");
    }

    fn table(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, OsString> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(*v)))
            .collect()
    }

    #[test]
    fn env_precedence_prefers_https() {
        let m = table(&[
            ("ALL_PROXY", "socks5://x:1080"),
            ("HTTPS_PROXY", "http://y:1"),
        ]);
        assert_eq!(
            pick_env(|key| m.get(key).cloned()),
            Some("http://y:1".to_owned())
        );
        let m = table(&[("https_proxy", " http://y:2 ")]);
        assert_eq!(
            pick_env(|key| m.get(key).cloned()),
            Some("http://y:2".to_owned())
        );
        let m = table(&[("HTTP_PROXY", "")]);
        assert_eq!(pick_env(|key| m.get(key).cloned()), None);
        let m = table(&[]);
        assert_eq!(pick_env(|key| m.get(key).cloned()), None);
    }

    #[test]
    fn override_round_trip_ignores_blank_values() {
        set_override(Some("  http://127.0.0.1:7897 "));
        assert_eq!(override_value().as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(for_git().as_deref(), Some("http://127.0.0.1:7897"));
        set_override(Some("   "));
        assert_eq!(override_value(), None);
        set_override(None);
    }

    /// Reads this machine's real configuration; run it when touching the precedence rules.
    #[test]
    #[ignore = "probes the local environment"]
    fn reports_the_proxy_of_this_machine() {
        let detected = resolve();
        println!("override   = {:?}", override_value());
        println!("env        = {:?}", env_proxy());
        println!("git config = {:?}", from_git_config());
        println!("effective  = {detected:?}");
        assert!(detected.is_some(), "no proxy reachable on this machine");
    }
}
