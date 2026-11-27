//! Doctor: the check registry (design §8.2 D).
//!
//! Every check is read-only and reports an `Issue` with a suggested action. Nested suspects
//! (decision D8) are *only* reported: Heron never reorganizes a user's tree on its own.

use std::path::PathBuf;

use serde::Serialize;

use crate::{Ctx, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Stable check id, e.g. `nested-suspect`, `broken-link`, `duplicate-content`.
    pub id: String,
    pub severity: Severity,
    pub subject: String,
    /// Suggested actions the user can take; never executed automatically.
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DoctorReport {
    pub issues: Vec<Issue>,
    /// The run was stopped before it reached the end, so `issues` is a prefix of the whole answer
    /// and the UI must not present it as a clean bill of health.
    #[serde(default)]
    pub canceled: bool,
}

impl DoctorReport {
    pub fn count(&self, severity: Severity) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .count()
    }
}

/// One step of a streamed doctor run: `"detected"`, `"projection"` and `"content"` while the three
/// loops work, then a single terminal tick whose phase is `"done"` or `"canceled"`.
#[derive(Debug, Clone, Copy)]
pub struct DoctorTick {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    pub findings: usize,
}

/// Snapshot read under a short DB lock so disk checks and blake3 hashing run lock-free.
#[derive(Debug, Clone)]
pub struct DoctorPlan {
    candidates: Vec<heron_store::models::DetectedRow>,
    projections: Vec<heron_store::models::ProjectionRow>,
    skills: Vec<(String, PathBuf)>,
}

/// Stage 1: snapshot every row `doctor` inspects under a short lock.
pub fn prepare(ctx: &Ctx) -> Result<DoctorPlan> {
    let candidates = heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate")?;
    let projections = heron_store::repos::projection::list_all(ctx.db.conn())?;
    let skills = heron_store::repos::skill::list(ctx.db.conn(), false)?
        .into_iter()
        .map(|s| {
            let dir = ctx.content_path(&s);
            (s.slug, dir)
        })
        .collect();
    Ok(DoctorPlan {
        candidates,
        projections,
        skills,
    })
}

/// Stage 2: run all filesystem checks and directory fingerprints without holding `Ctx`.
pub fn execute_plan(
    plan: DoctorPlan,
    hook: &mut dyn FnMut(DoctorTick) -> bool,
) -> Result<DoctorReport> {
    let total = plan.candidates.len() + plan.projections.len() + plan.skills.len();

    let mut issues = Vec::new();
    let mut done = 0usize;
    let mut canceled = false;

    // 1) Items on disk that Heron does not manage.
    for row in &plan.candidates {
        if row.kind == "nested-suspect" {
            issues.push(Issue {
                id: "nested-suspect".to_owned(),
                severity: Severity::Warn,
                subject: row.path.clone(),
                actions: vec![
                    "ignore".to_owned(),
                    "open-directory".to_owned(),
                    "export-diagnostics".to_owned(),
                ],
            });
        }
        if row.frontmatter_ok == Some(false) {
            issues.push(Issue {
                id: "frontmatter-invalid".to_owned(),
                severity: Severity::Warn,
                subject: row.path.clone(),
                actions: vec!["open-directory".to_owned()],
            });
        }
        if !step(hook, "detected", &mut done, total, &issues) {
            canceled = true;
            break;
        }
    }

    // 2) Broken projections.
    if !canceled {
        for row in &plan.projections {
            let path = PathBuf::from(&row.path);
            if !path.exists() {
                issues.push(Issue {
                    id: "broken-link".to_owned(),
                    severity: Severity::Error,
                    subject: row.path.clone(),
                    actions: vec!["reproject".to_owned(), "unproject".to_owned()],
                });
            }
            if !step(hook, "projection", &mut done, total, &issues) {
                canceled = true;
                break;
            }
        }
    }

    // 3) Duplicate content across managed skills, hashed in small parallel chunks so multi-core
    //    machines finish faster while keeping cancellation responsive between chunks.
    if !canceled {
        const CHUNK: usize = 4;
        let mut seen: std::collections::HashMap<String, String> = Default::default();
        for chunk in plan.skills.chunks(CHUNK) {
            let results: Vec<(&str, bool, Option<String>)> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|(slug, dir)| {
                        s.spawn(move || {
                            if !dir.is_dir() {
                                return (slug.as_str(), false, None);
                            }
                            let hash = heron_scan::fingerprint_dir(dir, &Default::default())
                                .ok()
                                .map(|fp| fp.hash.to_string());
                            (slug.as_str(), true, hash)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap_or(("", false, None)))
                    .collect()
            });

            for (slug, exists, hash) in results {
                if !exists {
                    issues.push(Issue {
                        id: "store-missing".to_owned(),
                        severity: Severity::Error,
                        subject: slug.to_owned(),
                        actions: vec!["restore-from-backup".to_owned(), "remove".to_owned()],
                    });
                } else if let Some(hash) = hash {
                    if let Some(other) = seen.insert(hash, slug.to_owned()) {
                        issues.push(Issue {
                            id: "duplicate-content".to_owned(),
                            severity: Severity::Info,
                            subject: format!("{other} == {slug}"),
                            actions: vec!["merge".to_owned()],
                        });
                    }
                }
                if !step(hook, "content", &mut done, total, &issues) {
                    canceled = true;
                    break;
                }
            }
            if canceled {
                break;
            }
        }
    }

    let _ = hook(DoctorTick {
        phase: if canceled { "canceled" } else { "done" },
        done,
        total,
        findings: issues.len(),
    });
    Ok(DoctorReport { issues, canceled })
}

/// Run every check, reporting each unit of work to `hook`. Answering `false` stops the run at the
/// next unit and the report says so; what it already found is kept.
pub fn run_with_hook(ctx: &Ctx, hook: &mut dyn FnMut(DoctorTick) -> bool) -> Result<DoctorReport> {
    let plan = prepare(ctx)?;
    execute_plan(plan, hook)
}

/// One unit of work reported. `findings` is what has been found so far: every check here adds an
/// issue only when something actually needs the user's attention.
fn step(
    hook: &mut dyn FnMut(DoctorTick) -> bool,
    phase: &'static str,
    done: &mut usize,
    total: usize,
    issues: &[Issue],
) -> bool {
    *done += 1;
    hook(DoctorTick {
        phase,
        done: *done,
        total,
        findings: issues.len(),
    })
}

/// Run every check. Read-only: the doctor reports, the user decides.
pub fn run(ctx: &Ctx) -> Result<DoctorReport> {
    run_with_hook(ctx, &mut |_| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron_platform::dirs::Dirs;
    use heron_store::db::Db;
    use heron_store::models::{ClientRow, DetectedUpsert, ProjectionRow, SkillRow, TargetRow};
    use heron_store::repos::{client, detected, projection, skill, target};
    use std::fs;

    /// Seven units of work, one per loop plus enough of them to stop in the middle of the run:
    /// two candidates, two projections whose paths are gone, three skills (two with identical
    /// content, one with no directory at all).
    struct Store {
        /// Kept alive so the temporary home outlives the database inside it.
        _tmp: tempfile::TempDir,
        ctx: Ctx,
    }

    impl Store {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data = tmp.path().join("data");
            let dirs = Dirs::resolve_with_override("heron", Some(&data)).unwrap();
            dirs.ensure().unwrap();
            let db = Db::open(&dirs.db_path()).unwrap();
            let mut ctx = Ctx::new(dirs, db, "test");

            fs::create_dir_all(ctx.store_root().join("twin-a")).unwrap();
            fs::create_dir_all(ctx.store_root().join("twin-b")).unwrap();
            let body = "---\nname: twin\ndescription: same bytes\n---\n";
            fs::write(ctx.store_root().join("twin-a/SKILL.md"), body).unwrap();
            fs::write(ctx.store_root().join("twin-b/SKILL.md"), body).unwrap();

            ctx.db
                .write(|tx| {
                    // The projection rows point at a real target, which in turn names a real
                    // client; the schema's foreign keys do not forgive shortcuts.
                    client::upsert(
                        tx,
                        &ClientRow {
                            id: "testclient".to_owned(),
                            display_name: "Test".to_owned(),
                            global_paths: vec![],
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
                            path: tmp.path().to_string_lossy().into(),
                            enabled: true,
                            is_default: true,
                            discover: "manual".to_owned(),
                        },
                    )?;
                    for (path, kind, frontmatter) in [
                        ("/abs/loose-skill", "skill", Some(true)),
                        ("/abs/nested/inner", "nested-suspect", Some(false)),
                    ] {
                        detected::upsert(
                            tx,
                            &DetectedUpsert {
                                path: path.to_owned(),
                                real_path: None,
                                client_id: Some("testclient".to_owned()),
                                scope: Some("global".to_owned()),
                                project_path: None,
                                observed_slug: None,
                                content_hash: None,
                                frontmatter_ok: frontmatter,
                                kind: kind.to_owned(),
                                mtime_ns: None,
                                size_bytes: None,
                            },
                        )?;
                    }
                    for slug in ["twin-a", "twin-b", "vanished"] {
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
                    }
                    for slug in ["twin-a", "twin-b"] {
                        projection::insert(
                            tx,
                            &ProjectionRow {
                                slug: slug.to_owned(),
                                link_name: slug.to_owned(),
                                target_id: "t1".to_owned(),
                                path: tmp.path().join("gone").join(slug).to_string_lossy().into(),
                                mode: "symlink".to_owned(),
                                ..ProjectionRow::default()
                            },
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            Self { _tmp: tmp, ctx }
        }
    }

    /// The issues in the order they were found, for order-sensitive assertions.
    fn ids(report: &DoctorReport) -> Vec<&str> {
        report.issues.iter().map(|i| i.id.as_str()).collect()
    }

    /// Stopping the run halfway is honest only if the report says so and keeps what it found.
    #[test]
    fn a_stopped_run_reports_what_it_got_and_says_it_stopped() {
        let store = Store::new();
        let full = run(&store.ctx).unwrap();
        assert!(!full.canceled);
        let mut everything = ids(&full);
        everything.sort_unstable();
        assert_eq!(
            everything,
            vec![
                "broken-link",
                "broken-link",
                "duplicate-content",
                "frontmatter-invalid",
                "nested-suspect",
                "store-missing"
            ]
        );

        // Three units stops on the last projection, so the content loop — the slow one — never runs.
        let mut stopped = None;
        let report = run_with_hook(&store.ctx, &mut |tick| {
            if tick.done >= 3 {
                stopped = Some(tick);
                return false;
            }
            true
        })
        .unwrap();
        let stopped = stopped.unwrap();
        assert_eq!(stopped.total, 7, "the denominator counts all three lists");
        assert_eq!(stopped.phase, "canceled");
        assert!(report.canceled);
        assert!(report.issues.len() < full.issues.len());
        // Same data, same order: a stopped run's issues are a prefix of the whole answer.
        assert!(ids(&full).starts_with(&ids(&report)));
        for id in ["duplicate-content", "store-missing"] {
            assert!(
                !ids(&report).contains(&id),
                "{id} belongs to the skipped loop"
            );
        }
    }

    #[test]
    fn ticks_only_ever_move_forward_and_end_on_the_terminal_phase() {
        let store = Store::new();
        let mut seen = Vec::new();
        let report = run_with_hook(&store.ctx, &mut |tick| {
            seen.push(tick);
            true
        })
        .unwrap();
        assert!(!report.canceled);
        assert_eq!(
            seen.iter().map(|t| t.phase).collect::<Vec<_>>(),
            vec![
                "detected",
                "detected",
                "projection",
                "projection",
                "content",
                "content",
                "content",
                "done"
            ]
        );
        let mut prev = 0;
        for tick in seen.iter().take(seen.len() - 1) {
            assert_eq!(tick.total, 7, "the denominator never changes mid-run");
            assert!(tick.done > prev, "{seen:?}");
            // Findings only ever grow, and never outrun what the report ends up carrying.
            assert!(tick.findings <= report.issues.len());
            prev = tick.done;
        }
        // The terminal tick only restates the finish line, which is what lets a bar reach 100%
        // without jumping past it.
        assert_eq!(seen.last().unwrap().done, 7);
        assert_eq!(seen.last().unwrap().findings, report.issues.len());
    }

    #[test]
    fn an_empty_store_completes_without_a_denominator() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let dirs = Dirs::resolve_with_override("heron", Some(&data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        let ctx = Ctx::new(dirs, db, "test");
        let mut seen = Vec::new();
        let report = run_with_hook(&ctx, &mut |tick| {
            seen.push(tick);
            true
        })
        .unwrap();
        assert!(report.issues.is_empty());
        assert!(!report.canceled);
        assert_eq!(seen.len(), 1, "just the terminal tick");
        assert_eq!(seen[0].total, 0);
    }

    /// The old entry point has to keep behaving exactly as before for the CLI.
    #[test]
    fn the_wrapper_answers_like_an_unstopped_hook() {
        let store = Store::new();
        let plain = run(&store.ctx).unwrap();
        let hooked = run_with_hook(&store.ctx, &mut |_| true).unwrap();
        assert_eq!(ids(&plain), ids(&hooked));
        assert!(!plain.canceled);
        assert_eq!(plain.count(Severity::Error), hooked.count(Severity::Error));
    }
}
