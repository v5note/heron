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

/// A skill reached through a user-made symlink is discovered at its real directory, not the link,
/// and is then adopted like any other directory.
#[test]
fn a_user_symlinked_skill_is_discovered_at_its_real_directory() {
    let env = Env::new();
    let real = env.home.join("personal/motion-design");
    write_skill(&real, "motion-design");
    let skills = env.skills_dir();
    fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&real, skills.join("motion-design")).unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let row = candidate(&ctx, "motion-design");
    // Capture the home before adopt turns it into a store link.
    let real_canon = real.canonicalize().unwrap();

    // Discover reports the skill's own home, never the link the user made to reach it.
    assert_eq!(
        Path::new(&row.path),
        real_canon.join("SKILL.md"),
        "the row points at the real directory, not the symlink"
    );
    assert_eq!(row.real_path.as_deref(), Some(real_canon.to_str().unwrap()));

    let plan = adopt::plan(&ctx, &row).unwrap();
    // Reached as a real directory, so it is suggested as an ordinary move into the store.
    assert!(
        matches!(plan.entry, EntryState::RealDir),
        "{:?}",
        plan.entry
    );
    assert_eq!(plan.suggested, AdoptPlacement::Replace);

    let report = adopt::execute(&mut ctx, &plan, plan.suggested).unwrap();
    assert_eq!(report.adopted, vec!["motion-design".to_owned()]);

    // The content is now in the store, and the directory it lived in points into the store.
    assert!(ctx.skill_path("motion-design").join("SKILL.md").is_file());
    let info = heron_platform::link::probe(&real).unwrap();
    assert!(info.kind.is_link(), "{:?}", info.kind);
    assert!(fs::canonicalize(&real)
        .unwrap()
        .starts_with(ctx.store_root().canonicalize().unwrap()));

    // The user's original symlink is left exactly where it was and now resolves through to the store.
    assert_eq!(fs::read_link(skills.join("motion-design")).unwrap(), real);
    assert!(skills.join("motion-design").join("SKILL.md").is_file());

    // The projection is booked against the real directory, where the store link now sits.
    let links = projection::list_for_skill(ctx.db.conn(), "motion-design").unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].path, real_canon.to_string_lossy());

    // But no landing point is invented for the directory the content really sits in: such a card
    // points somewhere no client reads, and it turns into a broken link the moment that third-party
    // checkout is deleted. The entry is booked against the client's own skills directory instead.
    let targets = heron_store::repos::target::list(ctx.db.conn()).unwrap();
    assert!(
        targets.iter().all(|t| t.discover != "manual"),
        "adopt registered a directory no client reads: {targets:?}"
    );

    assert_eq!(
        detected::get(ctx.db.conn(), &row.path)
            .unwrap()
            .unwrap()
            .adopt_state,
        "adopted"
    );
}

/// A row that still names a user's link (a legacy scan, or an explicit path) is bound in place —
/// the live-bind path stays intact even though the scanner now resolves links away.
#[test]
fn a_row_that_still_points_at_a_foreign_link_is_bound_in_place() {
    let env = Env::new();
    let real = env.home.join("personal/motion-design");
    write_skill(&real, "motion-design");
    let skills = env.skills_dir();
    fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&real, skills.join("motion-design")).unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let mut row = candidate(&ctx, "motion-design");
    // Re-point the row at the link, the way a scan before this change would have recorded it.
    row.path = skills
        .join("motion-design")
        .join("SKILL.md")
        .to_string_lossy()
        .to_string();

    let plan = adopt::plan(&ctx, &row).unwrap();
    assert!(
        matches!(&plan.entry, EntryState::ForeignLink { target }
            if *target == real.canonicalize().unwrap().to_string_lossy()),
        "a link row is still recognised as foreign: {:?}",
        plan.entry
    );
    assert_eq!(plan.suggested, AdoptPlacement::LiveBind);

    let report = adopt::execute(&mut ctx, &plan, plan.suggested).unwrap();
    assert_eq!(report.adopted, vec!["motion-design".to_owned()]);

    // Nothing on disk moved, and no store copy was made.
    assert_eq!(fs::read_link(skills.join("motion-design")).unwrap(), real);
    assert!(real.join("SKILL.md").is_file());
    assert!(
        !ctx.skill_path("motion-design").exists(),
        "a live bind creates no store copy"
    );
    assert!(
        !real.join("heron.skill.json").exists(),
        "a live bind writes nothing into someone else's directory"
    );
    let stored = skill::get(ctx.db.conn(), "motion-design")
        .unwrap()
        .expect("skill row");
    assert_eq!(
        stored.storage_path.as_deref(),
        Some(real.canonicalize().unwrap().to_str().unwrap())
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
    // Booked against any other target, `verify` names the wrong client and `unproject --on` misses it.
    let links = projection::list_for_skill(ctx.db.conn(), "pdf").unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].target_id, "agents:global", "{links:?}");
}
