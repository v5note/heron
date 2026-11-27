//! Unit tests for the built-in client matrix, catalog sync, and target liveness.

use super::*;
use heron_store::models::{ProjectionRow, SkillRow};
use heron_store::repos::skill;

#[test]
fn builtin_matrix_parses_from_toml() {
    let matrix = builtin().unwrap();
    // Adding a client means adding a file; the two counts must not drift apart.
    assert_eq!(matrix.len(), BUILTIN_FILES.len());
    let claude = matrix.iter().find(|c| c.id == "claude").unwrap();
    assert_eq!(claude.global_skills, ".claude/skills"); // config values always use '/'
    assert_eq!(claude.global_commands.as_deref(), Some(".claude/commands"));
    assert_eq!(claude.verified_by.as_deref(), Some("doc"));
    assert_eq!(claude.rescan.as_deref(), Some("session"));
    // Only documented rescan behaviour is encoded; everyone else stays watch.
    for c in matrix
        .iter()
        .filter(|c| c.id != "claude" && c.id != "qoder")
    {
        assert!(c.rescan.is_none(), "{} must opt in explicitly", c.id);
    }
    assert!(
        !claude.native_scan.as_ref().unwrap().supported,
        "D6: no injection until verified"
    );
    for c in &matrix {
        assert!(c.link_modes.iter().any(|m| m == "symlink"));
        assert!(!c.priority.eq(&-1));
        // A client with no rules at all would render "unknown" forever, which is the same as
        // not shipping it: every entry must be able to answer the question.
        let rules = c
            .detect
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no [detect]", c.id));
        assert!(
            !rules.dirs.is_empty() || !rules.apps.is_empty() || !rules.commands.is_empty(),
            "{} declares an empty [detect]",
            c.id
        );
        for cmd in &rules.commands {
            assert!(
                !cmd.bin.is_empty(),
                "{} has a command probe with no bin",
                c.id
            );
        }
    }
}

#[test]
fn bootstrap_is_idempotent_and_leaves_no_default_target() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = Path::new("/home/u");
    bootstrap(&mut db, home).unwrap();
    bootstrap(&mut db, home).unwrap();

    let want = builtin().unwrap().len();
    let clients = client::list(db.conn()).unwrap();
    assert_eq!(clients.len(), want, "only the built-in client matrix");
    assert!(!clients.iter().any(|c| c.id == "heron"));
    let targets = target::list(db.conn()).unwrap();
    assert_eq!(targets.len(), want, "one global target per built-in client");
    assert!(targets.iter().any(|t| t.id == "claude:global"));
    assert!(
        !targets.iter().any(|t| t.id == "heron:global"),
        "the store is not a projection target"
    );
    assert!(
        target::get_default(db.conn()).unwrap().is_none(),
        "nothing is projected without an explicit --on"
    );
    assert_eq!(targets.iter().filter(|t| t.is_default).count(), 0);
    // Bootstrapping tells us nothing about the machine.
    assert!(
        clients.iter().all(|c| c.detected.is_none()),
        "no verdict before the first detection"
    );
}

#[test]
fn sync_catalog_adds_rows_without_a_second_bootstrap() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = Path::new("/home/u");
    bootstrap(&mut db, home).unwrap();

    // Simulate an upgrade that gained a client: its rows are simply not there yet.
    db.write(|tx| {
        tx.conn()
            .execute("DELETE FROM target WHERE id = 'trae:global'", [])
            .map_err(heron_store::sql_err)?;
        tx.conn()
            .execute("DELETE FROM client WHERE id = 'trae'", [])
            .map_err(heron_store::sql_err)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        target::list(db.conn()).unwrap().len(),
        builtin().unwrap().len() - 1
    );

    sync_catalog(&mut db, home, false).unwrap();
    sync_catalog(&mut db, home, false).unwrap();

    assert!(client::get(db.conn(), "trae").unwrap().is_some());
    let targets = target::list(db.conn()).unwrap();
    let trae = targets.iter().find(|t| t.id == "trae:global").unwrap();
    assert_eq!(trae.kind, "trae");
    assert_eq!(
        targets.len(),
        builtin().unwrap().len(),
        "a repeat sync adds nothing"
    );
}

#[test]
fn neither_write_path_can_reset_the_other() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = Path::new("/home/u");
    bootstrap(&mut db, home).unwrap();

    // A renamed display name (as if the matrix were edited) and a negative verdict.
    db.write(|tx| {
        tx.conn()
            .execute(
                "UPDATE client SET display_name = 'Claude (local)' WHERE id = 'claude'",
                [],
            )
            .map_err(heron_store::sql_err)?;
        client::set_presence(tx, "claude", Some(false), "2026-11-23T00:00:00.000Z", &[])?;
        Ok(())
    })
    .unwrap();

    // The insert-only path rewrites nothing, in either direction.
    sync_catalog(&mut db, home, false).unwrap();
    let kept = client::get(db.conn(), "claude").unwrap().unwrap();
    assert_eq!(kept.display_name, "Claude (local)");
    assert_eq!(kept.detected, Some(0));

    // A second bootstrap — the force path — restores the catalogue and still cannot touch the
    // verdict, because `upsert` does not name the presence columns.
    sync_catalog(&mut db, home, true).unwrap();
    let after = client::get(db.conn(), "claude").unwrap().unwrap();
    let matrix_name = builtin()
        .unwrap()
        .into_iter()
        .find(|c| c.id == "claude")
        .unwrap()
        .display_name;
    assert_eq!(
        after.display_name, matrix_name,
        "catalogue is authoritative"
    );
    assert_eq!(
        after.detected,
        Some(0),
        "a catalogue write cannot invent a verdict"
    );
    assert_eq!(
        after.detected_at.as_deref(),
        Some("2026-11-23T00:00:00.000Z"),
        "nor overwrite the measurement"
    );
    assert!(after.detect_evidence.is_empty());

    // A verdict written for an unknown id changes no row.
    db.write(|tx| {
        assert_eq!(
            0,
            client::set_presence(
                tx,
                "no-such-client",
                Some(true),
                "2026-11-23T00:00:00.000Z",
                &[]
            )?
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn presence_and_the_user_switch_are_orthogonal() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = Path::new("/home/u");
    bootstrap(&mut db, home).unwrap();
    let clients = client::list(db.conn()).unwrap();
    let target = target::get(db.conn(), "claude:global").unwrap().unwrap();

    // Never measured: exactly as before presence existed.
    assert!(target_live(&target, &clients));

    let verdict = |db: &mut Db, v: Option<bool>| {
        db.write(|tx| {
            client::set_presence(tx, "claude", v, "2026-11-23T00:00:00.000Z", &[])?;
            Ok(())
        })
        .unwrap();
        client::list(db.conn()).unwrap()
    };
    assert!(
        target_live(&target, &verdict(&mut db, Some(true))),
        "installed and on"
    );
    assert!(
        !target_live(&target, &verdict(&mut db, Some(false))),
        "not on this machine"
    );
    // Absent is not "off": the row and the user's switch both survive.
    let still = target::get(db.conn(), "claude:global").unwrap().unwrap();
    assert!(still.enabled);
    assert_eq!(
        client::get(db.conn(), "claude").unwrap().unwrap().detected,
        Some(0)
    );
}

/// The shape adopt used to write: a `manual` target for a directory no client reads.
fn phantom(db: &mut Db, id: &str, dir: &Path, link: Option<&Path>) {
    let target_row = TargetRow {
        id: id.to_owned(),
        kind: "agents".to_owned(),
        scope: "global".to_owned(),
        project_path: None,
        path: dir.to_string_lossy().to_string(),
        enabled: true,
        is_default: false,
        discover: "manual".to_owned(),
    };
    db.write(|tx| {
        if skill::get(tx.conn(), "huashu-design")?.is_none() {
            skill::insert(
                tx,
                &SkillRow {
                    slug: "huashu-design".to_owned(),
                    display_name: "huashu-design".to_owned(),
                    description: String::new(),
                    tags: vec![],
                    status: "active".to_owned(),
                    origin: "adopted".to_owned(),
                    storage_path: None,
                    subpath: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )?;
        }
        target::upsert(tx, &target_row)?;
        if let Some(link) = link {
            projection::insert(
                tx,
                &ProjectionRow {
                    slug: "huashu-design".to_owned(),
                    target_id: id.to_owned(),
                    link_name: "huashu-design".to_owned(),
                    path: link.to_string_lossy().to_string(),
                    mode: "symlink".to_owned(),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_manual_target_with_nothing_left_on_disk_is_forgotten() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    bootstrap(&mut db, &tmp.path().join("home")).unwrap();
    let catalogue = target::list(db.conn()).unwrap().len();

    // The third-party checkout is gone: neither the skills dir nor the link inside it exists.
    let vanished = tmp.path().join("huashu-motion/skills");
    phantom(
        &mut db,
        "agents:phantom",
        &vanished,
        Some(&vanished.join("huashu-design")),
    );
    assert_eq!(target::list(db.conn()).unwrap().len(), catalogue + 1);

    assert_eq!(prune_phantom_targets(&mut db).unwrap(), 1);
    assert!(target::get(db.conn(), "agents:phantom").unwrap().is_none());
    assert!(
        projection::list_all(db.conn())
            .unwrap()
            .iter()
            .all(|p| p.target_id != "agents:phantom"),
        "the broken link went with the landing point that made it"
    );
    // `sync_catalog` rows are the matrix's, never a phantom's collateral.
    assert_eq!(target::list(db.conn()).unwrap().len(), catalogue);
    // Idempotent: a second scan finds nothing to do.
    assert_eq!(prune_phantom_targets(&mut db).unwrap(), 0);
}

#[test]
fn pruning_only_forgets_rows_adopt_wrote_and_only_when_the_disk_agrees() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".agents/skills")).unwrap();
    bootstrap(&mut db, &home).unwrap();
    let want = builtin().unwrap().len();

    // A live-bound directory that is still there.
    let present = tmp.path().join("my-workspace/skills");
    std::fs::create_dir_all(&present).unwrap();
    phantom(&mut db, "agents:present", &present, None);

    // Gone as a landing point, but the link recorded against it is still an entry on disk:
    // that is the user's broken link to repair, not a row to erase.
    let gone_dir = tmp.path().join("third-party/skills");
    let kept_link = tmp.path().join("elsewhere/huashu-design");
    std::fs::create_dir_all(kept_link.parent().unwrap()).unwrap();
    std::fs::write(&kept_link, "SKILL.md").unwrap();
    phantom(&mut db, "agents:gone-dir", &gone_dir, Some(&kept_link));
    assert_eq!(target::list(db.conn()).unwrap().len(), want + 2);

    assert_eq!(prune_phantom_targets(&mut db).unwrap(), 0);
    assert!(target::get(db.conn(), "agents:present").unwrap().is_some());
    assert!(target::get(db.conn(), "agents:gone-dir").unwrap().is_some());
    assert!(
        target::get(db.conn(), "agents:global").unwrap().is_some(),
        "the catalogue's own landing point is never a phantom"
    );
    assert!(
        std::fs::symlink_metadata(&present).is_ok(),
        "no file was touched"
    );
}

/// The shape the old project walk wrote: home itself registered as a project root, so a second
/// landing point on the very path the catalogue already owns.
fn home_project(db: &mut Db, home: &Path, kind: &str, projection: Option<&Path>) {
    let id = format!("{kind}:project:af63f34c");
    let row = TargetRow {
        kind: kind.to_owned(),
        scope: "project".to_owned(),
        project_path: Some(home.to_string_lossy().to_string()),
        path: home.join(".claude/skills").to_string_lossy().to_string(),
        enabled: false,
        is_default: false,
        discover: "auto".to_owned(),
        id: id.clone(),
    };
    db.write(|tx| {
        target::upsert(tx, &row)?;
        if let Some(link) = projection {
            skill::insert(
                tx,
                &SkillRow {
                    slug: "huashu-design".to_owned(),
                    display_name: "huashu-design".to_owned(),
                    description: String::new(),
                    tags: vec![],
                    status: "active".to_owned(),
                    origin: "adopted".to_owned(),
                    storage_path: None,
                    subpath: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )?;
            projection::insert(
                tx,
                &ProjectionRow {
                    slug: "huashu-design".to_owned(),
                    target_id: id.clone(),
                    link_name: "huashu-design".to_owned(),
                    path: link.to_string_lossy().to_string(),
                    mode: "symlink".to_owned(),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn opening_the_index_forgets_a_home_that_was_registered_as_a_project() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
    bootstrap(&mut db, &home).unwrap();
    let catalogue = target::list(db.conn()).unwrap().len();

    home_project(&mut db, &home, "claude", None);
    assert_eq!(target::list(db.conn()).unwrap().len(), catalogue + 1);

    // The repair runs on every entry point, not only after the user presses 扫描.
    bootstrap(&mut db, &home).unwrap();
    assert_eq!(target::list(db.conn()).unwrap().len(), catalogue);
    assert!(
        target::get(db.conn(), "claude:project:af63f34c")
            .unwrap()
            .is_none(),
        "the duplicate card is gone"
    );
    assert!(
        target::get(db.conn(), "claude:global").unwrap().is_some(),
        "the catalogue's own landing point survives with the user's switch"
    );
}

#[test]
fn a_real_project_and_a_project_that_owns_links_are_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&tmp.path().join("heron.db")).unwrap();
    let home = tmp.path().join("home");
    let project = tmp.path().join("work/proj-a");
    std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
    bootstrap(&mut db, &home).unwrap();

    // A project under home is what discovery really means; it must not be swept up.
    let real = TargetRow {
        id: "claude:project:1d9b0a".to_owned(),
        kind: "claude".to_owned(),
        scope: "project".to_owned(),
        project_path: Some(project.to_string_lossy().to_string()),
        path: project.join(".claude/skills").to_string_lossy().to_string(),
        enabled: false,
        is_default: false,
        discover: "auto".to_owned(),
    };
    db.write(|tx| target::upsert(tx, &real)).unwrap();

    // Home again, but something is still recorded under it: deleting the target would take
    // those links with it, so the row stays for the user to decide.
    let link = home.join(".claude/skills/huashu-design");
    home_project(&mut db, &home, "codex", Some(&link));

    bootstrap(&mut db, &home).unwrap();
    let ids: Vec<_> = target::list(db.conn())
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert!(ids.contains(&real.id));
    assert!(ids.contains(&"codex:project:af63f34c".to_owned()));
}
