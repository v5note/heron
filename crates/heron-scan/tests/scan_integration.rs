//! End-to-end scanner tests against a synthetic home directory.

use std::fs;
use std::path::{Path, PathBuf};

use heron_scan::classify::ClientSpec;
use heron_scan::classify::{Classifier, HitKind, Scope};
use heron_scan::walker::{scan, ScanOptions};

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
