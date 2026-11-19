//! End-to-end install / outdated / upgrade / rollback against a local bare repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use heron_core::PinPolicy;
use heron_platform::dirs::Dirs;
use heron_service::install::{self, InstallRequest};
use heron_service::upgrade::{self, UpgradeOutcome, UpgradeState};
use heron_service::Ctx;
use heron_store::db::Db;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Remote with one skill under `skills/pdf`.
struct Remote {
    tmp: tempfile::TempDir,
    work: PathBuf,
    bare: PathBuf,
}

impl Remote {
    fn new(body: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "Test"]);
        write(
            &work.join("skills/pdf/SKILL.md"),
            &format!("---\nname: pdf\ndescription: pdf skill\nmetadata:\n  version: \"1.0.0\"\n---\n{body}"),
        );
        write(&work.join("skills/pdf/references/a.md"), "ref v1");
        write(
            &work.join("skills/tdd/SKILL.md"),
            "---\nname: tdd\ndescription: tdd\n---\n",
        );
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-qm", "v1"]);
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
        Self { tmp, work, bare }
    }

    fn publish(&self, body: &str, version: &str) {
        write(
            &self.work.join("skills/pdf/SKILL.md"),
            &format!("---\nname: pdf\ndescription: pdf skill\nmetadata:\n  version: \"{version}\"\n---\n{body}"),
        );
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-qm", version]);
        git(
            &self.work,
            &["push", "-q", self.bare.to_str().unwrap(), "main"],
        );
    }

    fn url(&self) -> String {
        self.bare.to_str().unwrap().to_owned()
    }
}

struct Env {
    /// Kept alive for the duration of the test; removing it deletes the data directory.
    _tmp: tempfile::TempDir,
    data: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).unwrap();
        Self { _tmp: tmp, data }
    }

    fn ctx(&self) -> Ctx {
        let dirs = Dirs::resolve_with_override("heron", Some(&self.data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        Ctx::new(dirs, db, "cli")
    }
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn install_from_a_repo_subdirectory() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("body v1\n");
    let env = Env::new();
    let mut ctx = env.ctx();

    let req = InstallRequest {
        input: format!(
            "{}/tree/main/skills/pdf",
            remote.url().trim_end_matches(".git")
        ),
        ..Default::default()
    };
    let preview = install::preview(&ctx, &req).unwrap();
    assert_eq!(preview.slug, "pdf");
    assert!(!preview.existing);
    assert!(preview.available.iter().any(|p| p == "skills/pdf"));

    let report = install::install(&mut ctx, &req).unwrap();
    assert_eq!(report.slug, "pdf");
    assert_eq!(report.version, "1.0.0");

    let dir = ctx.skill_path("pdf");
    assert!(dir.join("SKILL.md").is_file());
    assert!(dir.join("references/a.md").is_file());
    assert!(dir.join(".git").is_dir(), "installed skills are versioned");
    let meta = heron_vcs::SkillRepo::open(&dir)
        .read_metadata()
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.source.as_ref().unwrap().subpath.as_deref(),
        Some("skills/pdf")
    );
    assert_eq!(meta.baseline.version, "1.0.0");
}

#[test]
fn ambiguous_repo_without_subpath_is_reported() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("x");
    let env = Env::new();
    let ctx = env.ctx();
    let req = InstallRequest {
        input: remote.url(),
        ..Default::default()
    };
    let err = install::preview(&ctx, &req).unwrap_err();
    assert_eq!(err.code(), "E_SOURCE_AMBIGUOUS");
    let msg = err.as_msg();
    assert_eq!(
        msg.params.get("count").map(String::as_str),
        Some("2"),
        "both skills listed"
    );
}

#[test]
fn install_twice_is_refused_in_favour_of_upgrade() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("v1");
    let env = Env::new();
    let mut ctx = env.ctx();
    let req = InstallRequest {
        input: remote.url(),
        subpath_override: Some("skills/pdf".into()),
        ..Default::default()
    };
    install::install(&mut ctx, &req).unwrap();
    let err = install::install(&mut ctx, &req).unwrap_err();
    assert_eq!(err.code(), "E_SLUG_CONFLICT");
    let msg = err.as_msg();
    assert_eq!(
        msg.params.get("detail").map(String::as_str),
        Some("use upgrade for an already installed skill")
    );
}

#[test]
fn outdated_then_upgrade_then_rollback() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("v1 body\n");
    let env = Env::new();
    let mut ctx = env.ctx();
    let req = InstallRequest {
        input: remote.url(),
        subpath_override: Some("skills/pdf".into()),
        ..Default::default()
    };
    install::install(&mut ctx, &req).unwrap();

    // No update yet.
    upgrade::refresh_sources(&ctx).unwrap();
    let cands = upgrade::outdated(&ctx, PinPolicy::Semver).unwrap();
    assert_eq!(cands[0].state, UpgradeState::UpToDate);

    // Upstream publishes v1.1.0.
    remote.publish("v2 body\n", "1.1.0");
    upgrade::refresh_sources(&ctx).unwrap();
    let cands = upgrade::outdated(&ctx, PinPolicy::Semver).unwrap();
    assert_eq!(cands[0].state, UpgradeState::HasUpdate);

    match upgrade::upgrade(&mut ctx, "pdf").unwrap() {
        UpgradeOutcome::Upgraded { from, to, .. } => assert_ne!(from, to),
        other => panic!("expected an upgrade, got {other:?}"),
    }
    let text = fs::read_to_string(ctx.skill_path("pdf").join("SKILL.md")).unwrap();
    assert!(text.contains("v2 body"));
    assert!(text.contains("version: \"1.1.0\"") || text.contains("version: 1.1.0"));

    // The landed version comes from the upstream tag / SKILL.md metadata, never the 0.0.0 fallback.
    let meta = heron_vcs::SkillRepo::open(ctx.skill_path("pdf"))
        .read_metadata()
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.baseline.version, "1.1.0",
        "semantic version is preserved on upgrade"
    );
    let tags = heron_vcs::SkillRepo::open(ctx.skill_path("pdf"))
        .list_tags()
        .unwrap();
    assert!(
        tags.contains(&"v1.1.0".to_owned()),
        "tagged with the real version: {tags:?}"
    );

    // Rolling back restores the first version and keeps a backup tag.
    let previous = upgrade::rollback(&mut ctx, "pdf", "1.0.0").unwrap();
    assert!(!previous.is_empty());
    let text = fs::read_to_string(ctx.skill_path("pdf").join("SKILL.md")).unwrap();
    assert!(text.contains("v1 body\n"), "rolled back content: {text}");
    let tags = heron_vcs::SkillRepo::open(ctx.skill_path("pdf"))
        .list_tags()
        .unwrap();
    assert!(tags.iter().any(|t| t.starts_with("heron-rollback-backup-")));
    let _ = remote.tmp;
}

#[test]
fn diff_shows_local_edits_and_conflict_payload_carries_both_sides() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("line1\nline2\nline3\nline4\nline5\n");
    let env = Env::new();
    let mut ctx = env.ctx();
    let req = InstallRequest {
        input: remote.url(),
        subpath_override: Some("skills/pdf".into()),
        ..Default::default()
    };
    install::install(&mut ctx, &req).unwrap();

    let skill_md = ctx.skill_path("pdf").join("SKILL.md");
    let text = fs::read_to_string(&skill_md).unwrap();
    fs::write(&skill_md, text.replace("line5", "MY-EDIT")).unwrap();

    let diff = heron_service::diff::diff_skill(&ctx, "pdf", None).unwrap();
    assert_eq!(diff.against, "local");
    let entry = diff.files.iter().find(|f| f.path == "SKILL.md").unwrap();
    assert!(entry.after.as_deref().unwrap().contains("MY-EDIT"));
    assert!(
        !entry.before.as_deref().unwrap().contains("MY-EDIT"),
        "before is the baseline"
    );

    // Upstream rewrites the same line -> genuine conflict.
    remote.publish("line1\nline2\nline3\nline4\nTHEIRS\n", "2.0.0");
    upgrade::refresh_sources(&ctx).unwrap();
    let payload = heron_service::diff::conflict_payload(&ctx, "pdf").unwrap();
    assert_eq!(payload.slug, "pdf");
    let file = payload.files.iter().find(|f| f.path == "SKILL.md").unwrap();
    assert!(file.before.as_deref().unwrap().contains("MY-EDIT"), "ours");
    assert!(file.after.as_deref().unwrap().contains("THEIRS"), "theirs");

    // Resolving by hand and retrying produces a clean upgrade.
    let resolved = file.after.clone().unwrap().replace("THEIRS", "RESOLVED");
    match heron_service::diff::resolve(&mut ctx, "pdf", &[("SKILL.md".to_owned(), resolved)])
        .unwrap()
    {
        UpgradeOutcome::Upgraded { .. } | UpgradeOutcome::Conflicts { .. } => {}
        other => panic!("unexpected outcome {other:?}"),
    }
    let merged = fs::read_to_string(&skill_md).unwrap();
    assert!(
        merged.contains("RESOLVED") || merged.contains("THEIRS"),
        "resolution applied: {merged}"
    );
    let _ = remote.tmp;
}

#[test]
fn local_edits_survive_an_upgrade_and_conflicts_are_reported() {
    if !git_available() {
        return;
    }
    let remote = Remote::new("line1\nline2\nline3\nline4\nline5\n");
    let env = Env::new();
    let mut ctx = env.ctx();
    let req = InstallRequest {
        input: remote.url(),
        subpath_override: Some("skills/pdf".into()),
        ..Default::default()
    };
    install::install(&mut ctx, &req).unwrap();

    // The user edits the last line locally.
    let skill_md = ctx.skill_path("pdf").join("SKILL.md");
    let text = fs::read_to_string(&skill_md).unwrap();
    fs::write(&skill_md, text.replace("line5", "LOCAL-EDIT")).unwrap();

    // Upstream edits the first line.
    remote.publish("UPSTREAM\nline2\nline3\nline4\nline5\n", "1.1.0");
    upgrade::refresh_sources(&ctx).unwrap();
    let cands = upgrade::outdated(&ctx, PinPolicy::Semver).unwrap();
    assert_eq!(
        cands[0].state,
        UpgradeState::Diverged,
        "local edits make it diverged"
    );

    match upgrade::upgrade(&mut ctx, "pdf").unwrap() {
        UpgradeOutcome::Upgraded { .. } => {}
        other => panic!("expected clean merge, got {other:?}"),
    }
    let merged = fs::read_to_string(&skill_md).unwrap();
    assert!(merged.contains("LOCAL-EDIT"), "local edit kept: {merged}");
    assert!(merged.contains("UPSTREAM"), "upstream applied: {merged}");

    // Now a genuine conflict: both sides change the same line.
    fs::write(&skill_md, merged.replace("line2", "LOCAL-LINE2")).unwrap();
    remote.publish("UPSTREAM\nTHEIR-LINE2\nline3\nline4\nline5\n", "1.2.0");
    upgrade::refresh_sources(&ctx).unwrap();
    let before = fs::read_to_string(&skill_md).unwrap();
    match upgrade::upgrade(&mut ctx, "pdf").unwrap() {
        UpgradeOutcome::Conflicts { files, .. } => {
            assert!(files.iter().any(|f| f.path == "SKILL.md"), "got {files:?}");
        }
        other => panic!("expected conflicts, got {other:?}"),
    }
    let after = fs::read_to_string(&skill_md).unwrap();
    assert_eq!(
        before, after,
        "an unsuccessful merge leaves the skill untouched"
    );
    let _ = remote.tmp;
}
