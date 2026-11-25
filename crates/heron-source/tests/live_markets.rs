//! Live marketplace checks.
//!
//! These hit the real sites, so they are marked `#[ignore]` and run explicitly with
//! `cargo test -p heron-source --test live_markets -- --ignored --nocapture`. They print what
//! they see, so a changed site is obvious instead of silently breaking search.

use heron_source::market::{adapter, Capability, InstallPlan};

#[test]
#[ignore = "network: run explicitly to re-verify market adapters"]
fn skills_sh_live_search() {
    let market = adapter("skills_sh").unwrap();
    assert_eq!(market.capability(), Capability::Indexed);
    match market.search("wechat-mp", 5) {
        Ok(items) => {
            eprintln!("skills.sh returned {} items", items.len());
            for item in items.iter().take(3) {
                eprintln!(
                    "  {} -> plan {:?}",
                    item.key,
                    market.install_plan(&item.key)
                );
            }
            if let Some(first) = items.first() {
                match market.install_plan(&first.key).unwrap() {
                    InstallPlan::Git { url, subpath, .. } => {
                        assert!(url.starts_with("https://github.com/"));
                        assert!(!subpath.is_empty());
                    }
                    other => panic!("skills.sh items must be installable, got {other:?}"),
                }
            }
        }
        Err(e) => eprintln!(
            "skills.sh unavailable ({}) - adapter degrades gracefully",
            e.as_msg().code
        ),
    }
}

#[test]
#[ignore = "network: run explicitly to re-verify market adapters"]
fn clawhub_live_search() {
    let market = adapter("clawhub").unwrap();
    match market.search("wechat-mp", 5) {
        Ok(items) => {
            eprintln!("clawhub returned {} items", items.len());
            for item in items.iter().take(3) {
                eprintln!(
                    "  {} | {} | {:?}",
                    item.key,
                    item.name,
                    market.install_plan(&item.key)
                );
            }
            assert!(!items.is_empty(), "clawhub search should find wechat-mp skills");
        }
        Err(e) => eprintln!(
            "clawhub unavailable ({}) - adapter degrades gracefully",
            e.as_msg().code
        ),
    }
}

#[test]
#[ignore = "network: run explicitly to re-verify market adapters"]
fn guided_markets_never_need_the_network() {
    for id in ["qoder", "skillhub_cn", "skillsmp"] {
        let market = adapter(id).unwrap();
        assert_eq!(market.capability(), Capability::Guided);
        let items = market.search("原型", 1).unwrap();
        assert_eq!(items.len(), 1, "{id} returns a deep link");
        assert!(items[0].url.starts_with("https://"));
    }
}
