//! End-to-end against a local bare repository (no network required).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use heron_git::{cache::cache_dir_for, CachedRepo, RefKind, RemoteUrl};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git must be installed");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// origin repo with two skills in `skills/`, a tag `v1.0.0`, plus a bare mirror.
struct Remote {
    _tmp: tempfile::TempDir,
    work: PathBuf,
    bare: PathBuf,
}

impl Remote {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "Test"]);

        write(
            &work.join("skills/wechat-mp/SKILL.md"),
            "---\nname: wechat-mp\ndescription: v1\n---\n",
        );
        write(&work.join("skills/wechat-mp/references/a.md"), "ref");
        write(
            &work.join("skills/tdd/SKILL.md"),
            "---\nname: tdd\ndescription: v1\n---\n",
        );
        write(&work.join("README.md"), "repo readme");
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-qm", "v1"]);
        git(&work, &["tag", "v1.0.0"]);

        let bare = tmp.path().join("remote.git");
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        Self {
            _tmp: tmp,
            work,
            bare,
        }
    }

    fn publish_update(&self, description: &str) {
        write(
            &self.work.join("skills/wechat-mp/SKILL.md"),
            &format!("---\nname: wechat-mp\ndescription: {description}\n---\n"),
        );
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-qm", "v2"]);
        git(&self.work, &["tag", "v2.0.0"]);
        git(
            &self.work,
            &["push", "-q", "--tags", self.bare.to_str().unwrap(), "main"],
        );
    }

    fn url(&self) -> String {
        self.bare.to_str().unwrap().to_owned()
    }
}

fn cached(remote: &Remote, cache_root: &Path) -> CachedRepo {
    let url = RemoteUrl::parse(&remote.url()).unwrap();
    CachedRepo::new(cache_root, url)
}

#[test]
fn ensure_creates_bare_cache_and_resolves_refs() {
    if !git_available() {
        return;
    }
    let remote = Remote::new();
    let cache_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());

    repo.ensure().unwrap();
    assert!(repo.path().join("HEAD").is_file(), "bare repo created");
    assert_eq!(repo.path(), cache_dir_for(cache_root.path(), repo.remote()));

    let head = repo.resolve(None).unwrap();
    assert_eq!(head.kind, RefKind::Branch);
    let explicit = repo.resolve(Some("main")).unwrap();
    assert_eq!(
        explicit.commit, head.commit,
        "explicit branch == default branch"
    );

    let tags = repo.tags().unwrap();
    assert!(tags
        .iter()
        .any(|t| t.name == "v1.0.0" && t.kind == RefKind::Tag));

    let short = &explicit.commit[..8];
    assert_eq!(repo.resolve(Some(short)).unwrap().kind, RefKind::Commit);

    let err = repo.resolve(Some("does-not-exist")).unwrap_err();
    assert_eq!(err.code(), "E_REF_NOT_FOUND");
}

#[test]
fn ensure_is_idempotent_and_picks_up_new_commits() {
    if !git_available() {
        return;
    }
    let remote = Remote::new();
    let cache_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());

    repo.ensure().unwrap();
    let first = repo.resolve(Some("main")).unwrap().commit;
    repo.ensure().unwrap();
    assert_eq!(
        repo.resolve(Some("main")).unwrap().commit,
        first,
        "no-op refresh keeps the commit"
    );

    remote.publish_update("v2");
    repo.ensure().unwrap();
    let second = repo.resolve(Some("main")).unwrap().commit;
    assert_ne!(first, second, "refresh sees the new upstream commit");
    assert!(repo.tags().unwrap().iter().any(|t| t.name == "v2.0.0"));
}

#[test]
fn tree_lists_a_subdirectory_only() {
    if !git_available() {
        return;
    }
    let remote = Remote::new();
    let cache_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());
    repo.ensure().unwrap();
    let head = repo.resolve(Some("main")).unwrap().commit;

    let entries = repo.tree(&head, "skills/wechat-mp").unwrap();
    let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["SKILL.md", "references/a.md"]);
    assert!(entries
        .iter()
        .any(|e| e.kind == heron_git::cache::TreeEntryKind::File));
}

#[test]
fn checkout_subdir_materializes_exactly_the_skill() {
    if !git_available() {
        return;
    }
    let remote = Remote::new();
    let cache_root = tempfile::tempdir().unwrap();
    let dest_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());
    repo.ensure().unwrap();
    let head = repo.resolve(Some("main")).unwrap().commit;

    let dest = dest_root.path().join("wechat-mp");
    repo.checkout_subdir(&head, "skills/wechat-mp", &dest).unwrap();

    assert!(dest.join("SKILL.md").is_file());
    assert!(dest.join("references/a.md").is_file());
    assert!(
        !dest.join("README.md").exists(),
        "only the subdirectory is extracted"
    );
    assert!(
        !dest.join(".git").exists(),
        "no working tree metadata leaks in"
    );
    assert!(
        !dest_root.path().join(".wechat-mp-heron-extract").exists(),
        "temp dir is swapped in"
    );
}

#[test]
fn checkout_subdir_keeps_binary_bytes_intact() {
    if !git_available() {
        return;
    }
    // A skill that ships an image is the common case; text-captured tar bytes would replace every
    // invalid UTF-8 sequence and the archive would either fail to parse or land corrupt files.
    let blob: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x00, 0xff, 0xfe, 0x01, 0xc2, 0x20];
    let remote = Remote::new();
    fs::write(remote.work.join("skills/wechat-mp/logo.png"), &blob).unwrap();
    git(&remote.work, &["add", "-A"]);
    git(&remote.work, &["commit", "-qm", "binary"]);
    git(
        &remote.work,
        &["push", "-q", remote.bare.to_str().unwrap(), "main"],
    );

    let cache_root = tempfile::tempdir().unwrap();
    let dest_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());
    repo.ensure().unwrap();
    let head = repo.resolve(Some("main")).unwrap().commit;

    let dest = dest_root.path().join("wechat-mp");
    repo.checkout_subdir(&head, "skills/wechat-mp", &dest).unwrap();
    assert_eq!(fs::read(dest.join("logo.png")).unwrap(), blob);
    assert!(
        !dest_root.path().join(".wechat-mp-heron.tar").exists(),
        "tarball is cleaned up"
    );
}

#[test]
fn checkout_refuses_to_overwrite_an_existing_destination() {
    if !git_available() {
        return;
    }
    let remote = Remote::new();
    let cache_root = tempfile::tempdir().unwrap();
    let dest_root = tempfile::tempdir().unwrap();
    let mut repo = cached(&remote, cache_root.path());
    repo.ensure().unwrap();
    let head = repo.resolve(Some("main")).unwrap().commit;

    let dest = dest_root.path().join("wechat-mp");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("user.md"), "mine").unwrap();

    let err = repo
        .checkout_subdir(&head, "skills/wechat-mp", &dest)
        .unwrap_err();
    assert_eq!(err.code(), "E_SAFETY_REFUSED");
    assert!(dest.join("user.md").is_file());
}

#[test]
fn missing_remote_is_a_network_error_with_context() {
    if !git_available() {
        return;
    }
    let cache_root = tempfile::tempdir().unwrap();
    let url = RemoteUrl::parse("https://127.0.0.1:1/nope.git").unwrap();
    let mut repo = CachedRepo::new(cache_root.path(), url);
    let err = repo.ensure().unwrap_err();
    assert_eq!(err.code(), "E_NETWORK");
    let msg = err.as_msg();
    assert_eq!(
        msg.params.get("url").map(String::as_str),
        Some("https://127.0.0.1:1/nope.git")
    );
}
