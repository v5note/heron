//! A real fetch against GitHub, kept out of the default test run.
//!
//! The bug this guards against: a repository whose branches sort before its actual default branch
//! (`auto-optimize/…` < `master`) used to be resolved as the default, so an install landed the
//! wrong content. Only a live remote exercises the `HEAD` refspec.
//!
//! Run with: `cargo test -p heron-git --test live_fetch -- --ignored --nocapture`

use std::time::Duration;

use heron_git::{CachedRepo, RefKind, RemoteUrl};

/// The user's own proxy resolution is inherited; this only needs to finish before the timeout.
const BUDGET: Duration = Duration::from_secs(120);

#[test]
#[ignore = "network"]
fn default_branch_of_a_real_repo_is_its_head_not_the_first_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = RemoteUrl::parse("https://github.com/alchaincyf/huashu-design").unwrap();
    let mut repo = CachedRepo::new(tmp.path(), remote).with_timeout(BUDGET);
    repo.ensure().unwrap();

    let head = repo.resolve(None).unwrap();
    assert_eq!(head.kind, RefKind::Branch, "default ref is a branch");
    assert!(!head.commit.is_empty());

    let heads = repo.heads().unwrap();
    let names: Vec<&str> = heads.iter().map(|h| h.name.as_str()).collect();
    let same_branch = heads
        .iter()
        .find(|h| h.commit == head.commit && h.name != "HEAD")
        .unwrap_or_else(|| panic!("HEAD must match one real branch, heads: {names:?}"));
    assert!(
        !same_branch.name.contains("/"),
        "the default branch is a top-level branch, got {:?}",
        same_branch.name
    );
}
