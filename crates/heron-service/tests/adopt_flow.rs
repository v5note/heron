//! End-to-end: scan -> adopt -> project -> verify -> unproject, on a synthetic home.

use std::fs;
use std::path::{Path, PathBuf};

use heron_platform::dirs::Dirs;
use heron_service::adopt::{self, AdoptPlacement};
use heron_service::clients;
use heron_service::project::{self, ProjectRequest};
use heron_service::scan::{self, ScanRequest};
use heron_service::{Ctx, Result};
use heron_store::db::Db;
use heron_store::repos::{detected, projection};

struct Env {
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

    fn ctx(&self, actor: &'static str) -> Ctx {
        let dirs = Dirs::resolve_with_override("heron", Some(&self.data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        Ctx::new(dirs, db, actor)
    }

    fn write_skill(&self, rel: &str, name: &str) {
        let dir = self.home.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n"),
        )
        .unwrap();
        fs::write(dir.join("notes.md"), format!("notes for {name}")).unwrap();
    }

    fn scan(&self, ctx: &mut Ctx) -> scan::ScanReport {
        let req = ScanRequest::default();
        scan::run(ctx, &self.home, &req).unwrap()
    }
}

fn find_row(ctx: &Ctx, slug: &str) -> heron_store::models::DetectedRow {
    detected::list_by_state(ctx.db.conn(), "candidate")
        .unwrap()
        .into_iter()
        .find(|r| r.observed_slug.as_deref() == Some(slug))
        .expect("detected row")
}

#[test]
fn adopt_moves_content_into_store_and_links_the_original() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    let report = env.scan(&mut ctx);
    assert_eq!(report.skills, 1);

    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    assert_eq!(plan.slug.as_deref(), Some("pdf"));
    assert!(plan.duplicate_of.is_none());

    let result = adopt::execute(&mut ctx, &plan, AdoptPlacement::Replace)?;
    assert_eq!(result.adopted, vec!["pdf"]);

    // Store copy exists and is a git repo with metadata.
    let stored = ctx.skill_path("pdf");
    assert!(stored.join("SKILL.md").is_file());
    assert!(stored.join("heron.skill.json").is_file());
    assert!(stored.join(".git").is_dir());

    // Original location is now a link to the store.
    let original = env.home.join(".agents/skills/pdf");
    let info = heron_platform::link::probe(&original)?;
    assert!(info.kind.is_link(), "original must become a projection");
    assert!(original.join("SKILL.md").is_file());

    // A backup was taken before the move.
    let backups = ctx.dirs.backups();
    let has_backup = fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .any(|e| e.path().join("adopt-pdf").join("SKILL.md").is_file());
    assert!(has_backup, "adopt always backs up first");

    // The detected row is marked adopted.
    let state = detected::get(ctx.db.conn(), &row.path)?
        .unwrap()
        .adopt_state;
    assert_eq!(state, "adopted");
    Ok(())
}

#[test]
fn adopt_deduplicates_identical_content() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    env.write_skill(".claude/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);

    let rows = detected::list_by_state(ctx.db.conn(), "candidate")?;
    assert_eq!(rows.len(), 2);

    let first = &rows[0];
    let mut plan = adopt::plan(&ctx, first)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    let second = &rows[1];
    plan = adopt::plan(&ctx, second)?;
    assert!(
        plan.duplicate_of.is_some(),
        "identical content must be deduplicated, got {plan:?}"
    );
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    // Still exactly one managed skill.
    let skills = heron_store::repos::skill::list(ctx.db.conn(), false)?;
    assert_eq!(skills.len(), 1, "one store copy, not two");
    Ok(())
}

#[test]
fn adopt_retries_after_a_store_copy_was_left_behind() -> Result<()> {
    // A previous adopt can fail after copying into the store (disk full, git error, permission on
    // the original). The retry must repair that half-created directory, not fail on its branch.
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");

    // Simulate the leftover: content already in the store, index unaware of it.
    let dest = ctx.skill_path("pdf");
    heron_service::adopt::copy_tree(&env.home.join(".agents/skills/pdf"), &dest)?;
    let repo = heron_vcs::SkillRepo::open(&dest);
    repo.init(&heron_vcs::SkillMeta::new("pdf", "local", "", "0.0.0"))?;

    let plan = adopt::plan(&ctx, &row)?;
    let report = adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;
    assert_eq!(report.adopted, vec!["pdf".to_owned()]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    // Adopting a second time from the same leftover directory is still safe.
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;
    let state = detected::get(ctx.db.conn(), &row.path)?
        .unwrap()
        .adopt_state;
    assert_eq!(state, "adopted");
    Ok(())
}

#[test]
fn adopt_keep_original_leaves_the_file_in_place() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    let original = env.home.join(".agents/skills/pdf");
    assert!(
        !heron_platform::link::probe(&original)?.kind.is_link(),
        "original untouched"
    );
    assert!(original.join("SKILL.md").is_file());
    assert!(ctx.skill_path("pdf").join("SKILL.md").is_file());
    Ok(())
}

#[test]
fn project_needs_an_explicit_target_and_verify_reports_visibility() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    let req = ProjectRequest {
        slug: "pdf".into(),
        target_id: "codex:global".into(),
        mode: None,
        link_name: None,
    };
    let (plan, request) = project::plan(&ctx, &req)?;
    let outcome = project::apply(&mut ctx, &req, &plan, &request)?;
    assert!(outcome.applied);
    // Compare path components, not a literal string: Windows uses '\' as the separator.
    let expected_suffix = Path::new("skills").join("pdf");
    assert!(
        Path::new(&outcome.path).ends_with(&expected_suffix),
        "projected path: {}",
        outcome.path
    );

    let projected = env.home.join(".codex").join("skills").join("pdf");
    assert!(
        projected.join("SKILL.md").is_file(),
        "projected into the named client target"
    );
    assert_eq!(projection::list_for_skill(ctx.db.conn(), "pdf")?.len(), 1);
    assert!(
        heron_store::repos::target::get_default(ctx.db.conn())
            .unwrap()
            .is_none(),
        "the store is not a target, so nothing is a default one"
    );

    let reports = heron_service::verify::verify(&mut ctx, Some("pdf"))?;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].visibility, heron_projection::Visibility::Visible);

    project::unproject(&mut ctx, "pdf", "codex:global")?;
    assert!(!projected.exists());
    assert!(
        ctx.skill_path("pdf").join("SKILL.md").is_file(),
        "store copy survives unproject"
    );
    Ok(())
}

#[test]
fn project_refuses_to_replace_user_data() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    // Somebody else already owns the destination.
    let occupied = env.home.join(".codex/skills/pdf");
    fs::create_dir_all(&occupied)?;
    fs::write(occupied.join("user.md"), "precious")?;

    let req = ProjectRequest {
        slug: "pdf".into(),
        target_id: "codex:global".into(),
        mode: None,
        link_name: None,
    };
    let (plan, request) = project::plan(&ctx, &req)?;
    assert!(plan.is_blocked());
    let err = project::apply(&mut ctx, &req, &plan, &request).unwrap_err();
    assert_eq!(err.code(), "E_NOT_MANAGED");
    assert!(occupied.join("user.md").is_file(), "user data untouched");
    Ok(())
}

#[test]
fn doctor_reports_nested_suspects_without_touching_them() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/huashu/.agents/skills/inner", "inner");
    let mut ctx = env.ctx("cli");
    let report = env.scan(&mut ctx);
    assert_eq!(report.nested_suspects, 1);

    let before = heron_service::adopt::content_hash(
        &env.home.join(".agents/skills/huashu/.agents/skills/inner"),
    )?;
    let doctor = heron_service::doctor::run(&ctx)?;
    assert!(doctor.issues.iter().any(|i| i.id == "nested-suspect"));
    let after = heron_service::adopt::content_hash(
        &env.home.join(".agents/skills/huashu/.agents/skills/inner"),
    )?;
    assert_eq!(before, after, "decision D8: doctor never modifies the tree");
    Ok(())
}

#[test]
fn legacy_command_adopts_only_the_command_file() -> Result<()> {
    let env = Env::new();
    let cmd_dir = env.home.join(".claude/commands");
    fs::create_dir_all(&cmd_dir)?;
    fs::write(
        cmd_dir.join("review.md"),
        "---\nname: review\ndescription: r\n---\n",
    )?;
    fs::write(cmd_dir.join("unrelated.md"), "not a command")?;

    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "review");
    assert_eq!(row.kind, "legacy-command");

    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::Replace)?;

    // Only the single file becomes the skill; siblings never leak into the store.
    let stored = ctx.skill_path("review");
    assert!(stored.join("SKILL.md").is_file());
    assert!(
        !stored.join("unrelated.md").exists(),
        "the whole commands dir must not be copied"
    );
    Ok(())
}

#[test]
fn two_copies_of_one_skill_both_register_as_visible() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    env.write_skill(".claude/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);

    for row in detected::list_by_state(ctx.db.conn(), "candidate")? {
        let plan = adopt::plan(&ctx, &row)?;
        adopt::execute(&mut ctx, &plan, AdoptPlacement::Replace)?;
    }
    assert_eq!(
        heron_store::repos::skill::list(ctx.db.conn(), false)?.len(),
        1,
        "one store copy"
    );
    assert_eq!(
        projection::list_for_skill(ctx.db.conn(), "pdf")?.len(),
        2,
        "both originals are projections"
    );

    // First verify: claude only rescans at session start, so its fresh landing reads as
    // "restart required" (§12.3); the agents client is a watcher, so its copy is simply visible.
    for report in heron_service::verify::verify(&mut ctx, Some("pdf"))? {
        let in_claude = Path::new(&report.path)
            .components()
            .any(|c| c.as_os_str() == ".claude");
        assert_eq!(
            report.visibility,
            if in_claude {
                heron_projection::Visibility::RestartRequired
            } else {
                heron_projection::Visibility::Visible
            },
            "{} in {}",
            report.path,
            report.target_id
        );
    }
    // Second verify: the landing is confirmed (the user had a chance to reopen the client),
    // so both copies report plain visible.
    for report in heron_service::verify::verify(&mut ctx, Some("pdf"))? {
        assert_eq!(
            report.visibility,
            heron_projection::Visibility::Visible,
            "{} in {} should be visible",
            report.path,
            report.target_id
        );
    }
    Ok(())
}

#[test]
fn client_directories_are_available_as_targets() -> Result<()> {
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    let req = ProjectRequest {
        slug: "pdf".into(),
        target_id: "claude:global".into(),
        mode: None,
        link_name: None,
    };
    let (plan, request) = project::plan(&ctx, &req)?;
    let outcome = project::apply(&mut ctx, &req, &plan, &request)?;
    assert!(outcome.applied);
    assert!(env.home.join(".claude/skills/pdf/SKILL.md").is_file());
    Ok(())
}

#[test]
fn adopt_records_the_description_from_the_file() -> Result<()> {
    // The library row is what the table renders; an adopt that never read the file left every
    // 收拢 skill as a name with no summary.
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    env.scan(&mut ctx);
    let row = find_row(&ctx, "pdf");
    let plan = adopt::plan(&ctx, &row)?;
    adopt::execute(&mut ctx, &plan, AdoptPlacement::KeepOriginal)?;

    let skill = heron_store::repos::skill::get(ctx.db.conn(), "pdf")?.unwrap();
    assert_eq!(skill.description, "test");
    Ok(())
}

#[test]
fn repair_descriptions_fills_rows_adopted_before_that_fix() -> Result<()> {
    let env = Env::new();
    let mut ctx = env.ctx("cli");
    // The state an older app leaves behind: content in the store, index without a description.
    let dest = ctx.skill_path("pdf");
    fs::create_dir_all(&dest).unwrap();
    fs::write(
        dest.join("SKILL.md"),
        "---\nname: pdf\ndescription: test\n---\n",
    )
    .unwrap();
    ctx.db.write(|tx| {
        heron_store::repos::skill::insert(
            tx,
            &heron_store::models::SkillRow {
                slug: "pdf".into(),
                display_name: "pdf".into(),
                description: String::new(),
                tags: vec![],
                status: "active".into(),
                origin: "adopted".into(),
                storage_path: None,
                subpath: None,
                created_at: String::new(),
                updated_at: String::new(),
            },
        )
    })?;

    assert_eq!(adopt::repair_descriptions(&mut ctx)?, 1);
    assert_eq!(
        heron_store::repos::skill::get(ctx.db.conn(), "pdf")
            .unwrap()
            .unwrap()
            .description,
        "test"
    );
    // Idempotent, so it can ride along with every library read.
    assert_eq!(adopt::repair_descriptions(&mut ctx)?, 0);
    Ok(())
}

#[test]
fn scan_is_available_without_git_installed() {
    // The scan path must not depend on git; only adopt/project/upgrade do.
    let env = Env::new();
    env.write_skill(".agents/skills/pdf", "pdf");
    let mut ctx = env.ctx("cli");
    assert_eq!(env.scan(&mut ctx).skills, 1);
    let _ = clients::builtin().unwrap().len();
    let _: &Path = env.tmp.path();
}
