//! Integration tests for the index: migrations, repositories, transactions.

use heron_core::ids::BUILTIN_MARKETS;
use heron_store::db::{Db, SCHEMA_VERSION};
use heron_store::models::{DetectedUpsert, ProjectionRow, SourceRow, TargetRow};
use heron_store::repos::{detected, event, projection, source, target};
use std::collections::HashSet;

fn temp_db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("heron.db")).unwrap();
    (dir, db)
}

fn detected_row(path: &str, slug: &str) -> DetectedUpsert {
    DetectedUpsert {
        path: path.to_owned(),
        real_path: None,
        client_id: Some("claude".to_owned()),
        scope: Some("global".to_owned()),
        project_path: None,
        observed_slug: Some(slug.to_owned()),
        content_hash: Some(format!("blake3:{slug}")),
        frontmatter_ok: Some(true),
        kind: "skill".to_owned(),
        mtime_ns: Some(1_700_000_000_000_000_000),
        size_bytes: Some(1234),
    }
}

fn seed_client(tx: &heron_store::WriteTx<'_>, id: &str) -> heron_core::Result<()> {
    heron_store::repos::client::upsert(
        tx,
        &heron_store::models::ClientRow {
            id: id.to_owned(),
            display_name: id.to_owned(),
            global_paths: vec![],
            project_paths: vec![],
            link_modes: vec!["symlink".to_owned()],
            global_commands: vec![],
            project_commands: vec![],
            priority: 0,
            native_scan: None,
            verified_at: None,
            verified_by: None,
        },
    )
}

fn target_row(id: &str, path: &str, is_default: bool) -> TargetRow {
    TargetRow {
        id: id.to_owned(),
        kind: "heron".to_owned(),
        scope: "global".to_owned(),
        project_path: None,
        path: path.to_owned(),
        enabled: true,
        is_default,
        discover: "manual".to_owned(),
    }
}

#[test]
fn migration_creates_schema_at_expected_version() {
    let (_dir, db) = temp_db();
    let version: i64 = db
        .conn()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(version, 2, "0001_init + 0002_client_commands");
    assert!(db.integrity_ok());
}

#[test]
fn migration_is_idempotent_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("heron.db");
    {
        let db = Db::open(&path).unwrap();
        db.kv_set("k", "v").unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.kv_get("k").unwrap().as_deref(), Some("v"));
    assert!(db.integrity_ok());
}

#[test]
fn migration_upgrades_a_v1_database_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("heron.db");
    // Build a v1 database by hand (only 0001), then let Db::open migrate it.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../src/migrations/0001_init.sql"))
            .unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();
        conn.execute(
            "INSERT INTO client (id, display_name, global_paths, project_paths, link_modes)
             VALUES ('claude', 'Claude', '[]', '[]', '[]')",
            [],
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let rows = heron_store::repos::client::list(db.conn()).unwrap();
    assert_eq!(rows.len(), 1, "existing data survives the upgrade");
    assert!(
        rows[0].global_commands.is_empty(),
        "new columns get defaults"
    );
}

#[test]
fn schema_rejects_newer_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("heron.db");
    {
        let db = Db::open(&path).unwrap();
        db.conn()
            .pragma_update(None, "user_version", 99i64)
            .unwrap();
    }
    let err = Db::open(&path).unwrap_err();
    match err {
        heron_store::HeronError::Msg(m) => assert_eq!(m.code, "E_DB_CORRUPT"),
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn write_transaction_rolls_back_on_error() {
    let (_dir, mut db) = temp_db();
    let result: heron_core::Result<()> = db.write(|tx| {
        source::upsert(
            tx,
            &SourceRow {
                id: "s1".into(),
                name: "S1".into(),
                kind: "git".into(),
                adapter: None,
                url: Some("https://example.com/s.git".into()),
                ref_name: None,
                subpath: None,
                path: None,
                priority: 1,
                trusted: false,
                enabled: true,
                capability: None,
                last_synced_at: None,
                last_commit: None,
                last_probe_ok: None,
            },
        )?;
        Err(heron_core::ErrorCode::Internal.msg().into())
    });
    assert!(result.is_err());
    assert!(
        source::list(db.conn()).unwrap().is_empty(),
        "row must be rolled back"
    );
}

#[test]
fn default_target_is_the_heron_landing() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        seed_client(tx, "heron")?;
        seed_client(tx, "claude")?;
        target::upsert(
            tx,
            &target_row("heron:global", "/home/x/.heron/skills", true),
        )?;
        target::upsert(
            tx,
            &target_row("claude:global", "/home/x/.claude/skills", false),
        )?;
        Ok(())
    })
    .unwrap();

    let default = target::get_default(db.conn()).unwrap().unwrap();
    assert_eq!(default.id, "heron:global");
    assert!(default.path.ends_with(".heron/skills"));

    db.write(|tx| target::set_default(tx, "claude:global"))
        .unwrap();
    assert_eq!(
        target::get_default(db.conn()).unwrap().unwrap().id,
        "claude:global"
    );
}

#[test]
fn projections_require_existing_target_and_cascade() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        seed_client(tx, "heron")?;
        target::upsert(tx, &target_row("heron:global", "/data/.heron/skills", true))?;
        heron_store::repos::skill::insert(
            tx,
            &heron_store::models::SkillRow {
                slug: "pdf".into(),
                display_name: "pdf".into(),
                description: String::new(),
                tags: vec![],
                status: "active".into(),
                origin: "store".into(),
                created_at: String::new(),
                updated_at: String::new(),
            },
        )?;
        projection::insert(
            tx,
            &ProjectionRow {
                slug: "pdf".into(),
                target_id: "heron:global".into(),
                link_name: "pdf".into(),
                path: "/data/.heron/skills/pdf".into(),
                mode: "symlink".into(),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();

    let rows = projection::list_for_skill(db.conn(), "pdf").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mode, "symlink");
    assert_eq!(rows[0].state, "ok");

    db.write(|tx| target::remove(tx, "heron:global")).unwrap();
    assert!(projection::list_all(db.conn()).unwrap().is_empty());
}

#[test]
fn projection_foreign_key_rejects_unknown_skill() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        seed_client(tx, "heron")?;
        target::upsert(tx, &target_row("heron:global", "/p", true))?;
        Ok(())
    })
    .unwrap();
    let res: heron_core::Result<i64> = db.write(|tx| {
        projection::insert(
            tx,
            &ProjectionRow {
                slug: "ghost".into(),
                target_id: "heron:global".into(),
                link_name: "ghost".into(),
                path: "/p/ghost".into(),
                mode: "copy".into(),
                ..Default::default()
            },
        )
    });
    assert!(res.is_err());
}

#[test]
fn scan_upsert_and_prune_keeps_ignored_rows() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        for (path, slug) in [("/a/pdf", "pdf"), ("/a/tdd", "tdd"), ("/a/old", "old")] {
            detected::upsert(tx, &detected_row(path, slug))?;
        }
        Ok(())
    })
    .unwrap();
    db.write(|tx| detected::set_state(tx, "/a/old", "ignored"))
        .unwrap();
    db.write(|tx| detected::set_state(tx, "/a/tdd", "adopted"))
        .unwrap();

    let seen: HashSet<String> = ["/a/pdf", "/a/old"].iter().map(|s| s.to_string()).collect();
    let removed = db.write(|tx| detected::prune_not_in(tx, &seen)).unwrap();
    assert_eq!(removed, 1, "only the adopted-but-now-absent row is pruned");

    let all = detected::list_all(db.conn()).unwrap();
    let paths: Vec<_> = all.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["/a/old", "/a/pdf"]);
    assert_eq!(
        all[0].adopt_state, "ignored",
        "the user's ignore decision survives"
    );
}

#[test]
fn upsert_is_idempotent_for_same_path() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| detected::upsert(tx, &detected_row("/a/pdf", "pdf")))
        .unwrap();
    let mut again = detected_row("/a/pdf", "pdf-v2");
    again.content_hash = Some("blake3:changed".into());
    db.write(|tx| detected::upsert(tx, &again)).unwrap();

    let all = detected::list_all(db.conn()).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].observed_slug.as_deref(), Some("pdf-v2"));
    assert_eq!(all[0].content_hash.as_deref(), Some("blake3:changed"));
}

#[test]
fn count_by_kind_reports_legacy_commands_and_nested_suspects() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        for (path, kind) in [
            ("/a/pdf", "skill"),
            ("/a/tdd", "skill"),
            ("/c/.claude/commands/review.md", "legacy-command"),
            ("/a/skills/.agents/skills/x", "nested-suspect"),
        ] {
            let mut row = detected_row(path, "x");
            row.kind = kind.to_owned();
            detected::upsert(tx, &row)?;
        }
        Ok(())
    })
    .unwrap();

    let counts = detected::count_by_kind(db.conn()).unwrap();
    let get = |k: &str| {
        counts
            .iter()
            .find(|(kind, _)| kind == k)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };
    assert_eq!(get("skill"), 2);
    assert_eq!(get("legacy-command"), 1);
    assert_eq!(get("nested-suspect"), 1);
}

#[test]
fn events_record_undo_steps_and_pending() {
    let (_dir, mut db) = temp_db();
    let id = db
        .write(|tx| {
            let id = event::record(tx, "cli", "adopt", Some("pdf"), Some("{\"n\":1}"), true)?;
            event::push_undo_step(tx, id, 0, "{\"kind\":\"created_link\"}")?;
            event::push_undo_step(tx, id, 1, "{\"kind\":\"moved_dir\"}")?;
            Ok(id)
        })
        .unwrap();

    let steps = event::undo_steps(db.conn(), id).unwrap();
    assert_eq!(steps.len(), 2);
    assert!(
        steps[0].contains("moved_dir"),
        "undo steps are read newest-first"
    );

    let pending = event::pending_undoable(db.conn()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].op, "adopt");

    db.write(|tx| event::finish(tx, id, false)).unwrap();
    assert!(event::pending_undoable(db.conn()).unwrap().is_empty());
    assert_eq!(event::recent(db.conn(), 10).unwrap().len(), 1);
}

#[test]
fn builtin_market_rows_survive_a_real_file_db() {
    let (_dir, mut db) = temp_db();
    db.write(|tx| {
        for (index, (id, name, enabled)) in BUILTIN_MARKETS.iter().enumerate() {
            let guided = matches!(*id, "qoder" | "skillhub_cn" | "skillsmp");
            source::upsert(
                tx,
                &SourceRow {
                    id: (*id).to_owned(),
                    name: (*name).to_owned(),
                    kind: "market".into(),
                    adapter: Some((*id).to_owned()),
                    url: None,
                    ref_name: None,
                    subpath: None,
                    path: None,
                    priority: index as i64,
                    trusted: true,
                    enabled: *enabled,
                    capability: Some(if guided {
                        "guided".into()
                    } else {
                        "indexed".into()
                    }),
                    last_synced_at: None,
                    last_commit: None,
                    last_probe_ok: None,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();

    let rows = source::list(db.conn()).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows.iter()
            .filter(|r| r.capability.as_deref() == Some("indexed"))
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.capability.as_deref() == Some("guided"))
            .count(),
        3
    );
}
