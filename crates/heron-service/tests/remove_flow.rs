//! Remove: the links come back, the store copy lands in the trash, the index forgets the slug —
//! and a directory the user owns is never moved.

use std::fs;
use std::path::{Path, PathBuf};

use heron_platform::dirs::Dirs;
use heron_service::adopt::{self, AdoptPlacement};
use heron_service::project::{self, ProjectRequest};
use heron_service::remove;
use heron_service::scan::{self, ScanRequest};
use heron_service::Ctx;
use heron_store::db::Db;
use heron_store::repos::{projection, skill};

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
    detected(ctx)
        .into_iter()
        .find(|r| r.observed_slug.as_deref() == Some(slug))
        .unwrap_or_else(|| panic!("no candidate row for {slug}"))
}

fn detected(ctx: &Ctx) -> Vec<heron_store::models::DetectedRow> {
    heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate").unwrap()
}

fn project(ctx: &mut Ctx, slug: &str, target_id: &str) {
    let req = ProjectRequest {
        slug: slug.into(),
        target_id: target_id.into(),
        mode: None,
        link_name: None,
    };
    let (plan, request) = project::plan(ctx, &req).unwrap();
    project::apply(ctx, &req, &plan, &request).unwrap();
}

#[test]
fn remove_retracts_every_link_and_parks_the_store_copy_in_the_trash() {
    let env = Env::new();
    write_skill(&env.home.join(".agents/skills/wechat-mp"), "wechat-mp");
    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let row = candidate(&ctx, "wechat-mp");
    let plan = adopt::plan(&ctx, &row).unwrap();
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal).unwrap();

    // Two clients both hold a link: removal must clear both, not just the one the user clicked.
    project(&mut ctx, "wechat-mp", "codex:global");
    project(&mut ctx, "wechat-mp", "claude:global");
    assert_eq!(
        projection::list_for_skill(ctx.db.conn(), "wechat-mp")
            .unwrap()
            .len(),
        2
    );

    let plan = remove::plan(&ctx, "wechat-mp").unwrap();
    assert!(plan.owned, "the content is a store copy");
    assert_eq!(plan.links.len(), 2);
    let trash = plan
        .trash
        .clone()
        .expect("store copy has a trash destination");

    let outcome = remove::apply(&mut ctx, &plan).unwrap();
    assert_eq!(outcome.links.len(), 2);
    assert_eq!(outcome.trash.as_deref(), Some(trash.as_str()));

    // Links are gone from disk and from the index.
    assert!(!env.home.join(".codex/skills/wechat-mp").exists());
    assert!(!env.home.join(".claude/skills/wechat-mp").exists());
    assert!(projection::list_for_skill(ctx.db.conn(), "wechat-mp")
        .unwrap()
        .is_empty());

    // The store copy moved aside instead of vanishing.
    assert!(!ctx.skill_path("wechat-mp").exists());
    assert!(Path::new(&trash).join("SKILL.md").is_file());

    // The library forgot it, and its source binding cascaded with it.
    assert!(skill::get(ctx.db.conn(), "wechat-mp").unwrap().is_none());
    let bindings: i64 = ctx
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM source_binding WHERE slug = 'wechat-mp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bindings, 0);
}

#[test]
fn remove_of_a_live_bound_skill_never_touches_the_users_directory() {
    let env = Env::new();
    let real = fs::canonicalize(&env.home)
        .unwrap()
        .join("personal/motion-design");
    write_skill(&real, "motion-design");
    fs::create_dir_all(env.home.join(".agents/skills")).unwrap();
    std::os::unix::fs::symlink(&real, env.home.join(".agents/skills/motion-design")).unwrap();

    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let mut row = candidate(&ctx, "motion-design");
    // Live-bind only applies to a row that still names the user's link: the scanner now resolves
    // the link to the real directory, so re-point the row the way a pre-change scan would have.
    row.path = env
        .home
        .join(".agents/skills/motion-design/SKILL.md")
        .to_string_lossy()
        .to_string();
    let plan = adopt::plan(&ctx, &row).unwrap();
    adopt::execute(&mut ctx, &plan, AdoptPlacement::LiveBind).unwrap();
    assert_eq!(
        ctx.content_path_of("motion-design"),
        real,
        "the library points at the user's own directory"
    );

    let plan = remove::plan(&ctx, "motion-design").unwrap();
    assert!(!plan.owned, "we only borrow that directory");
    assert!(plan.trash.is_none());

    remove::apply(&mut ctx, &plan).unwrap();
    assert!(skill::get(ctx.db.conn(), "motion-design")
        .unwrap()
        .is_none());
    assert!(
        real.join("SKILL.md").is_file(),
        "the user's content survives"
    );
    assert!(real.join("notes.md").is_file());
}

#[test]
fn remove_frees_the_slug_for_a_fresh_index_entry() {
    let env = Env::new();
    write_skill(&env.home.join(".agents/skills/wechat-mp"), "wechat-mp");
    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();
    let row = candidate(&ctx, "wechat-mp");
    let plan = adopt::plan(&ctx, &row).unwrap();
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal).unwrap();

    let plan = remove::plan(&ctx, "WECHAT-MP").unwrap();
    assert_eq!(
        plan.slug, "wechat-mp",
        "a differently-cased request finds the row"
    );
    remove::apply(&mut ctx, &plan).unwrap();

    // Re-registering under the same slug is what a reinstall does; the primary key must be free.
    let again = heron_store::models::SkillRow {
        slug: "wechat-mp".to_owned(),
        display_name: "WeChat MP".to_owned(),
        description: String::new(),
        tags: Vec::new(),
        status: "active".to_owned(),
        origin: "store".to_owned(),
        storage_path: None,
        subpath: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    ctx.db
        .write(|tx| skill::insert(tx, &again))
        .expect("slug is reusable after removal");
}

#[test]
fn remove_of_an_unknown_slug_reports_not_managed() {
    let env = Env::new();
    let ctx = env.ctx();
    let err = remove::plan(&ctx, "ghost").unwrap_err();
    assert_eq!(err.code(), "E_NOT_MANAGED");
}

#[test]
fn trash_can_be_listed_restored_and_emptied() {
    let env = Env::new();
    write_skill(&env.home.join(".agents/skills/wechat-mp"), "wechat-mp");
    write_skill(&env.home.join(".agents/skills/finesse-ui"), "finesse-ui");
    let mut ctx = env.ctx();
    scan::run(&mut ctx, &env.home, &ScanRequest::default()).unwrap();

    for slug in ["wechat-mp", "finesse-ui"] {
        let row = candidate(&ctx, slug);
        let plan = adopt::plan(&ctx, &row).unwrap();
        adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal).unwrap();
        let rm = remove::plan(&ctx, slug).unwrap();
        remove::apply(&mut ctx, &rm).unwrap();
    }

    let entries = remove::list_trash(&ctx).unwrap();
    assert_eq!(entries.len(), 2);

    // Restore one back into the library.
    let target = entries.iter().find(|e| e.slug == "wechat-mp").unwrap();
    let restored = remove::restore(&mut ctx, &target.id).unwrap();
    assert_eq!(restored.slug, "wechat-mp");
    assert!(ctx.skill_path("wechat-mp").join("SKILL.md").is_file());
    assert!(skill::get(ctx.db.conn(), "wechat-mp").unwrap().is_some());
    assert_eq!(remove::list_trash(&ctx).unwrap().len(), 1);

    // Empty the remainder.
    let emptied = remove::empty_trash(&mut ctx).unwrap();
    assert_eq!(emptied, 1);
    assert!(remove::list_trash(&ctx).unwrap().is_empty());
}
