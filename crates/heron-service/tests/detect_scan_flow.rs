//! Presence detection as it actually runs: inside `scan::prepare`, against a synthetic machine.
//!
//! The invariant this file exists to keep is one sentence long — **a verdict of "not installed"
//! never takes a directory away from the scanner**. Detection changes what the overview grid says
//! about a client; it does not change what Heron is willing to look at, because a client that
//! creates `~/.cursor/skills` tomorrow must be found by the scan that runs tomorrow without the
//! user having to re-enable anything first.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use heron_platform::dirs::Dirs;
use heron_service::clients;
use heron_service::detect::Machine;
use heron_service::scan::{self, ScanRequest};
use heron_store::db::Db;
use heron_store::repos::{client, target};

/// A machine with exactly one client on it: `home/.codex` exists, and nothing else does — no
/// bundle, no binary on `PATH`.
struct OnlyCodex {
    home: PathBuf,
}

impl Machine for OnlyCodex {
    fn is_dir(&self, path: &Path) -> bool {
        path == self.home.join(".codex")
    }

    fn app_exists(&self, _name: &str) -> bool {
        false
    }

    fn cli_version(&self, _bin: &str, _args: &[&str], _timeout: Duration) -> Option<String> {
        None
    }

    fn now(&self) -> String {
        "2026-11-23T00:00:00.000Z".to_owned()
    }
}

fn ctx(data: &Path) -> heron_service::Ctx {
    let dirs = Dirs::resolve_with_override("heron", Some(data)).unwrap();
    dirs.ensure().unwrap();
    let db = Db::open(&dirs.db_path()).unwrap();
    heron_service::Ctx::new(dirs, db, "cli")
}

#[test]
fn one_measured_client_never_narrows_the_walk() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let data = tmp.path().join("data");
    fs::create_dir_all(&home).unwrap();

    let mut ctx = ctx(&data);
    let machine = OnlyCodex { home: home.clone() };
    let plan = scan::prepare_with(&mut ctx, &home, &ScanRequest::default(), &machine).unwrap();

    // (a) The roots are the whole catalogue, not the detected subset.
    let visits = |rel: &str| {
        plan.roots()
            .iter()
            .any(|r| r.as_path() == home.join(rel).as_path())
    };
    assert!(visits(".codex/skills"), "the one real client is scanned");
    for rel in [
        ".cursor/skills",
        ".trae/skills",
        ".agents/skills",
        ".config/opencode/skills",
    ] {
        assert!(
            visits(rel),
            "{rel} belongs to an undetected client and must still be walked"
        );
    }

    // (b) Every catalogue client has a row, with a real verdict on it.
    let rows = client::list(ctx.db.conn()).unwrap();
    let matrix = clients::builtin().unwrap();
    assert_eq!(
        rows.len(),
        matrix.len(),
        "detection reports on rows, it does not prune them"
    );
    let row_of = |id: &str| rows.iter().find(|c| c.id == id).expect("client row");
    assert_eq!(row_of("codex").detected, Some(1));
    assert_eq!(
        row_of("codex").detect_evidence,
        vec!["dir:~/.codex".to_owned()],
        "the verdict says what it looked at"
    );
    assert_eq!(
        row_of("codex").detected_at.as_deref(),
        Some("2026-11-23T00:00:00.000Z")
    );
    assert_eq!(row_of("cursor").detected, Some(0));
    assert!(
        row_of("cursor").detect_evidence.is_empty(),
        "a miss has nothing honest to show"
    );

    // (c) The user switch and the measurement stay orthogonal: the target row exists either way,
    // only `live` differs.
    let targets = target::list(ctx.db.conn()).unwrap();
    let global = |id: &str| {
        targets
            .iter()
            .find(|t| t.kind == id && t.scope == "global")
            .expect("global target row")
    };
    assert!(global("codex").enabled, "bootstrap enabled it as before");
    assert!(global("cursor").enabled, "an absent client is not disabled");
    assert!(clients::target_live(global("codex"), &rows));
    assert!(
        !clients::target_live(global("cursor"), &rows),
        "absent means greyed out, not deleted"
    );
}

/// Repeated scans must not accumulate rows, and a client that only appeared in a newer binary's
/// catalogue is measured by the very next scan rather than waiting for the one after that.
#[test]
fn detection_survives_a_repeated_scan_and_an_upgrade() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let data = tmp.path().join("data");
    fs::create_dir_all(&home).unwrap();

    let mut ctx = ctx(&data);
    let machine = OnlyCodex { home: home.clone() };
    scan::prepare_with(&mut ctx, &home, &ScanRequest::default(), &machine).unwrap();
    scan::prepare_with(&mut ctx, &home, &ScanRequest::default(), &machine).unwrap();

    let rows = client::list(ctx.db.conn()).unwrap();
    let matrix = clients::builtin().unwrap();
    assert_eq!(rows.len(), matrix.len());
    assert_eq!(
        rows.iter().filter(|c| c.id == "codex").count(),
        1,
        "the catalogue is upserted, never appended to"
    );

    // Simulate the upgrade that added a client: its rows are simply not there yet.
    ctx.db
        .write(|tx| {
            tx.conn()
                .execute("DELETE FROM target WHERE id = 'trae:global'", [])
                .map_err(heron_store::sql_err)?;
            tx.conn()
                .execute("DELETE FROM client WHERE id = 'trae'", [])
                .map_err(heron_store::sql_err)?;
            Ok(())
        })
        .unwrap();
    let plan = scan::prepare_with(&mut ctx, &home, &ScanRequest::default(), &machine).unwrap();

    let rows = client::list(ctx.db.conn()).unwrap();
    assert_eq!(rows.len(), matrix.len(), "the new client was filled in");
    assert_eq!(
        rows.iter()
            .find(|c| c.id == "trae")
            .expect("trae row")
            .detected,
        Some(0),
        "…and measured in the same pass that created it"
    );
    assert!(
        plan.roots()
            .iter()
            .any(|r| r.as_path() == home.join(".trae/skills").as_path()),
        "and the walk gets the directory immediately"
    );
}

/// The library projects into the very directories the scanner walks, so a taken-over skill would
/// otherwise come back as a discovery. It must not: neither in a fresh index, nor in rows an earlier
/// scan left behind.
#[test]
fn a_projection_is_not_offered_as_a_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let data = tmp.path().join("data");
    let skills = home.join(".codex/skills");
    for name in ["original", "mine"] {
        fs::create_dir_all(skills.join(name)).unwrap();
        fs::write(
            skills.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\n\nbody\n"),
        )
        .unwrap();
    }
    let original_md = skills
        .join("original")
        .join("SKILL.md")
        .to_string_lossy()
        .into_owned();

    let mut ctx = ctx(&data);
    let machine = OnlyCodex { home: home.clone() };
    let rescan = |ctx: &mut heron_service::Ctx| {
        let plan = scan::prepare_with(ctx, &home, &ScanRequest::default(), &machine).unwrap();
        let result = scan::walk(&plan, &mut heron_scan::walker::ScanMonitor::quiet()).unwrap();
        scan::persist(ctx, &result, true).unwrap();
    };
    let candidates = |ctx: &heron_service::Ctx| -> Vec<String> {
        heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate")
            .unwrap()
            .into_iter()
            .map(|row| row.path)
            .collect()
    };

    rescan(&mut ctx);
    assert_eq!(candidates(&ctx).len(), 2, "both are skills on disk");

    // What the library's own projection writes into that directory: a skill, a target, and a
    // projection row naming the destination. The same trio `project::apply` leaves behind.
    ctx.db
        .write(|tx| {
            heron_store::repos::target::upsert(
                tx,
                &heron_store::models::TargetRow {
                    id: "codex:global".into(),
                    kind: "codex".into(),
                    scope: "global".into(),
                    project_path: None,
                    path: skills.to_string_lossy().into_owned(),
                    enabled: true,
                    is_default: true,
                    discover: "auto".into(),
                },
            )?;
            heron_store::repos::skill::insert(
                tx,
                &heron_store::models::SkillRow {
                    slug: "mine".into(),
                    display_name: "mine".into(),
                    description: String::new(),
                    tags: Vec::new(),
                    status: "active".into(),
                    origin: "store".into(),
                    storage_path: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )?;
            heron_store::repos::projection::insert(
                tx,
                &heron_store::models::ProjectionRow {
                    id: 0,
                    slug: "mine".into(),
                    target_id: "codex:global".into(),
                    link_name: "mine".into(),
                    path: skills.join("mine").to_string_lossy().into_owned(),
                    mode: "symlink".into(),
                    version: None,
                    content_hash: None,
                    inode_kind: None,
                    state: String::new(),
                    created_at: String::new(),
                },
            )
            .map(|_| ())
        })
        .unwrap();

    // The rows the index already holds stop being discoveries at once, without a new walk.
    let stale = heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate").unwrap();
    let listed = scan::without_projection_outputs(&ctx, stale).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>(),
        vec![original_md.as_str()],
        "the projected copy is hidden, the user's own file is not"
    );

    // And the next walk stops indexing it at all, so the stale row is pruned.
    rescan(&mut ctx);
    assert_eq!(
        candidates(&ctx),
        vec![original_md.clone()],
        "a projection is never rediscovered"
    );
}

/// The machine that motivated the `trae-cn` entry: the CN build installed on its own, with no trace
/// of the international Trae. The two entries are one keystroke apart in the catalogue, and a probe
/// that confused them would either grey out a real target or offer to write into a directory that no
/// application reads.
struct OnlyTraeCn {
    home: PathBuf,
}

impl Machine for OnlyTraeCn {
    fn is_dir(&self, path: &Path) -> bool {
        path == self.home.join(".trae-cn")
    }

    fn app_exists(&self, name: &str) -> bool {
        // By exact bundle name, as `probe::app_exists` joins it onto /Applications.
        name == "Trae CN.app"
    }

    fn cli_version(&self, _bin: &str, _args: &[&str], _timeout: Duration) -> Option<String> {
        None
    }

    fn now(&self) -> String {
        "2026-11-24T00:00:00.000Z".to_owned()
    }
}

#[test]
fn the_two_trae_builds_do_not_answer_for_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let data = tmp.path().join("data");
    fs::create_dir_all(&home).unwrap();

    let mut ctx = ctx(&data);
    let machine = OnlyTraeCn { home: home.clone() };
    let plan = scan::prepare_with(&mut ctx, &home, &ScanRequest::default(), &machine).unwrap();

    let rows = client::list(ctx.db.conn()).unwrap();
    let row_of = |id: &str| rows.iter().find(|c| c.id == id).expect("client row");
    assert_eq!(row_of("trae-cn").detected, Some(1));
    // `ClientToml` has no `deny_unknown_fields`, so a mistyped key in the TOML parses "fine" and
    // quietly yields an empty default. These two would otherwise only surface as a target that can
    // never be projected into, or a client that sorts to the top of every list for no reason.
    assert_eq!(
        row_of("trae-cn").link_modes,
        vec![
            "symlink".to_owned(),
            "junction".to_owned(),
            "copy".to_owned()
        ],
        "the landing modes actually landed"
    );
    assert_eq!(row_of("trae-cn").priority, 65, "sorted beside `trae`");
    assert_eq!(
        row_of("trae-cn").detect_evidence,
        vec!["dir:~/.trae-cn".to_owned()],
        "the CN build is found by its own home directory"
    );
    assert_eq!(
        row_of("trae").detected,
        Some(0),
        "the international build stays absent: ~/.trae-cn is a sibling of ~/.trae, not under it"
    );

    // Neither verdict narrows the walk, and the two locations are distinct directories.
    let visits = |rel: &str| {
        plan.roots()
            .iter()
            .any(|r| r.as_path() == home.join(rel).as_path())
    };
    assert!(visits(".trae-cn/skills"), "the detected build is scanned");
    assert!(visits(".trae/skills"), "the absent build is scanned anyway");

    let targets = target::list(ctx.db.conn()).unwrap();
    let global = |id: &str| {
        targets
            .iter()
            .find(|t| t.kind == id && t.scope == "global")
            .expect("global target row")
    };
    assert!(
        clients::target_live(global("trae-cn"), &rows),
        "…and the CN target is a live landing place"
    );
    assert!(!clients::target_live(global("trae"), &rows));
}
