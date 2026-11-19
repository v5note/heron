//! Windows-only link safety tests (implementation doc §10.2, cases L1–L6).
//!
//! These are the guardrail for risk R3 ("junction removal deletes the target"). They must
//! pass in a **non-elevated** CI context on `windows-2022`.

#![cfg(windows)]

use std::path::{Path, PathBuf};

use heron_core::ProjectionMode;
use heron_platform::link::{self, LinkKind, LinkOutcome};
use tempfile::TempDir;

fn make_target(root: &TempDir, name: &str) -> PathBuf {
    let target = root.path().join("store").join(name);
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
    std::fs::write(target.join("sentinel.txt"), "do not delete me").unwrap();
    target
}

fn assert_target_intact(target: &Path) {
    let sentinel = target.join("sentinel.txt");
    assert!(
        sentinel.is_file(),
        "sentinel vanished — link removal recursed into the target"
    );
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "do not delete me"
    );
}

/// L1: junction create + remove must leave the target untouched.
#[test]
fn l1_junction_create_and_remove_keeps_target() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "pdf");
    let link_path = root.path().join("skills").join("pdf");

    let outcome = link::create_dir_link(&target, &link_path, ProjectionMode::Junction).unwrap();
    assert!(matches!(
        outcome,
        LinkOutcome::Created(LinkKind::Junction) | LinkOutcome::AlreadyPresent
    ));

    assert_eq!(link::probe(&link_path).unwrap().kind, LinkKind::Junction);
    assert!(link_path.join("sentinel.txt").is_file());

    link::remove_link(&link_path).unwrap();
    assert!(!link_path.exists(), "junction still present");
    assert_target_intact(&target);
}

/// L2: asking for a symbolic link must succeed or degrade to a junction, never fail hard.
#[test]
fn l2_symlink_degrades_to_junction_without_privilege() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "pdf");
    let link_path = root.path().join("skills").join("pdf");

    match link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap() {
        LinkOutcome::Created(LinkKind::SymbolicDir) => {
            // Running elevated or with Developer Mode: the symlink path is exercised.
            assert_eq!(link::probe(&link_path).unwrap().kind, LinkKind::SymbolicDir);
        }
        LinkOutcome::Downgraded { from, to, warning } => {
            assert_eq!(from, LinkKind::SymbolicDir);
            assert_eq!(to, LinkKind::Junction);
            assert_eq!(warning.code(), "E_LINK_PRIVILEGE");
            assert_eq!(link::probe(&link_path).unwrap().kind, LinkKind::Junction);
        }
        other => panic!("unexpected outcome {other:?}"),
    }
    link::remove_link(&link_path).unwrap();
    assert_target_intact(&target);
}

/// L3: probe must tell a junction apart from a directory symlink.
#[test]
fn l3_probe_distinguishes_junction_from_symlink() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "pdf");

    let junction_path = root.path().join("skills").join("j");
    link::create_dir_link(&target, &junction_path, ProjectionMode::Junction).unwrap();
    assert_eq!(
        link::probe(&junction_path).unwrap().kind,
        LinkKind::Junction
    );

    let symlink_path = root.path().join("skills").join("s");
    match link::create_dir_link(&target, &symlink_path, ProjectionMode::Symlink).unwrap() {
        LinkOutcome::Created(LinkKind::SymbolicDir) => {
            assert_eq!(
                link::probe(&symlink_path).unwrap().kind,
                LinkKind::SymbolicDir
            );
        }
        LinkOutcome::Downgraded { .. } => {
            // No privilege: the "symlink" path is a junction, which probe must report as such.
            assert_eq!(link::probe(&symlink_path).unwrap().kind, LinkKind::Junction);
        }
        other => panic!("unexpected outcome {other:?}"),
    }

    assert_ne!(link::probe(&target).unwrap().kind, LinkKind::Junction);
    assert_eq!(link::probe(&target).unwrap().kind, LinkKind::Directory);
}

/// L4: a real directory is never removed through the link API.
#[test]
fn l4_remove_refuses_real_directory() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("a.txt"), "a").unwrap();

    let err = link::remove_link(&real).unwrap_err();
    assert_eq!(err.code(), "E_SAFETY_REFUSED");
    assert!(real.join("a.txt").is_file());
}

/// L5: long paths beyond MAX_PATH round-trip through the platform layer.
#[test]
fn l5_long_path_junction_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "pdf");

    let mut deep = root.path().to_path_buf();
    for _ in 0..6 {
        deep = deep.join("a-rather-long-directory-segment-for-testing");
    }
    let link_path = deep.join("pdf");
    link::create_dir_link(&target, &link_path, ProjectionMode::Junction).unwrap();
    assert!(link_path.join("sentinel.txt").is_file());
    link::remove_link(&link_path).unwrap();
    assert_target_intact(&target);
}

/// L6: a dangling link reports the missing target instead of pretending to be healthy.
#[test]
fn l6_broken_junction_reports_missing_target() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "pdf");
    let link_path = root.path().join("skills").join("pdf");
    link::create_dir_link(&target, &link_path, ProjectionMode::Junction).unwrap();

    std::fs::remove_dir_all(&target).unwrap();
    let info = link::probe(&link_path).unwrap();
    assert!(info.kind.is_link());
    assert!(!info.resolved_exists);

    link::remove_link(&link_path).unwrap();
    assert!(!link_path.exists());
}

#[test]
fn diag_raw_bytes() {
    let root = tempfile::tempdir().unwrap();
    let target = make_target(&root, "diag2");
    let link_path = root.path().join("skills").join("diag2");
    let outcome = link::create_dir_link(&target, &link_path, ProjectionMode::Symlink).unwrap();
    eprintln!("DIAG outcome={outcome:?}");
    eprintln!("DIAG raw={}", link::debug_raw_reparse(&link_path));
    eprintln!("DIAG probe={:?}", link::probe(&link_path));
}
