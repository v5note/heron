//! Argument parsing and command dispatch.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use heron_core::{ErrorCode, HeronError, Result};
use heron_scan::classify::{Classification, Classifier, HitKind, Scope};
use heron_scan::walker::{scan, ScanOptions};
use heron_store::db::Db;
use heron_store::models::{ClientRow, DetectedUpsert, TargetRow};
use heron_store::repos::{client, detected, skill, target};

use crate::clients::{builtin_specs, default_landing};
use crate::render::{human_bytes, print_hits, print_scan};

#[derive(Parser, Debug)]
#[command(
    name = "heron",
    version,
    about = "Heron - local skill manager (M0 preview)"
)]
pub struct Cli {
    /// Machine-readable output.
    #[arg(long, global = true)]
    pub json: bool,
    /// Override the data directory (defaults to the platform location).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scan every known skill location and refresh the index.
    Scan {
        /// Additional roots to include (extra projects, external volumes).
        #[arg(long = "roots")]
        roots: Vec<PathBuf>,
        #[arg(long)]
        max_dirs: Option<usize>,
        /// Print every hit, including nested suspects.
        #[arg(long)]
        verbose: bool,
    },
    /// List managed skills.
    List,
    /// Show what exists on disk that Heron does not manage yet.
    Detected {
        /// candidate | ignored | adopted | conflict | all
        #[arg(long, default_value = "all")]
        state: String,
        /// Include nested suspects in the listing.
        #[arg(long)]
        all: bool,
    },
}

pub fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let dirs = heron_platform::dirs::Dirs::resolve_with_override("heron", cli.data_dir.as_deref())
        .map_err(HeronError::from)?;
    if std::env::var_os("HERON_DEBUG_DIRS").is_some() {
        eprintln!(
            "debug: data={} config={} cache={} log={}",
            heron_platform::paths::display(&dirs.data),
            heron_platform::paths::display(&dirs.config),
            heron_platform::paths::display(&dirs.cache),
            heron_platform::paths::display(&dirs.log)
        );
    }
    dirs.ensure().map_err(HeronError::from)?;

    let mut db = Db::open(&dirs.db_path())?;
    match cli.command {
        Command::Scan {
            roots,
            max_dirs,
            verbose,
        } => cmd_scan(&mut db, &roots, max_dirs, verbose, cli.json),
        Command::List => cmd_list(&db, cli.json),
        Command::Detected { state, all } => cmd_detected(&db, &state, all, cli.json),
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
        })
        .ok_or_else(|| HeronError::from(ErrorCode::Internal.msg().with("detail", "no_home_dir")))
}

fn cmd_scan(
    db: &mut Db,
    roots: &[PathBuf],
    max_dirs: Option<usize>,
    verbose: bool,
    json: bool,
) -> Result<ExitCode> {
    let home = home_dir()?;
    bootstrap(db, &home)?;

    let specs = client_specs(db)?;
    let projects = project_roots(db)?;
    // Roots passed on the command line behave as extra project roots.
    let mut projects = projects;
    for root in roots {
        if !projects
            .iter()
            .any(|p| heron_platform::paths::same_path_ci(p, root))
        {
            projects.push(root.clone());
        }
    }
    let classifier = Classifier::new(specs, projects);

    let mut options = ScanOptions::default();
    if let Some(limit) = max_dirs {
        options.max_dirs = limit;
    }
    let result = scan(&classifier, &options)?;

    let mut seen = std::collections::HashSet::new();
    db.write(|tx| {
        for hit in &result.hits {
            let path = hit.path.to_string_lossy().to_string();
            seen.insert(path.clone());
            let (mtime_ns, size_bytes) = file_stats(&hit.path);
            detected::upsert(
                tx,
                &DetectedUpsert {
                    path,
                    real_path: Some(hit.real_path.to_string_lossy().to_string()),
                    client_id: hit.client.clone(),
                    scope: hit.scope.map(scope_str),
                    project_path: hit
                        .project
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    observed_slug: hit.slug.clone(),
                    content_hash: None,
                    frontmatter_ok: Some(hit.frontmatter_ok()),
                    kind: kind_str(hit.kind),
                    mtime_ns,
                    size_bytes,
                },
            )?;
        }
        detected::prune_not_in(tx, &seen)?;
        Ok(())
    })?;
    db.kv_set("last_scan_at", &chrono::Utc::now().to_rfc3339())?;

    if json {
        let out = serde_json::json!({
            "skills": result.hits.iter().filter(|h| h.kind == HitKind::Skill).count(),
            "legacyCommands": result.hits.iter().filter(|h| h.kind == HitKind::LegacyCommand).count(),
            "nestedSuspects": result.nested.len(),
            "needsAttention": result.hits.iter().filter(|h| !h.frontmatter_ok()).count(),
            "dirsVisited": result.stats.dirs_visited,
            "durationMs": result.stats.duration_ms,
            "truncated": result.stats.truncated,
            "rootsMissing": result.stats.roots_missing.iter().map(|p| heron_platform::paths::display(p)).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        print_scan(&result);
        if verbose {
            println!();
            print_hits(&result, true);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn scope_str(scope: Scope) -> String {
    match scope {
        Scope::Global => "global".to_owned(),
        Scope::Project => "project".to_owned(),
    }
}

fn kind_str(kind: HitKind) -> String {
    match kind {
        HitKind::Skill => "skill",
        HitKind::LegacyCommand => "legacy-command",
        HitKind::NestedSuspect => "nested-suspect",
    }
    .to_owned()
}

fn file_stats(path: &Path) -> (Option<i64>, Option<i64>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return (None, None);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64);
    (mtime, Some(meta.len() as i64))
}

/// Seed the client matrix and the default landing target on first run.
fn bootstrap(db: &mut Db, home: &Path) -> Result<()> {
    if db.kv_get("bootstrap_done")?.as_deref() == Some("1") {
        return Ok(());
    }
    let specs = builtin_specs(home);
    db.write(|tx| {
        for spec in &specs {
            client::upsert(
                tx,
                &ClientRow {
                    id: spec.id.clone(),
                    display_name: spec.display_name.clone(),
                    // Stored values are real filesystem paths; `display()` is presentation only.
                    global_paths: spec
                        .global_paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    project_paths: spec
                        .project_paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    link_modes: vec!["symlink".into(), "junction".into(), "copy".into()],
                    global_commands: spec
                        .command_paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    project_commands: spec
                        .project_command_paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    priority: spec.priority,
                    native_scan: None,
                    verified_at: Some("2026-11-19".into()),
                    verified_by: Some("doc".into()),
                },
            )?;
        }
        // The default landing is owned by Heron itself (decision D10), so it needs its own
        // client row: targets reference `client.id`.
        client::upsert(
            tx,
            &ClientRow {
                id: "heron".into(),
                display_name: "Heron landing".into(),
                global_paths: vec![default_landing(home).to_string_lossy().to_string()],
                project_paths: vec![".heron/skills".into()],
                link_modes: vec!["symlink".into(), "junction".into(), "copy".into()],
                global_commands: vec![],
                project_commands: vec![],
                priority: 0,
                native_scan: None,
                verified_at: Some("2026-11-19".into()),
                verified_by: Some("design".into()),
            },
        )?;
        target::upsert(
            tx,
            &TargetRow {
                id: "heron:global".into(),
                kind: "heron".into(),
                scope: "global".into(),
                project_path: None,
                path: default_landing(home).to_string_lossy().to_string(),
                enabled: true,
                is_default: true,
                discover: "manual".into(),
            },
        )?;
        Ok(())
    })?;
    db.kv_set("bootstrap_done", "1")?;
    Ok(())
}

fn client_specs(db: &Db) -> Result<Vec<heron_scan::classify::ClientSpec>> {
    Ok(client::list(db.conn())?
        .into_iter()
        .map(|row| heron_scan::classify::ClientSpec {
            id: row.id,
            display_name: row.display_name,
            global_paths: row.global_paths.iter().map(PathBuf::from).collect(),
            project_paths: row.project_paths.iter().map(PathBuf::from).collect(),
            command_paths: row.global_commands.iter().map(PathBuf::from).collect(),
            project_command_paths: row.project_commands.iter().map(PathBuf::from).collect(),
            priority: row.priority,
        })
        .collect())
}

fn project_roots(db: &Db) -> Result<Vec<PathBuf>> {
    Ok(target::list(db.conn())?
        .into_iter()
        .filter(|t| t.scope == "project")
        .filter_map(|t| t.project_path.map(PathBuf::from))
        .collect())
}

fn cmd_list(db: &Db, json: bool) -> Result<ExitCode> {
    let skills = skill::list(db.conn(), false)?;
    let all = detected::list_all(db.conn())?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "managed": skills, "detected": all.len() })
            )
            .unwrap()
        );
    } else if skills.is_empty() {
        println!("no managed skills yet - run `heron scan`, then `heron detected`");
        println!("(found on disk: {} items)", all.len());
    } else {
        for s in &skills {
            println!("{:<28} {:<10} {}", s.slug, s.origin, s.display_name);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_detected(db: &Db, state: &str, all: bool, json: bool) -> Result<ExitCode> {
    let rows = if state == "all" {
        detected::list_all(db.conn())?
    } else {
        detected::list_by_state(db.conn(), state)?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    if rows.is_empty() {
        println!("nothing detected - run `heron scan` first");
        return Ok(ExitCode::SUCCESS);
    }
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for row in &rows {
        *counts.entry(row.kind.clone()).or_default() += 1;
        if all || row.kind != "nested-suspect" {
            let name = row.observed_slug.clone().unwrap_or_else(|| "?".into());
            let client = row.client_id.clone().unwrap_or_else(|| "-".into());
            let scope = row.scope.clone().unwrap_or_else(|| "-".into());
            let flag = if row.frontmatter_ok == Some(false) {
                "  [needs attention]"
            } else {
                ""
            };
            let shown = heron_platform::paths::display(Path::new(&row.path));
            println!(
                "{name:<28} {client:<9} {scope:<8} {:<16} {}{flag}",
                row.kind, shown
            );
        }
    }
    println!();
    for (kind, n) in counts {
        println!("{n:>4}  {kind}");
    }
    let total_bytes: u64 = rows
        .iter()
        .filter_map(|r| r.size_bytes.map(|b| b as u64))
        .sum();
    if total_bytes > 0 {
        println!(
            "total size of detected SKILL.md files: {}",
            human_bytes(total_bytes)
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(dead_code)]
fn _classification_marker(_: &Classification) {}
