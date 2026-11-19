//! Link-layer safety tests (implementation doc §10.2).
//!
//! The single most destructive failure this product can have is deleting a *target* while
//! removing a link. Every test therefore writes a sentinel file inside the target, performs
//! the operation, and asserts the sentinel is still there afterwards.
//!
//! The Windows-only cases (junctions, symlink privilege fallback, long paths) live in
//! `link_safety_windows.rs` and are gated by `cfg(windows)`.

use std::path::{Path, PathBuf};

use heron_core::ProjectionMode;
use heron_platform::link::{self, LinkKind, LinkOutcome};
use tempfile::TempDir;

fn make_target(root: &TempDir) -> PathBuf {
    let target = root.path().join("store").join("pdf");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
    std::fs::write(target.join("sentinel.txt"), "do not delete me").unwrap();
    target
}

fn assert_target_intact(target: &Path) {
    assert!(
        target.is_dir(),
        "target directory disappeared: {}",
        target.display()
    );
    let sentinel = target.join("sentinel.txt");
    assert!(
        sentinel.is_file(),
        "sentinel vanished — link removal recursed into the target"
    );
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "do not delete me"
    );
    assert!(target.join("SKILL.md").is_file());
}

/// L-core-1: create, probe, remove — target content must survive.
#[test]
fn create_probe_remove_keeps_target() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let link_path = root
        .path()
        .join("project")
        .join(".heron")
        .join("skills")
        .join("pdf");

    let outcome = link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();
    match &outcome {
        LinkOutcome::Created(kind) => assert!(kind.is_link(), "expected a link, got {kind:?}"),
        LinkOutcome::Downgraded { to, .. } => assert!(to.is_link()),
        LinkOutcome::AlreadyPresent => panic!("fresh path reported AlreadyPresent"),
    }
    assert!(
        link_path.join("sentinel.txt").is_file(),
        "link does not resolve to the target"
    );

    let info = link::probe(&link_path).unwrap();
    assert!(info.kind.is_link());
    assert!(info.resolved_exists);

    link::remove_link(&link_path).unwrap();
    assert!(!link_path.exists(), "link still present after removal");
    assert_target_intact(&target);
}

/// L-core-2: linking twice is idempotent.
#[test]
fn create_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let link_path = root.path().join("skills").join("pdf");

    link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();
    let second = link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();
    assert_eq!(second, LinkOutcome::AlreadyPresent);
    assert_target_intact(&target);
}

/// L-core-3: a real directory in the way is never deleted.
#[test]
fn refuses_to_replace_real_directory() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let occupied = root.path().join("skills").join("pdf");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(occupied.join("keep.txt"), "precious").unwrap();

    let err = link::create_dir_link(&target, &occupied, ProjectionMode::Symlink).unwrap_err();
    assert_eq!(err.code(), "E_SAFETY_REFUSED");
    assert!(
        occupied.join("keep.txt").is_file(),
        "user data in the way was destroyed"
    );
}

/// L-core-4: removing something that is not a link is refused.
#[test]
fn remove_refuses_non_link() {
    let root = tempfile::tempdir().unwrap();
    let real_dir = root.path().join("real");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::write(real_dir.join("a.txt"), "a").unwrap();

    let err = link::remove_link(&real_dir).unwrap_err();
    assert_eq!(err.code(), "E_SAFETY_REFUSED");
    assert!(real_dir.join("a.txt").is_file());

    let real_file = root.path().join("f.txt");
    std::fs::write(&real_file, "f").unwrap();
    let err = link::remove_link(&real_file).unwrap_err();
    assert_eq!(err.code(), "E_SAFETY_REFUSED");
    assert!(real_file.is_file());
}

/// L-core-5: create requires an existing directory target.
#[test]
fn create_rejects_missing_target() {
    let root = tempfile::tempdir().unwrap();
    let link_path = root.path().join("skills").join("ghost");
    let err = link::create_dir_link(
        &root.path().join("nope"),
        &link_path,
        ProjectionMode::Symlink,
    )
    .unwrap_err();
    assert_eq!(err.code(), "E_LINK_TARGET_MISSING");
    assert!(!link_path.exists());
}

/// L-core-6: relink moves the pointer without touching either target.
#[test]
fn relink_switches_target_and_keeps_both() {
    let root = tempfile::tempdir().unwrap();
    let old = make_target(&root);
    let new = root.path().join("store").join("pdf-v2");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("SKILL.md"), "v2").unwrap();
    std::fs::write(new.join("sentinel.txt"), "do not delete me").unwrap();

    let link_path = root.path().join("skills").join("pdf");
    link::create_dir_link(&old, &link_path, ProjectionMode::Symlink).unwrap();
    link::relink(&link_path, &new).unwrap();

    assert_eq!(
        std::fs::read_to_string(link_path.join("SKILL.md")).unwrap(),
        "v2"
    );
    assert_target_intact(&old);
    assert_target_intact(&new);
}

/// L-core-7: broken links are reported, not silently treated as healthy.
#[test]
fn broken_link_is_detected_and_removable() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let link_path = root.path().join("skills").join("pdf");
    link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();

    std::fs::remove_dir_all(&target).unwrap();

    let info = link::probe(&link_path).unwrap();
    assert!(info.kind.is_link(), "a dangling link is still a link");
    assert!(
        !info.resolved_exists,
        "dangling link must report the target as missing"
    );

    link::remove_link(&link_path).unwrap();
    assert!(!link_path.exists());
}

/// L-core-8: copy/pointer modes must not be handled by the link layer.
#[test]
fn link_layer_rejects_copy_and_pointer() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let link_path = root.path().join("skills").join("pdf");
    for mode in [ProjectionMode::Copy, ProjectionMode::Pointer] {
        let err = link::create_dir_link(&target, &link_path, mode).unwrap_err();
        assert_eq!(err.code(), "E_INTERNAL");
        assert!(!link_path.exists());
    }
}

/// L-core-9: probing a missing path is `Missing`, not an error.
#[test]
fn probe_missing_path() {
    let root = tempfile::tempdir().unwrap();
    let info = link::probe(&root.path().join("absent")).unwrap();
    assert_eq!(info.kind, LinkKind::Missing);
    assert!(info.target.is_none());
    // Removing a missing link is a no-op, not a failure.
    link::remove_link(&root.path().join("absent")).unwrap();
}

/// L-core-10: creating nested parents works (project targets under .heron/skills).
#[test]
fn creates_missing_parent_directories() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root);
    let link_path = root.path().join("a").join("b").join("c").join("pdf");
    link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();
    assert!(link_path.join("sentinel.txt").is_file());
    assert_target_intact(&target);
}
