//! Argument parsing and dispatch. Every command delegates to `heron-service`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use heron_core::{ErrorCode, HeronError, ProjectionMode, Result};
use heron_service::adopt::{self, AdoptPlacement};
use heron_service::project::{self, ProjectRequest};
use heron_service::scan::{self, ScanRequest};
use heron_service::{doctor, verify, Ctx};
use heron_store::db::Db;
use heron_store::repos::{detected, skill, target};

use crate::render::human_bytes;

#[derive(Parser, Debug)]
#[command(name = "heron", version, about = "Heron - local skill manager")]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scan every known skill location and refresh the index.
    Scan {
        #[arg(long = "roots")]
        roots: Vec<PathBuf>,
        #[arg(long)]
        max_dirs: Option<usize>,
        #[arg(long)]
        verbose: bool,
    },
    /// List managed skills.
    List,
    /// Show what exists on disk that Heron does not manage yet.
    Detected {
        #[arg(long, default_value = "all")]
        state: String,
        #[arg(long)]
        all: bool,
    },
    /// Bring detected skills into the managed store.
    Adopt {
        /// Slugs or paths to adopt; omit with --all to take every candidate.
        targets: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        dry_run: bool,
        /// Keep the original where it is instead of replacing it with a projection.
        #[arg(long)]
        keep_original: bool,
    },
    /// Materialize a managed skill into a target directory.
    Project {
        slug: String,
        /// Target id, e.g. `heron:global` or `claude:global` (defaults to the default target).
        #[arg(long)]
        on: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a projection (the store copy is kept).
    Unproject {
        slug: String,
        #[arg(long)]
        on: Option<String>,
    },
    /// Show where skills are projected and whether a client would load them.
    Verify { slug: Option<String> },
    /// Read-only health check.
    Doctor,
    /// List targets known to the index.
    Targets,
}

pub fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let dirs = heron_platform::dirs::Dirs::resolve_with_override("heron", cli.data_dir.as_deref())
        .map_err(HeronError::from)?;
    dirs.ensure().map_err(HeronError::from)?;
    let home = home_dir()?;
    let db = Db::open(&dirs.db_path())?;
    let mut ctx = Ctx::new(dirs, db, "cli");
    heron_service::clients::bootstrap(&mut ctx.db, &home)?;

    match cli.command {
        Command::Scan {
            roots,
            max_dirs,
            verbose,
        } => cmd_scan(&mut ctx, &home, roots, max_dirs, verbose, cli.json),
        Command::List => cmd_list(&ctx, cli.json),
        Command::Detected { state, all } => cmd_detected(&ctx, &state, all, cli.json),
        Command::Adopt {
            targets,
            all,
            dry_run,
            keep_original,
        } => cmd_adopt(&mut ctx, &targets, all, dry_run, keep_original, cli.json),
        Command::Project {
            slug,
            on,
            mode,
            dry_run,
        } => cmd_project(&mut ctx, &slug, on, mode, dry_run, cli.json),
        Command::Unproject { slug, on } => cmd_unproject(&mut ctx, &slug, on),
        Command::Verify { slug } => cmd_verify(&ctx, slug.as_deref(), cli.json),
        Command::Doctor => cmd_doctor(&ctx, cli.json),
        Command::Targets => cmd_targets(&ctx, cli.json),
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
    ctx: &mut Ctx,
    home: &Path,
    roots: Vec<PathBuf>,
    max_dirs: Option<usize>,
    verbose: bool,
    json: bool,
) -> Result<ExitCode> {
    let req = ScanRequest {
        extra_roots: roots,
        max_dirs,
        extra_excludes: vec![],
    };
    let report = scan::run(ctx, home, &req)?;
    if verbose && !json {
        for row in detected::list_by_state(ctx.db.conn(), "candidate")? {
            let name = row.observed_slug.clone().unwrap_or_else(|| "?".to_owned());
            let shown = heron_platform::paths::display(Path::new(&row.path));
            println!("  {name:<28} {:<16} {shown}", row.kind);
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!(
            "scanned {} directories in {} ms",
            report.dirs_visited, report.duration_ms
        );
        println!("  skills          {}", report.skills);
        println!("  legacy commands {}", report.legacy_commands);
        println!(
            "  nested suspects {}   (reported only - nothing is moved)",
            report.nested_suspects
        );
        println!("  needs attention {}", report.needs_attention);
        for missing in &report.roots_missing {
            println!("  skipped (not found) {missing}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_list(ctx: &Ctx, json: bool) -> Result<ExitCode> {
    let skills = skill::list(ctx.db.conn(), false)?;
    let all = detected::list_all(ctx.db.conn())?;
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
            println!("{:<28} {:<12} {}", s.slug, s.origin, s.display_name);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_detected(ctx: &Ctx, state: &str, all: bool, json: bool) -> Result<ExitCode> {
    let rows = if state == "all" {
        detected::list_all(ctx.db.conn())?
    } else {
        detected::list_by_state(ctx.db.conn(), state)?
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

fn cmd_adopt(
    ctx: &mut Ctx,
    wanted: &[String],
    all: bool,
    dry_run: bool,
    keep_original: bool,
    json: bool,
) -> Result<ExitCode> {
    let candidates = detected::list_by_state(ctx.db.conn(), "candidate")?;
    let selected: Vec<_> = if all {
        candidates
            .into_iter()
            .filter(|r| r.kind != "nested-suspect" && r.observed_slug.is_some())
            .collect()
    } else if wanted.is_empty() {
        return Err(HeronError::from(
            ErrorCode::SlugInvalid
                .msg()
                .with("detail", "specify a slug/path or --all"),
        ));
    } else {
        candidates
            .into_iter()
            .filter(|r| {
                wanted.iter().any(|w| {
                    r.observed_slug.as_deref() == Some(w.as_str())
                        || r.path == *w
                        || r.path.ends_with(&format!("/{w}/SKILL.md"))
                })
            })
            .collect()
    };

    if selected.is_empty() {
        println!("nothing to adopt (no matching detected skills)");
        return Ok(ExitCode::SUCCESS);
    }

    // Planning is stateful (dedup against the library), so plan sequentially and simulate each
    // planned slug so that two identical copies in one batch are reported as one adoption.
    if dry_run {
        let mut virtual_library: Vec<String> = Vec::new();
        let mut plans = Vec::new();
        for row in &selected {
            let plan = adopt::plan(ctx, row)?;
            let duplicate = plan
                .duplicate_of
                .clone()
                .or_else(|| plan.slug.clone().filter(|s| virtual_library.contains(s)));
            if let (Some(slug), None) = (plan.slug.clone(), duplicate.as_ref()) {
                virtual_library.push(slug);
            }
            plans.push((plan, duplicate));
        }
        for (plan, duplicate) in &plans {
            let slug = plan.slug.clone().unwrap_or_else(|| "?".into());
            let dup = duplicate
                .as_ref()
                .map(|d| format!(" (same content as {d}: no second copy)"))
                .unwrap_or_default();
            println!("would adopt {slug}{dup}");
            if duplicate.is_none() {
                for step in &plan.steps {
                    println!("  - {}", serde_json::to_string(step).unwrap());
                }
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    let placement = if keep_original {
        AdoptPlacement::KeepOriginal
    } else {
        AdoptPlacement::Replace
    };
    let mut adopted = Vec::new();
    // Re-plan per item so dedup sees what earlier iterations adopted.
    for row in &selected {
        let plan = adopt::plan(ctx, row)?;
        let report = adopt::execute(ctx, &plan, placement)?;
        adopted.extend(report.adopted);
        for skipped in report.skipped {
            println!("skipped: {skipped}");
        }
        for error in report.errors {
            println!("error: {error}");
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&adopted).unwrap());
    } else {
        for slug in &adopted {
            println!("adopted {slug}");
        }
        if !adopted.is_empty() {
            println!();
            println!("next: heron project <slug>   (or --on claude:global for a specific client)");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_mode(raw: Option<String>) -> Result<Option<ProjectionMode>> {
    raw.map(|m| match m.as_str() {
        "copy" => Ok(ProjectionMode::Copy),
        "symlink" => Ok(ProjectionMode::Symlink),
        "junction" => Ok(ProjectionMode::Junction),
        "pointer" => Ok(ProjectionMode::Pointer),
        other => Err(HeronError::from(
            ErrorCode::SlugInvalid.msg().with("raw", other),
        )),
    })
    .transpose()
}

fn resolve_target(ctx: &Ctx, on: Option<String>) -> Result<String> {
    match on {
        Some(id) => Ok(id),
        None => target::get_default(ctx.db.conn())?
            .map(|t| t.id)
            .ok_or_else(|| {
                HeronError::from(ErrorCode::TargetNotFound.msg().with("target", "default"))
            }),
    }
}

fn cmd_project(
    ctx: &mut Ctx,
    slug: &str,
    on: Option<String>,
    mode: Option<String>,
    dry_run: bool,
    json: bool,
) -> Result<ExitCode> {
    let slug = project::managed_slug(ctx, slug)?;
    let req = ProjectRequest {
        slug: slug.clone(),
        target_id: resolve_target(ctx, on)?,
        mode: parse_mode(mode)?,
        link_name: None,
    };
    let (plan, request) = project::plan(ctx, &req)?;
    if dry_run {
        for step in &plan.steps {
            println!("{}", serde_json::to_string(step).unwrap());
        }
        if plan.is_blocked() {
            return Ok(ExitCode::from(3));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let outcome = project::apply(ctx, &req, &plan, &request)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
    } else if outcome.applied {
        println!(
            "projected {} -> {} [{}]",
            outcome.slug, outcome.path, outcome.mode
        );
    } else {
        println!("{} already projected at {}", outcome.slug, outcome.path);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_unproject(ctx: &mut Ctx, slug: &str, on: Option<String>) -> Result<ExitCode> {
    let slug = project::managed_slug(ctx, slug)?;
    let target = resolve_target(ctx, on)?;
    project::unproject(ctx, &slug, &target)?;
    println!("removed projections of {slug} from {target} (store copy kept)");
    Ok(ExitCode::SUCCESS)
}

fn cmd_verify(ctx: &Ctx, slug: Option<&str>, json: bool) -> Result<ExitCode> {
    let reports = verify::verify(ctx, slug)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    if reports.is_empty() {
        println!("no projections yet - use `heron project <slug>`");
        return Ok(ExitCode::SUCCESS);
    }
    for report in &reports {
        println!(
            "{:<28} {:<16} {:<9} {}",
            report.slug,
            report.target_id,
            report.mode,
            verify::describe(&report.visibility)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_doctor(ctx: &Ctx, json: bool) -> Result<ExitCode> {
    let report = doctor::run(ctx)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    if report.issues.is_empty() {
        println!("no issues found");
        return Ok(ExitCode::SUCCESS);
    }
    for issue in &report.issues {
        println!(
            "{:<10} {:<20} {}",
            format!("{:?}", issue.severity).to_lowercase(),
            issue.id,
            issue.subject
        );
    }
    let code = if report.count(doctor::Severity::Error) > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(code)
}

fn cmd_targets(ctx: &Ctx, json: bool) -> Result<ExitCode> {
    let targets = target::list(ctx.db.conn())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&targets).unwrap());
    } else {
        for t in targets {
            let star = if t.is_default { "*" } else { " " };
            println!(
                "{star} {:<22} {:<10} {}",
                t.id,
                t.kind,
                heron_platform::paths::display(Path::new(&t.path))
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
