//! The proxy setting.
//!
//! Detecting the right proxy is `heron_platform::proxy`'s job; this module owns the value the
//! user types in Settings: it is validated, stored in the database and applied to the process,
//! so the GUI and the CLI keep behaving identically.

use heron_core::{ErrorCode, HeronError, Result};
use heron_platform::proxy;

use crate::Ctx;

/// The kv key holding the Settings value.
const KEY: &str = "network.proxy";

/// The Settings value, if one was typed. `None` means Heron auto-detects (environment, git
/// config, then the operating system's own proxy setting).
pub fn get(ctx: &Ctx) -> Result<Option<String>> {
    Ok(ctx.db.kv_get(KEY)?.filter(|value| !value.trim().is_empty()))
}

/// Validates, stores and immediately applies the Settings value; empty clears it.
pub fn set(ctx: &Ctx, value: Option<&str>) -> Result<()> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if let Some(value) = value {
        validate(value)?;
    }
    ctx.db.kv_set(KEY, value.unwrap_or_default())?;
    proxy::set_override(value);
    Ok(())
}

/// Applies the stored value before the first request of a process.
pub fn restore(ctx: &Ctx) -> Result<()> {
    let value = get(ctx)?;
    proxy::set_override(value.as_deref());
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
        assert_eq!(get(&ctx).unwrap(), None);

        set(&ctx, Some("  http://127.0.0.1:7897 ")).unwrap();
        assert_eq!(get(&ctx).unwrap().as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:7897")
        );

        set(&ctx, Some("")).unwrap();
        assert_eq!(get(&ctx).unwrap(), None);
        assert_eq!(proxy::override_value(), None);

        // A rejected value leaves both the database and the process untouched.
        set(&ctx, Some("http://127.0.0.1:7897")).unwrap();
        set(&ctx, Some("ftp://nope")).unwrap_err();
        assert_eq!(get(&ctx).unwrap().as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:7897")
        );

        proxy::set_override(None);
    }

    #[test]
    fn restore_applies_the_persisted_value() {
        let _serial = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let (_tmp, ctx) = ctx();
        ctx.db.kv_set(KEY, "http://127.0.0.1:8118").unwrap();
        restore(&ctx).unwrap();
        assert_eq!(
            proxy::override_value().as_deref(),
            Some("http://127.0.0.1:8118")
        );
        proxy::set_override(None);
    }
}
