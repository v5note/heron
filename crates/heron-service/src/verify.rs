//! Verify: would each projection actually be loaded by its client? (design §10.11.1)

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use heron_projection::verify::{classify, VerifyInput, Visibility};
use heron_store::db::{iso_from_unix_millis, now_iso};
use heron_store::models::VisibilityRow;
use heron_store::repos::{client, projection, target, visibility};

use crate::{Ctx, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub slug: String,
    pub target_id: String,
    pub path: String,
    pub mode: String,
    pub visibility: Visibility,
}

/// One step of a streamed verify pass: phase `"checking"` once per projection row, then a single
/// terminal tick whose phase is `"done"` or `"canceled"`.
#[derive(Debug, Clone, Copy)]
pub struct VerifyTick {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    pub findings: usize,
}

/// How a verify pass ended. `total` is the denominator it worked against (the projection rows it
/// was asked to look at), so a stopped pass reads as a fraction of the real workload.
#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub rows: Vec<VerifyReport>,
    pub total: usize,
    pub findings: usize,
    pub canceled: bool,
}

/// Verify every projection (or only those of `slug`), and refresh the visibility cache.
pub fn verify(ctx: &mut Ctx, slug: Option<&str>) -> Result<Vec<VerifyReport>> {
    verify_with_hook(ctx, slug, &mut |_| true).map(|outcome| outcome.rows)
}

/// `verify` with a progress hook: one tick per projection row plus the terminal one. Answering
/// `false` stops the pass as soon as possible.
///
/// A stopped pass still writes what it did check. The cache is upserted per projection and never
/// replaced wholesale, so the projections this pass never reached simply keep their previous rows
/// — while skipping the write entirely would throw away verdicts that were really gathered, and a
/// `DELETE` first would turn "not looked at yet" into "no cache at all".
pub fn verify_with_hook(
    ctx: &mut Ctx,
    slug: Option<&str>,
    hook: &mut dyn FnMut(VerifyTick) -> bool,
) -> Result<VerifyOutcome> {
    let rows = match slug {
        Some(slug) => projection::list_for_skill(ctx.db.conn(), slug)?,
        None => projection::list_all(ctx.db.conn())?,
    };
    let targets = target::list(ctx.db.conn())?;
    let clients = client::list(ctx.db.conn())?;
    let prev = visibility::list(ctx.db.conn())?;
    let session_rescan: Vec<String> = crate::clients::builtin()?
        .into_iter()
        .filter(|c| c.rescan.as_deref() == Some("session"))
        .map(|c| c.id)
        .collect();
    let checked_at = now_iso();

    let total = rows.len();
    let mut reports = Vec::new();
    let mut cache = Vec::new();
    let mut done = 0usize;
    // A finding is a verdict the user has to act on: every state but `visible` (not_scanned,
    // shadowed, restart_required, broken) counts; `unknown` never reaches a report.
    let mut findings = 0usize;
    let mut canceled = false;
    for row in rows {
        let Some(target_row) = targets.iter().find(|t| t.id == row.target_id) else {
            // The row was still looked at, so the bar has to move, or a store full of projections
            // whose target is gone would appear to hang.
            done += 1;
            if !hook(VerifyTick {
                phase: "checking",
                done,
                total,
                findings,
            }) {
                canceled = true;
                break;
            }
            continue;
        };
        // An adopted original keeps its own location (inside a client directory). Judge it by
        // that client's scan paths; everything else by its target's.
        let (kind, project) = if let Some((client, _)) = row.link_name.split_once('@') {
            (client.to_owned(), None)
        } else {
            (target_row.kind.clone(), target_row.project_path.clone())
        };
        let scan_paths = scan_paths_for(&kind, project.as_deref(), &clients, &targets);
        let occupied = occupied_names(&kind, project.as_deref());
        let path = PathBuf::from(&row.path);
        // A link resolves when its target exists; a plain copy resolves when the dir exists.
        let resolves = match heron_platform::link::probe(&path) {
            Ok(info) => {
                if info.kind.is_link() {
                    info.resolved_exists
                } else if info.kind.is_real_dir() {
                    true
                } else {
                    path.exists()
                }
            }
            Err(_) => path.exists(),
        };

        let rescans_on_session = session_rescan.iter().any(|id| id == &kind);
        // Restart accounting for session-rescan clients (design §12.3): a landing stays
        // "restart required" until some earlier verify has already confirmed it — the user has
        // then had a chance to reopen the client. A newer on-disk mtime (re-projection or an
        // in-place refresh under the same row) starts the clock over.
        let landing_unconfirmed = rescans_on_session
            && match prev.iter().find(|v| v.projection_id == row.id) {
                None => true,
                Some(v) => match landed_at(&path) {
                    Some(ms) => v.checked_at.as_str() < iso_from_unix_millis(ms).as_str(),
                    None => false,
                },
            };

        let visibility = classify(&VerifyInput {
            path,
            scan_paths,
            occupied_names: occupied,
            resolves,
            rescans_on_session,
            landing_unconfirmed,
        })?;

        // The table's CHECK has no `unknown`; classify never yields it, but the cache says so
        // explicitly instead of tripping the constraint.
        if !matches!(visibility, Visibility::Unknown) {
            cache.push(VisibilityRow {
                projection_id: row.id,
                client_id: kind,
                state: visibility.as_str().to_owned(),
                shadowed_by: match &visibility {
                    Visibility::Shadowed { by } => Some(by.clone()),
                    _ => None,
                },
                checked_at: checked_at.clone(),
            });
        }
        if !matches!(visibility, Visibility::Visible) {
            findings += 1;
        }
        reports.push(VerifyReport {
            slug: row.slug.clone(),
            target_id: row.target_id.clone(),
            path: row.path.clone(),
            mode: row.mode.clone(),
            visibility,
        });
        done += 1;
        if !hook(VerifyTick {
            phase: "checking",
            done,
            total,
            findings,
        }) {
            canceled = true;
            break;
        }
    }
    ctx.db.write(|tx| {
        for row in &cache {
            visibility::upsert(tx, row)?;
        }
        Ok(())
    })?;
    let _ = hook(VerifyTick {
        phase: if canceled { "canceled" } else { "done" },
        done,
        total,
        findings,
    });
    Ok(VerifyOutcome {
        rows: reports,
        total,
        findings,
        canceled,
    })
}

/// When the projection itself landed on disk (the link's own mtime, target untouched).
fn landed_at(path: &Path) -> Option<i64> {
    std::fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// Which directories a client scans, resolved for this projection's scope.
fn scan_paths_for(
    kind: &str,
    project: Option<&str>,
    clients: &[heron_store::models::ClientRow],
    targets: &[heron_store::models::TargetRow],
) -> Vec<PathBuf> {
    let _ = targets;
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(row) = clients.iter().find(|c| c.id == kind) {
        for p in &row.global_paths {
            paths.push(PathBuf::from(p));
        }
        if let Some(project) = project {
            for rel in &row.project_paths {
                paths.push(PathBuf::from(project).join(rel));
            }
        }
    }
    paths
}

/// Names already claimed by higher-priority sources (plugins) for that client.
fn occupied_names(_kind: &str, _project: Option<&str>) -> Vec<String> {
    // Plugin detection lands with the native-scan work (M3); until then nothing is shadowed.
    Vec::new()
}

/// Is this client known at all? Used by the CLI to warn about unknown target kinds.
pub fn known_client(id: &str) -> bool {
    id == "custom"
        || crate::clients::builtin()
            .map(|m| m.iter().any(|c| c.id == id))
            .unwrap_or(false)
}

/// Helper so the report can be rendered in one line.
pub fn describe(visibility: &Visibility) -> String {
    match visibility {
        Visibility::Visible => "visible".to_owned(),
        Visibility::NotScanned => "not scanned".to_owned(),
        Visibility::Shadowed { by } => format!("shadowed by {by}"),
        Visibility::RestartRequired => "restart required".to_owned(),
        Visibility::Broken => "broken".to_owned(),
        Visibility::Unknown => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron_platform::dirs::Dirs;
    use heron_store::db::Db;
    use heron_store::models::{ClientRow, ProjectionRow, SkillRow, TargetRow};
    use heron_store::repos::skill;
    use std::fs;

    /// Three projections, ordered by slug the way `list_all` orders them: `alpha` lands inside the
    /// client's scan directory and is therefore visible, `bravo` and `charlie` point at paths that
    /// do not exist and are therefore broken.
    struct Store {
        /// Kept alive so the temporary home outlives the database inside it.
        _tmp: tempfile::TempDir,
        ctx: Ctx,
        ids: std::collections::HashMap<String, i64>,
    }

    impl Store {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data = tmp.path().join("data");
            let scanned = tmp.path().join("client").join("skills");
            fs::create_dir_all(scanned.join("alpha")).unwrap();
            let dirs = Dirs::resolve_with_override("heron", Some(&data)).unwrap();
            dirs.ensure().unwrap();
            let db = Db::open(&dirs.db_path()).unwrap();
            let mut ctx = Ctx::new(dirs, db, "test");

            let mut ids = std::collections::HashMap::new();
            ctx.db
                .write(|tx| {
                    client::upsert(
                        tx,
                        &ClientRow {
                            id: "testclient".to_owned(),
                            display_name: "Test".to_owned(),
                            global_paths: vec![scanned.to_string_lossy().to_string()],
                            project_paths: vec![],
                            link_modes: vec!["symlink".to_owned()],
                            global_commands: vec![],
                            project_commands: vec![],
                            priority: 10,
                            native_scan: None,
                            verified_at: None,
                            verified_by: None,
                            detected: Some(1),
                            detected_at: None,
                            detect_evidence: vec![],
                        },
                    )?;
                    target::upsert(
                        tx,
                        &TargetRow {
                            id: "t1".to_owned(),
                            kind: "testclient".to_owned(),
                            scope: "global".to_owned(),
                            project_path: None,
                            path: scanned.to_string_lossy().to_string(),
                            enabled: true,
                            is_default: true,
                            discover: "manual".to_owned(),
                        },
                    )?;
                    for (slug, dir) in [
                        ("alpha", scanned.join("alpha")),
                        ("bravo", tmp.path().join("gone").join("bravo")),
                        ("charlie", tmp.path().join("gone").join("charlie")),
                    ] {
                        skill::insert(
                            tx,
                            &SkillRow {
                                slug: slug.to_owned(),
                                display_name: slug.to_owned(),
                                description: String::new(),
                                tags: vec![],
                                status: "active".to_owned(),
                                origin: "store".to_owned(),
                                storage_path: None,
                                subpath: None,
                                created_at: String::new(),
                                updated_at: String::new(),
                            },
                        )?;
                        let id = projection::insert(
                            tx,
                            &ProjectionRow {
                                id: 0,
                                slug: slug.to_owned(),
                                target_id: "t1".to_owned(),
                                link_name: slug.to_owned(),
                                path: dir.to_string_lossy().to_string(),
                                mode: "symlink".to_owned(),
                                version: None,
                                content_hash: None,
                                inode_kind: None,
                                state: String::new(),
                                created_at: String::new(),
                            },
                        )?;
                        ids.insert(slug.to_owned(), id);
                    }
                    Ok(())
                })
                .unwrap();
            Self {
                _tmp: tmp,
                ctx,
                ids,
            }
        }

        /// The cached verdict of one projection, straight out of the table.
        fn cached(&self, slug: &str) -> Option<VisibilityRow> {
            let id = self.ids[slug];
            visibility::list(self.ctx.db.conn())
                .unwrap()
                .into_iter()
                .find(|v| v.projection_id == id)
        }
    }

    #[test]
    fn a_whole_pass_reports_every_projection_and_counts_the_findings() {
        let mut store = Store::new();
        let outcome = verify_with_hook(&mut store.ctx, None, &mut |_| true).unwrap();
        assert!(!outcome.canceled);
        assert_eq!(outcome.total, 3);
        assert_eq!(outcome.rows.len(), 3);
        // One is visible, so the two broken landings are the findings.
        assert_eq!(outcome.findings, 2);
        assert_eq!(outcome.rows[0].visibility, Visibility::Visible);
        assert_eq!(outcome.rows[1].visibility, Visibility::Broken);
    }

    /// The old entry point has to keep behaving exactly as before for the CLI and the tests.
    #[test]
    fn the_wrapper_answers_with_the_rows_alone() {
        let mut store = Store::new();
        let reports = verify(&mut store.ctx, None).unwrap();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].slug, "alpha");
        let scoped = verify(&mut store.ctx, Some("bravo")).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].slug, "bravo");
    }

    #[test]
    fn ticks_only_ever_move_forward() {
        let mut store = Store::new();
        let mut seen = Vec::new();
        verify_with_hook(&mut store.ctx, None, &mut |tick| {
            seen.push(tick);
            true
        })
        .unwrap();
        assert_eq!(seen.len(), 4, "three rows plus the terminal tick");
        assert_eq!(seen.last().unwrap().phase, "done");
        let mut prev = 0;
        for tick in seen.iter().take(seen.len() - 1) {
            assert_eq!(tick.total, 3, "the denominator never changes mid-pass");
            assert!(tick.done > prev, "{seen:?}");
            prev = tick.done;
        }
        // The terminal tick only restates the finish line, which is what lets a bar reach 100%
        // without jumping past it.
        let last = seen.last().unwrap();
        assert_eq!((last.done, last.total, last.findings), (3, 3, 2));
        assert_eq!(seen[0].findings, 0, "alpha is visible");
        assert_eq!(seen[2].findings, 2);
    }

    /// Cancelling is a correctness matter, not a speed one: a pass that stopped halfway must not
    /// cost the user verdicts it never gathered.
    #[test]
    fn a_stopped_pass_keeps_the_cache_it_never_reached() {
        let mut store = Store::new();
        let sentinel = VisibilityRow {
            projection_id: store.ids["charlie"],
            client_id: "testclient".to_owned(),
            state: "visible".to_owned(),
            shadowed_by: None,
            checked_at: "1970-01-01T00:00:00.000Z".to_owned(),
        };
        store
            .ctx
            .db
            .write(|tx| visibility::upsert(tx, &sentinel))
            .unwrap();

        let outcome = verify_with_hook(&mut store.ctx, None, &mut |tick| tick.done < 1).unwrap();
        assert!(outcome.canceled);
        assert_eq!(outcome.rows.len(), 1, "only alpha was actually judged");
        assert_eq!(outcome.total, 3);

        // The row this pass did judge is written; the one it never reached keeps its old verdict.
        let alpha = store.cached("alpha").expect("checked row is cached");
        assert_eq!(alpha.state, "visible");
        let charlie = store.cached("charlie").expect("unchecked row survives");
        assert_eq!(charlie.checked_at, "1970-01-01T00:00:00.000Z");
        assert!(
            store.cached("bravo").is_none(),
            "never reached, never invented"
        );
    }
}
