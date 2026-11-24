//! The proxy setting.
//!
//! Detecting the right proxy is `heron_platform::proxy`'s job; this module owns the value the
//! user types in Settings: it is validated, stored in the database and applied to the process,
//! so the GUI and the CLI keep behaving identically.

use heron_core::{ErrorCode, HeronError, Result};
use heron_platform::proxy;
use serde::{Deserialize, Serialize};

use crate::Ctx;

/// The kv key holding the Settings value.
const KEY_URL: &str = "network.proxy";
const KEY_ENABLED: &str = "network.proxy.enabled";

/// The proxy configuration state exposed to the UI and callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// The custom proxy URL configured by the user, if any.
    pub url: Option<String>,
    /// Whether network proxy usage is enabled.
    pub enabled: bool,
    /// The actual effective proxy URL currently in use (None if disabled or direct).
    pub current: Option<String>,
}

/// The Settings value, if one was typed, along with whether proxy is active.
pub fn get(ctx: &Ctx) -> Result<ProxyConfig> {
    let url = ctx
        .db
        .kv_get(KEY_URL)?
        .filter(|value| !value.trim().is_empty());
    let enabled_str = ctx.db.kv_get(KEY_ENABLED)?;
    let enabled = enabled_str.as_deref() != Some("false");
    let current = if enabled {
        proxy::resolve().map(|p| p.url)
    } else {
        None
    };
    Ok(ProxyConfig {
        url,
        enabled,
        current,
    })
}

/// Validates, stores and immediately applies the Settings value and enabled state.
pub fn set(ctx: &Ctx, url: Option<&str>, enabled: bool) -> Result<ProxyConfig> {
    let url = url.map(str::trim).filter(|value| !value.is_empty());
    if let Some(url) = url {
        validate(url)?;
    }
    ctx.db.kv_set(KEY_URL, url.unwrap_or_default())?;
    ctx.db
        .kv_set(KEY_ENABLED, if enabled { "true" } else { "false" })?;
    if enabled {
        proxy::set_disabled(false);
        proxy::set_override(url);
    } else {
        proxy::set_disabled(true);
        proxy::set_override(None);
    }
    let current = if enabled {
        proxy::resolve().map(|p| p.url)
    } else {
        None
    };
    Ok(ProxyConfig {
        url: url.map(str::to_owned),
        enabled,
        current,
    })
}

/// Applies the stored value before the first request of a process.
pub fn restore(ctx: &Ctx) -> Result<()> {
    let cfg = get(ctx)?;
    if cfg.enabled {
        proxy::set_disabled(false);
        proxy::set_override(cfg.url.as_deref());
    } else {
        proxy::set_disabled(true);
        proxy::set_override(None);
    }
    Ok(())
}

/// A typo here would otherwise surface much later as an unexplained timeout.
fn validate(value: &str) -> Result<()> {
    let well_formed = match value.split_once("://") {
        Some((scheme, authority)) => {
            matches!(scheme, "http" | "https" | "socks4" | "socks5" | "socks5h")
                && !authority.trim_matches('/').is_empty()
        }
        // `host:port`, as pasted from a proxy client: no scheme, no path, explicit port.
        None => {
            value.contains(':')
                && !value.contains('/')
                && value
                    .rsplit(':')
                    .next()
                    .is_some_and(|port| port.parse::<u16>().is_ok())
        }
    };
    if well_formed && !value.chars().any(char::is_whitespace) {
        return Ok(());
    }
    Err(HeronError::from(
        ErrorCode::Network.msg().with("url", value.to_owned()).with(
            "detail",
            "expected http://host:port or socks5://host:port".to_owned(),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_proxy_forms_are_accepted() {
        for value in [
            "http://127.0.0.1:7897",
            "https://proxy.internal:3128",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:7897",
            "127.0.0.1:7897",
            "proxy.internal:3128",
        ] {
            validate(value).unwrap_or_else(|e| panic!("{value}: {e:?}"));
        }
    }

    #[test]
    fn typos_are_rejected_before_they_become_a_mystery_timeout() {
        for value in [
            "",
            "   ",
            "http://",
            "ftp://proxy:21",
            "127.0.0.1",
            "http://127.0.0.1:7897 /feed",
        ] {
            let err = validate(value).unwrap_err();
            assert_eq!(err.code(), "E_NETWORK", "{value} must be rejected");
        }
    }

    fn ctx() -> (tempfile::TempDir, Ctx) {
        use heron_platform::dirs::Dirs;
        use heron_store::db::Db;
        let tmp = tempfile::tempdir().unwrap();
        let dirs =
            Dirs::resolve_with_override("heron-test", Some(&tmp.path().join("data"))).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        (tmp, Ctx::new(dirs, db, "test"))
    }

    /// The override is process-wide state, so the tests that read it take turns.
    static GLOBAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_setting_is_stored_applied_and_cleared() {
        let _serial = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let (_tmp, ctx) = ctx();
        let initial = get(&ctx).unwrap();
        assert_eq!(initial.url, None);
        assert!(initial.enabled);

        let cfg = set(&ctx, Some("  http://127.0.0.1:7897 "), true).unwrap();
        assert_eq!(cfg.url.as_deref(), Some("http://127.0.0.1:7897"));
        assert!(cfg.enabled);
        assert_eq!(cfg.current.as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert!(!proxy::is_disabled());

        // Disabling preserves the URL but turns off the process override
        let disabled_cfg = set(&ctx, Some("http://127.0.0.1:7897"), false).unwrap();
        assert_eq!(disabled_cfg.url.as_deref(), Some("http://127.0.0.1:7897"));
        assert!(!disabled_cfg.enabled);
        assert_eq!(disabled_cfg.current, None);
        assert_eq!(proxy::override_value(), None);
        assert!(proxy::is_disabled());

        // Clearing the URL while enabled falls back to auto-detect
        let cleared = set(&ctx, Some(""), true).unwrap();
        assert_eq!(cleared.url, None);
        assert!(cleared.enabled);
        assert_eq!(proxy::override_value(), None);
        assert!(!proxy::is_disabled());

        // A rejected value leaves both the database and the process untouched.
        set(&ctx, Some("http://127.0.0.1:7897"), true).unwrap();
        set(&ctx, Some("ftp://nope"), true).unwrap_err();
        assert_eq!(
            get(&ctx).unwrap().url.as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:7897")
        );

        proxy::set_override(None);
        proxy::set_disabled(false);
    }

    #[test]
    fn restore_applies_the_persisted_value() {
        let _serial = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let (_tmp, ctx) = ctx();
        ctx.db.kv_set(KEY_URL, "http://127.0.0.1:8118").unwrap();
        ctx.db.kv_set(KEY_ENABLED, "true").unwrap();
        restore(&ctx).unwrap();
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:8118")
        );
        assert!(!proxy::is_disabled());

        ctx.db.kv_set(KEY_ENABLED, "false").unwrap();
        restore(&ctx).unwrap();
        assert_eq!(proxy::override_value(), None);
        assert!(proxy::is_disabled());

        proxy::set_override(None);
        proxy::set_disabled(false);
    }
}
