//! End-to-end repository tests against a real `git` binary.

use std::fs;
use std::path::Path;

use heron_vcs::merge::MergeOutcome;
use heron_vcs::{SkillMeta, SkillRepo};

fn repo(dir: &Path) -> SkillRepo {
    fs::create_dir_all(dir).unwrap();
    // Enough unchanged context between editable lines so independent edits merge cleanly.
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: pdf\ndescription: d\n---\nl1\nc2\nc3\nl2\nc5\nc6\nc7\nl3\n",
    )
    .unwrap();
    let repo = SkillRepo::open(dir);
    repo.init(&SkillMeta::new("pdf", "local", "", "0.0.0"))
        .unwrap();
    repo
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn init_creates_main_and_upstream_and_commits_metadata() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());

    assert!(repo.is_repo());
    assert!(tmp.path().join("heron.skill.json").is_file());
    let meta = repo.read_metadata().unwrap().unwrap();
    assert_eq!(meta.slug, "pdf");
    assert_eq!(meta.schema, 1);
    assert!(!meta.created_at.is_empty());

    let tags = repo.list_tags().unwrap();
    assert!(tags.is_empty(), "adoption creates no release tag");

    // upstream points at the same commit as main.
    assert_eq!(repo.rev_parse("upstream").unwrap(), repo.head().unwrap());
}

#[test]
fn commits_are_recorded_with_heron_identity() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: pdf\ndescription: d\n---\nchanged\n",
    )
    .unwrap();
    let sha = repo.commit_all("heron: local edit").unwrap();
    assert_eq!(sha, repo.head().unwrap());

    let log = repo.log(5).unwrap();
    assert!(log.iter().any(|c| c.summary == "heron: local edit"));
    assert_eq!(
        log[0].author, "Heron",
        "commits never depend on the machine's git config"
    );
}

#[test]
fn empty_commit_is_a_noop() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let before = repo.head().unwrap();
    let after = repo.commit_all("heron: nothing changed").unwrap();
    assert_eq!(before, after);
}

#[test]
fn clean_three_way_merge_combines_both_sides() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let base = repo.head().unwrap();

    // 1) upstream rewrites the last line (lands on main first, so it becomes the ancestor
    //    from which the local edit diverges in this linear simulation).
    fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: pdf\ndescription: d\n---\nl1\nc2\nc3\nTHEIRS\nc5\nc6\nc7\nl3\n",
    )
    .unwrap();
    let theirs = repo.commit_all("upstream: theirs").unwrap();
    repo.set_upstream_point(&theirs).unwrap();

    // 2) local edit touches a different line, far enough away to be a separate hunk.
    fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: pdf\ndescription: d\n---\nl1\nc2\nc3\nl2\nc5\nc6\nc7\nLOCAL\n",
    )
    .unwrap();
    repo.commit_all("heron: local edit").unwrap();

    // 3) merging theirs on top of the local edit must keep both changes.
    match repo.merge_from_upstream(&theirs, &base).unwrap() {
        MergeOutcome::Clean { commit, .. } => {
            repo.fast_forward_to(&commit).unwrap();
            let merged = fs::read_to_string(tmp.path().join("SKILL.md")).unwrap();
            assert!(merged.contains("LOCAL"), "local change kept: {merged}");
            assert!(
                merged.contains("THEIRS"),
                "upstream change applied: {merged}"
            );
        }
        MergeOutcome::Conflicts { files } => panic!("unexpected conflicts: {files:?}"),
    }
}

#[test]
fn conflicting_merge_reports_files_and_leaves_worktree_untouched() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let base = repo.head().unwrap();

    // Theirs and ours both rewrite the same line.
    let theirs = {
        fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: pdf\ndescription: d\n---\nTHEIRS\n",
        )
        .unwrap();
        repo.commit_all("upstream: rewrite").unwrap()
    };
    let ours = {
        fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: pdf\ndescription: d\n---\nLOCAL\n",
        )
        .unwrap();
        repo.commit_all("heron: local rewrite").unwrap()
    };
    repo.set_upstream_point(&theirs).unwrap();
    repo.fast_forward_to(&ours).unwrap();

    match repo.merge_from_upstream(&theirs, &base).unwrap() {
        MergeOutcome::Conflicts { files } => {
            assert!(files.iter().any(|f| f.path == "SKILL.md"), "got {files:?}");
        }
        MergeOutcome::Clean { .. } => panic!("same-line rewrite must conflict"),
    }
    // Nothing was applied.
    assert!(fs::read_to_string(tmp.path().join("SKILL.md"))
        .unwrap()
        .contains("LOCAL"));
}

#[test]
fn reset_to_a_tag_is_reversible() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let v1 = repo.head().unwrap();
    repo.tag_version("1.0.0", &v1).unwrap();
    assert_eq!(repo.rev_parse("v1.0.0").unwrap(), v1);

    fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: pdf\ndescription: d\n---\nv2 body\n",
    )
    .unwrap();
    let v2 = repo.commit_all("heron: v2").unwrap();
    repo.tag_version("2.0.0", &v2).unwrap();

    let previous = repo.reset_to("v1.0.0").unwrap();
    assert_eq!(previous, v2, "reset reports where it came from");
    assert!(repo.read_metadata().unwrap().is_some());

    let tags = repo.list_tags().unwrap();
    assert!(tags.contains(&"v1.0.0".to_owned()));
    assert!(
        tags.iter().any(|t| t.starts_with("heron-rollback-backup-")),
        "the move itself is tag-protected: {tags:?}"
    );
}

#[test]
fn local_changes_are_detected_against_the_baseline() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    assert!(!repo.has_local_changes().unwrap());

    fs::write(tmp.path().join("notes.md"), "added by the user").unwrap();
    let sha = repo.commit_all("heron: user note").unwrap();

    let mut meta = repo.read_metadata().unwrap().unwrap();
    meta.baseline.commit = repo.rev_parse(&format!("{sha}^")).unwrap();
    repo.write_metadata(&meta).unwrap();
    let _ = repo.commit_all("heron: baseline");

    let changed = repo.changed_since_baseline().unwrap();
    assert!(changed.iter().any(|p| p == "notes.md"), "got {changed:?}");
}
