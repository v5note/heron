//! Adopt's three-way split: a real directory, a link Heron made, a link the user made — and the
//! installer lockfile that says where the content came from.

use std::fs;
use std::path::{Path, PathBuf};

use heron_platform::dirs::Dirs;
use heron_service::adopt::{self, AdoptPlacement, EntryState};
use heron_service::scan::{self, ScanRequest};
use heron_service::Ctx;
use heron_store::db::Db;
use heron_store::repos::{binding, detected, projection, skill};

struct Env {
    /// Kept so the temporary home outlives the test body.
    #[allow(dead_code)]
    tmp: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let data = tmp.path().join("data");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&data).unwrap();
        Self { tmp, home, data }
    }

    fn ctx(&self) -> Ctx {
        let dirs = Dirs::resolve_with_override("heron", Some(&self.data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        Ctx::new(dirs, db, "cli")
    }

    fn skills_dir(&self) -> PathBuf {
        self.home.join(".agents/skills")
    }
}

fn write_skill(dir: &Path, name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n"),
    )
    .unwrap();
    fs::write(dir.join("notes.md"), format!("notes for {name}")).unwrap();
}

fn candidate(ctx: &Ctx, slug: &str) -> heron_store::models::DetectedRow {
    detected::list_by_state(ctx.db.conn(), "candidate")
        .unwrap()
        .into_iter()
        .find(|r| r.observed_slug.as_deref() == Some(slug))
        .unwrap_or_else(|| panic!("no candidate row for {slug}"))
}

/// A link the user made records a decision about where the content lives. Adopting must keep it.
#[test]
fn a_foreign_link_is_bound_in_place_rather_than_repointed() {
    let env = Env::new();
    let real = env.home.join("personal/motion-design");
    write_skill(&real, "motion-design");
    let skills = env.skills_dir();
    fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&real, skills.join("motion-design")).unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let row = candidate(&ctx, "motion-design");

    let plan = adopt::plan(&ctx, &row).unwrap();
    assert!(
        matches!(plan.entry, EntryState::ForeignLink { .. }),
        "the scanner found a link we did not create: {:?}",
        plan.entry
    );
    assert_eq!(plan.suggested, AdoptPlacement::LiveBind);

    let report = adopt::execute(&mut ctx, &plan, plan.suggested).unwrap();
    assert_eq!(report.adopted, vec!["motion-design".to_owned()]);

    // Nothing on disk moved, and nothing was copied.
    assert_eq!(
        fs::read_link(skills.join("motion-design")).unwrap(),
        real,
        "the user's own link keeps its target"
    );
    assert!(real.join("SKILL.md").is_file());
    assert!(
        !ctx.skill_path("motion-design").exists(),
        "a live bind creates no store copy"
    );
    assert!(
        !real.join("heron.skill.json").exists(),
        "a live bind writes nothing into someone else's directory"
    );

    // The library knows the authoritative content is outside the store.
    let stored = skill::get(ctx.db.conn(), "motion-design")
        .unwrap()
        .expect("skill row");
    assert_eq!(
        stored.storage_path.as_deref(),
        Some(real.canonicalize().unwrap().to_str().unwrap())
    );

    // And the entry the client already reads is booked as that skill's projection, against the
    // client directory it sits in so `verify` judges the right paths.
    let links = projection::list_for_skill(ctx.db.conn(), "motion-design").unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(
        links[0].path,
        skills.join("motion-design").to_string_lossy()
    );
    assert!(
        links[0].link_name.starts_with("agents@"),
        "link name carries the client: {}",
        links[0].link_name
    );
    assert_eq!(
        detected::get(ctx.db.conn(), &row.path)
            .unwrap()
            .unwrap()
            .adopt_state,
        "adopted"
    );

    // Adopting the same entry twice must not stack a second projection row.
    let again = adopt::plan(&ctx, &row).unwrap();
    let second = adopt::execute(&mut ctx, &again, AdoptPlacement::LiveBind).unwrap();
    assert_eq!(second.adopted, vec!["motion-design".to_owned()]);
    assert_eq!(
        projection::list_for_skill(ctx.db.conn(), "motion-design")
            .unwrap()
            .len(),
        1,
        "the projection is recorded once"
    );
}

/// Copying a foreign link into the store stays available, but it has to admit what it leaves behind.
#[test]
fn copying_a_foreign_link_is_opt_in_and_reports_the_original() {
    let env = Env::new();
    let real = env.home.join("personal/motion-design");
    write_skill(&real, "motion-design");
    let skills = env.skills_dir();
    fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&real, skills.join("motion-design")).unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let plan = adopt::plan(&ctx, &candidate(&ctx, "motion-design")).unwrap();

    let report = adopt::execute(&mut ctx, &plan, AdoptPlacement::Replace).unwrap();
    assert_eq!(report.adopted, vec!["motion-design".to_owned()]);
    assert!(
        matches!(&plan.entry, EntryState::ForeignLink { target } if *target == real.canonicalize().unwrap().to_string_lossy()),
        "the plan names the directory a copy would stop referencing: {:?}",
        plan.entry
    );

    // The entry is now Heron's, and the user's directory is untouched.
    assert!(ctx.skill_path("motion-design").join("SKILL.md").is_file());
    let info = heron_platform::link::probe(&skills.join("motion-design")).unwrap();
    assert!(info.kind.is_link(), "{:?}", info.kind);
    // Heron's own links are written relative to the client directory, so compare resolved paths.
    assert!(fs::canonicalize(skills.join("motion-design"))
        .unwrap()
        .starts_with(ctx.store_root().canonicalize().unwrap()));
    assert!(real.join("notes.md").is_file());
    assert!(
        !real.join("heron.skill.json").exists(),
        "the left-behind original is not converted"
    );
}

/// `npx skills` leaves a lockfile next to the entries it installed; adopt must not lose that.
#[test]
fn installer_lockfile_provenance_is_recorded_on_adopt() {
    let env = Env::new();
    let skills = env.skills_dir();
    write_skill(&skills.join("pdf"), "pdf");
    fs::write(
        skills.join("skills-lock.json"),
        serde_json::json!({
            "version": 1,
            "skills": {
                "pdf": {
                    "source": "someone/repo",
                    "sourceType": "github",
                    "skillPath": "skills/pdf",
                    "computedHash": "deadbeef"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let plan = adopt::plan(&ctx, &candidate(&ctx, "pdf")).unwrap();
    let upstream = plan.upstream.clone().expect("lockfile answered");
    assert_eq!(
        upstream.url.as_deref(),
        Some("https://github.com/someone/repo.git")
    );
    assert_eq!(upstream.subpath.as_deref(), Some("skills"));
    assert_eq!(plan.suggested, AdoptPlacement::Replace);

    adopt::execute(&mut ctx, &plan, AdoptPlacement::Replace).unwrap();
    let rows = binding::get(ctx.db.conn(), "pdf").unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, "git");
    assert_eq!(
        rows[0].url.as_deref(),
        Some("https://github.com/someone/repo.git")
    );
    assert_eq!(rows[0].subpath.as_deref(), Some("skills"));
    assert!(
        rows[0].content_hash.is_none(),
        "an installer hash is not a git revision and must not become a merge baseline"
    );

    // The replacement link sits inside `.agents/skills`, so the projection belongs to that client.
    // Booked on the landing instead, `verify` names the wrong client and `unproject --on` misses it.
    let links = projection::list_for_skill(ctx.db.conn(), "pdf").unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].target_id, "agents:global", "{links:?}");
}
