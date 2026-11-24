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
