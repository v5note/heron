//! End-to-end scanner tests against a synthetic home directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use heron_scan::classify::ClientSpec;
use heron_scan::classify::{Classifier, HitKind, Scope};
use heron_scan::walker::{
    scan, scan_monitored, Hit, ScanMonitor, ScanOptions, ScanStats, PROGRESS_DIRS,
    PROGRESS_INTERVAL,
};

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn skill(name: &str, desc: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n")
}

/// Builds a fake home with claude + agents + a project.
fn fixture() -> (tempfile::TempDir, Classifier) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    let proj = home.join("work/projA");

    write(
        &home,
        ".claude/skills/pdf/SKILL.md",
        &skill("pdf", "Extract PDF text"),
    );
    write(&home, ".claude/skills/pdf/references/guide.md", "ref");
    write(
        &home,
        ".claude/commands/review.md",
        &skill("review", "Review code"),
    );
    write(
        &home,
        ".agents/skills/tdd/SKILL.md",
        &skill("tdd", "Write tests first"),
    );
    write(&home, ".agents/skills/broken/SKILL.md", "# no frontmatter");
    write(
        &proj,
        ".claude/skills/deploy/SKILL.md",
        &skill("deploy", "Deploy the app"),
    );
    write(
        &proj,
        "node_modules/pkg/SKILL.md",
        &skill("vendored", "should be ignored"),
    );
    write(
        &home,
        ".agents/skills/huashu/.agents/skills/inner/SKILL.md",
        &skill("inner", "nested"),
    );
    write(&home, ".claude/skills/.DS_Store", "junk");

    let clients = vec![
        ClientSpec {
            id: "claude".into(),
            display_name: "Claude Code".into(),
            global_paths: vec![home.join(".claude/skills")],
            project_paths: vec![PathBuf::from(".claude/skills")],
            command_paths: vec![home.join(".claude/commands")],
            project_command_paths: vec![PathBuf::from(".claude/commands")],
            priority: 20,
        },
        ClientSpec {
            id: "agents".into(),
            display_name: "Agents".into(),
            global_paths: vec![home.join(".agents/skills")],
            project_paths: vec![PathBuf::from(".agents/skills")],
            command_paths: vec![],
            project_command_paths: vec![],
            priority: 10,
        },
    ];
    let classifier = Classifier::new(clients, vec![proj]);
    (tmp, classifier)
}

#[test]
fn finds_skills_commands_and_nested_suspects() {
    let (tmp, classifier) = fixture();
    let result = scan(&classifier, &ScanOptions::default()).unwrap();

    let slugs: Vec<_> = result.hits.iter().filter_map(|h| h.slug.clone()).collect();
    assert!(
        slugs.contains(&"pdf".to_string()),
        "global claude skill found: {slugs:?}"
    );
    assert!(slugs.contains(&"tdd".to_string()), "agents skill found");
    assert!(slugs.contains(&"deploy".to_string()), "project skill found");
    assert!(
        slugs.contains(&"review".to_string()),
        "legacy command captured"
    );

    assert_eq!(result.count(HitKind::LegacyCommand), 1);
    // Nested-suspect skills are recorded in `nested` and still surface as a hit with that kind.
    let nested_hits = result
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::NestedSuspect)
        .count();
    assert_eq!(
        nested_hits, 1,
        "nested suspect is reported as a hit of kind nested-suspect"
    );
    assert_eq!(result.nested.len(), 1);
    // The nested skill itself is the reported subject (its parent is the copied tree).
    assert!(
        result.nested[0].path.ends_with("inner"),
        "nested path: {:?}",
        result.nested[0].path
    );

    // node_modules is ignored, .DS_Store is noise.
    assert!(!slugs.contains(&"vendored".to_string()));
    assert!(!result
        .stats
        .roots_missing
        .iter()
        .any(|p| p.ends_with(".claude/skills")));

    let _ = tmp;
}

#[test]
fn project_scope_is_reported() {
    let (_tmp, classifier) = fixture();
    let result = scan(&classifier, &ScanOptions::default()).unwrap();
    let deploy = result
        .hits
        .iter()
        .find(|h| h.slug.as_deref() == Some("deploy"))
        .unwrap();
    assert_eq!(deploy.scope, Some(Scope::Project));
    assert!(deploy.project.as_ref().unwrap().ends_with("projA"));
    assert_eq!(deploy.client.as_deref(), Some("claude"));
}

#[test]
fn broken_frontmatter_is_kept_but_flagged() {
    let (_tmp, classifier) = fixture();
    let result = scan(&classifier, &ScanOptions::default()).unwrap();
    let broken = result
        .hits
        .iter()
        .find(|h| h.slug.as_deref() == Some("broken"))
        .unwrap();
    assert!(
        !broken.frontmatter_ok(),
        "skill without frontmatter is flagged, not dropped"
    );
    assert!(broken.issues.iter().any(|i| i.detail == "no_frontmatter"));
}

#[test]
fn missing_roots_are_reported_not_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    let clients = vec![ClientSpec {
        id: "claude".into(),
        display_name: "Claude".into(),
        global_paths: vec![tmp.path().join("does-not-exist")],
        project_paths: vec![],
        command_paths: vec![],
        project_command_paths: vec![],
        priority: 0,
    }];
    let classifier = Classifier::new(clients, vec![]);
    let result = scan(&classifier, &ScanOptions::default()).unwrap();
    assert!(result.hits.is_empty());
    assert_eq!(result.stats.roots_missing.len(), 1);
}

#[test]
fn max_dirs_truncates_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..30 {
        write(
            tmp.path(),
            &format!(".claude/skills/s{i}/SKILL.md"),
            &skill(&format!("s{i}"), "x"),
        );
    }
    let clients = vec![ClientSpec {
        id: "claude".into(),
        display_name: "Claude".into(),
        global_paths: vec![tmp.path().join(".claude/skills")],
        project_paths: vec![],
        command_paths: vec![],
        project_command_paths: vec![],
        priority: 0,
    }];
    let classifier = Classifier::new(clients, vec![]);
    let opts = ScanOptions {
        max_dirs: 5,
        ..Default::default()
    };
    let result = scan(&classifier, &opts).unwrap();
    assert!(result.stats.truncated);
    assert!(result.hits.len() < 30);
}

#[test]
fn scan_is_deterministic() {
    let (_tmp, classifier) = fixture();
    let a = scan(&classifier, &ScanOptions::default()).unwrap();
    let b = scan(&classifier, &ScanOptions::default()).unwrap();
    let paths_a: Vec<_> = a.hits.iter().map(|h| h.path.clone()).collect();
    let paths_b: Vec<_> = b.hits.iter().map(|h| h.path.clone()).collect();
    assert_eq!(paths_a, paths_b);
}

/// A classifier that points one client's global root at `roots` and nothing else.
fn roots_classifier(roots: &[PathBuf]) -> Classifier {
    Classifier::new(
        vec![ClientSpec {
            id: "test".into(),
            display_name: "Test".into(),
            global_paths: roots.to_vec(),
            project_paths: vec![],
            command_paths: vec![],
            project_command_paths: vec![],
            priority: 10,
        }],
        vec![],
    )
}

/// `count` plain directories under `root`, the first of which is a skill: enough entries to
/// force the walker through several throttle windows, with a hit to carry along.
fn wide_tree(root: &Path, count: usize) {
    fs::create_dir_all(root).unwrap();
    for i in 0..count {
        let dir = root.join(format!("d{i:05}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("filler.txt"), "x").unwrap();
    }
    write(
        root,
        "d00000/SKILL.md",
        &skill("first", "found before the walk stops"),
    );
}

#[test]
fn a_cancelled_walk_stops_before_reading_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("skills");
    wide_tree(&root, 500);
    let classifier = roots_classifier(&[root]);
    let cancel = AtomicBool::new(true);

    let mut monitor = ScanMonitor {
        cancel: Some(&cancel),
        on_progress: None,
    };
    let result = scan_monitored(&classifier, &ScanOptions::default(), &mut monitor).unwrap();

    assert!(result.stats.canceled, "the result says it was cancelled");
    assert_eq!(result.stats.dirs_visited, 0, "not one directory is read");
    assert!(result.hits.is_empty());
    assert_eq!(result.stats.root_total, 1);
    assert_eq!(result.stats.root_index, 0);
}

#[test]
fn raising_cancel_during_the_walk_stops_it_early_and_keeps_what_it_found() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("skills");
    wide_tree(&root, 4000);
    let classifier = roots_classifier(&[root]);
    let cancel = AtomicBool::new(false);

    let mut ticks: Vec<ScanStats> = Vec::new();
    // The second tick asks the walk to stop. The block ends the borrow `ticks` gives the hook.
    let result = {
        let mut hook = |stats: &ScanStats, _hits: &[Hit]| {
            if ticks.len() == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
            ticks.push(stats.clone());
        };
        let mut monitor = ScanMonitor {
            cancel: Some(&cancel),
            on_progress: Some(&mut hook),
        };
        scan_monitored(&classifier, &ScanOptions::default(), &mut monitor).unwrap()
    };

    // Two throttled ticks, then the tick that flushes the tail of a walk that ended early.
    assert_eq!(ticks.len(), 2 + 1, "ticks: {ticks:?}");
    assert!(
        !ticks[0].canceled && !ticks[1].canceled,
        "the walk reports itself live, before it stops"
    );
    assert!(ticks[2].canceled);
    assert!(result.stats.canceled);
    assert!(
        result.stats.dirs_visited < 1_000,
        "the walk stopped near the second tick, not at {}",
        result.stats.dirs_visited
    );
    // A cancelled walk is a partial result, not a failure: what it found is still there.
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].slug.as_deref(), Some("first"));
}

#[test]
fn progress_ticks_are_throttled_and_say_which_root_is_being_walked() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    wide_tree(&a, 3000);
    wide_tree(&b, 3000);
    let classifier = roots_classifier(&[a, b]);

    let started = Instant::now();
    let mut ticks: Vec<ScanStats> = Vec::new();
    let result = {
        let mut hook = |stats: &ScanStats, _hits: &[Hit]| ticks.push(stats.clone());
        let mut monitor = ScanMonitor {
            cancel: None,
            on_progress: Some(&mut hook),
        };
        scan_monitored(&classifier, &ScanOptions::default(), &mut monitor).unwrap()
    };

    let dirs = result.stats.dirs_visited;
    assert!(dirs >= 6_000, "the tree is as wide as the test assumes: {dirs}");
    assert!(
        ticks.len() * 5 < dirs,
        "{ticks:?} is one progress event per directory"
    );
    // The other half of the contract: throttling must not starve the caller. A tick is allowed
    // whenever 200 directories have been visited since the last one, or 120ms have passed, so
    // the count can never exceed what the elapsed walk has actually paid for.
    let interval_ms = PROGRESS_INTERVAL.as_millis() as usize;
    // 200-directory windows, 120ms windows, and the forced ticks at the two root boundaries and
    // at the end of the walk.
    let allowed =
        4 + dirs / PROGRESS_DIRS + started.elapsed().as_millis() as usize / interval_ms;
    assert!(
        ticks.len() <= allowed,
        "{} ticks is more than the {allowed} the limits allow",
        ticks.len()
    );

    // Root bookkeeping is real, not estimated: every tick knows the denominator.
    assert!(ticks.iter().all(|t| t.root_total == 2));
    assert!(ticks.iter().any(|t| t.root_index == 1));
    assert!(ticks.iter().any(|t| t.root_index == 2));
    let last = ticks.last().unwrap();
    assert!(
        last.current_root.is_none(),
        "the closing tick has left the traversal"
    );
    assert!(!last.truncated);
    assert!(result.stats.current_root.is_none());
}
