//! Argument parsing and dispatch. Every command delegates to `heron-service`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use heron_core::{ErrorCode, HeronError, ProjectionMode, Result};
use heron_service::adopt::{self, AdoptPlacement};
use heron_service::install::{self, InstallRequest};
use heron_service::project::{self, ProjectRequest};
use heron_service::scan::{self, ScanRequest};
use heron_service::upgrade::{self, UpgradeOutcome, UpgradeState};
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
    /// Install a skill from a repository URL, \`owner/repo\` or tree link.
    Install {
        input: String,
        #[arg(long = "ref")]
        ref_override: Option<String>,
        #[arg(long)]
        subpath: Option<String>,
        #[arg(long = "as")]
        as_slug: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// List managed skills whose upstream has moved on.
    Outdated,
    /// Merge upstream changes into local skills.
    Upgrade {
        /// Slugs to upgrade; omit to upgrade everything with an update.
        slugs: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        dry_run: bool,
        /// Discard local edits and take the upstream snapshot wholesale, instead of three-way
        /// merging them. The inverse of 设置 → 通用 → 更新时保留本地改动.
        #[arg(long)]
        overwrite: bool,
    },
    /// Return a skill to an earlier landed version.
    Rollback {
        slug: String,
        #[arg(long)]
        to: String,
    },
    /// Show local changes (or the diff against a version).
    Diff {
        slug: String,
        #[arg(long)]
        against: Option<String>,
    },
    /// Show the two sides of an upgrade conflict.
    Conflict { slug: String },
    /// Resolve a conflict by taking one side for every file.
    Resolve {
        slug: String,
        /// `ours` keeps the local content, `theirs` takes upstream for every conflicting file.
        #[arg(long, default_value = "theirs")]
        take: String,
    },
    /// Manage skill markets (search + install plan).
    Market {
        #[command(subcommand)]
        action: MarketCmd,
    },
    /// Team workflow: export / import a reproducible skill set.
    Lock {
        #[command(subcommand)]
        action: LockCmd,
    },
    /// Discover project roots that contain client skill directories.
    Projects {
        /// Roots to scan for projects (defaults to the home directory's first level).
        #[arg(long = "roots")]
        roots: Vec<PathBuf>,
        /// Register what is found (recorded disabled until enabled).
        #[arg(long)]
        register: bool,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum MarketCmd {
    /// List the built-in markets and their capability.
    List,
    /// Search one market.
    Search {
        /// Market id (see `heron market list`).
        market: String,
        /// What to look for.
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show how an item would be installed.
    Plan { market: String, key: String },
}

#[derive(Subcommand, Debug)]
pub enum LockCmd {
    /// Write heron.lock from what is installed.
    Export {
        /// Directory that receives `heron.lock`.
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Print the lock instead of writing it (for piping / review).
        #[arg(long, conflicts_with = "dir")]
        print: bool,
    },
    /// Compare heron.lock with the local store.
    Diff {
        #[arg(long, default_value = "heron.lock")]
        file: PathBuf,
    },
    /// Install whatever the lock lists but the store lacks.
    Apply {
        #[arg(long, default_value = "heron.lock")]
        file: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
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
        Command::Verify { slug } => cmd_verify(&mut ctx, slug.as_deref(), cli.json),
        Command::Doctor => cmd_doctor(&ctx, cli.json),
        Command::Targets => cmd_targets(&ctx, cli.json),
        Command::Install {
            input,
            ref_override,
            subpath,
            as_slug,
            dry_run,
        } => cmd_install(
            &mut ctx,
            input,
            ref_override,
            subpath,
            as_slug,
            dry_run,
            cli.json,
        ),
        Command::Outdated => cmd_outdated(&ctx, cli.json),
        Command::Upgrade {
            slugs,
            all,
            dry_run,
            overwrite,
        } => cmd_upgrade(&mut ctx, &slugs, all, dry_run, overwrite, cli.json),
        Command::Rollback { slug, to } => {
            let previous = upgrade::rollback(&mut ctx, &slug, &to)?;
            println!(
                "rolled {slug} back to {to} (was {})",
                &previous[..8.min(previous.len())]
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Diff { slug, against } => cmd_diff(&ctx, &slug, against, cli.json),
        Command::Conflict { slug } => cmd_conflict(&ctx, &slug, cli.json),
        Command::Resolve { slug, take } => cmd_resolve(&mut ctx, &slug, &take, cli.json),
        Command::Market { action } => cmd_market(action, cli.json),
        Command::Lock { action } => cmd_lock(&mut ctx, action, cli.json),
        Command::Projects {
            roots,
            register,
            depth,
        } => cmd_projects(&mut ctx, &home, roots, register, depth, cli.json),
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

fn cmd_verify(ctx: &mut Ctx, slug: Option<&str>, json: bool) -> Result<ExitCode> {
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

fn cmd_install(
    ctx: &mut Ctx,
    input: String,
    ref_override: Option<String>,
    subpath: Option<String>,
    as_slug: Option<String>,
    dry_run: bool,
    json: bool,
) -> Result<ExitCode> {
    let req = InstallRequest {
        input,
        ref_override,
        subpath_override: subpath,
        as_slug,
    };
    let preview = install::preview(ctx, &req)?;
    if dry_run {
        if json {
            println!("{}", serde_json::to_string_pretty(&preview).unwrap());
        } else {
            println!("source   {}", heron_source::url::describe(&preview.source));
            println!("skill    {}", preview.selected.as_deref().unwrap_or("-"));
            println!("slug     {}", preview.slug);
            println!(
                "commit   {}",
                &preview.commit[..8.min(preview.commit.len())]
            );
            if preview.available.len() > 1 {
                println!("also available:");
                for item in preview
                    .available
                    .iter()
                    .filter(|p| Some(*p) != preview.selected.as_ref())
                {
                    println!("  {item}");
                }
            }
            if preview.existing {
                println!(
                    "note     already installed - use heron upgrade {}",
                    preview.slug
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    let report = install::install(ctx, &req)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!(
            "installed {} {} from {}",
            report.slug, report.version, report.source
        );
        println!("  at {}", report.path);
        println!();
        println!("next: heron project {}", report.slug);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_outdated(ctx: &Ctx, json: bool) -> Result<ExitCode> {
    upgrade::refresh_sources(ctx)?;
    let candidates = upgrade::outdated(ctx, heron_core::PinPolicy::Semver)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&candidates).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    let mut any = false;
    for c in &candidates {
        let state = match c.state {
            UpgradeState::HasUpdate => "update available",
            UpgradeState::UpToDate => "up to date",
            UpgradeState::Pinned => "pinned",
            UpgradeState::LocalModified => "local changes",
            UpgradeState::Diverged => "diverged (local + upstream)",
            UpgradeState::UpstreamGone => "upstream gone",
            UpgradeState::NotTracked => "not tracked",
        };
        println!("{:<28} {:<28} {}", c.slug, state, c.installed);
        any |= matches!(c.state, UpgradeState::HasUpdate | UpgradeState::Diverged);
    }
    if !any {
        println!();
        println!("everything is up to date");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_upgrade(
    ctx: &mut Ctx,
    slugs: &[String],
    all: bool,
    dry_run: bool,
    overwrite: bool,
    json: bool,
) -> Result<ExitCode> {
    upgrade::refresh_sources(ctx)?;
    let candidates = upgrade::outdated(ctx, heron_core::PinPolicy::Semver)?;
    let selected: Vec<String> = if all || slugs.is_empty() {
        candidates
            .iter()
            .filter(|c| matches!(c.state, UpgradeState::HasUpdate | UpgradeState::Diverged))
            .map(|c| c.slug.clone())
            .collect()
    } else {
        slugs.to_vec()
    };

    if selected.is_empty() {
        println!("nothing to upgrade");
        return Ok(ExitCode::SUCCESS);
    }
    if dry_run {
        for slug in &selected {
            let cand = candidates.iter().find(|c| &c.slug == slug);
            let latest = cand.and_then(|c| c.latest.clone()).unwrap_or_default();
            println!("would upgrade {slug} -> {}", &latest[..8.min(latest.len())]);
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut conflicts = 0;
    let mut outcomes = Vec::new();
    for slug in &selected {
        let outcome = upgrade::upgrade(ctx, slug, !overwrite)?;
        if let UpgradeOutcome::Conflicts { .. } = &outcome {
            conflicts += 1;
        }
        outcomes.push(outcome);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&outcomes).unwrap());
    } else {
        for outcome in &outcomes {
            match outcome {
                UpgradeOutcome::Upgraded { slug, from, to, .. } => {
                    println!(
                        "upgraded {slug} {} -> {}",
                        &from[..8.min(from.len())],
                        &to[..8.min(to.len())]
                    )
                }
                UpgradeOutcome::UpToDate { slug } => println!("{slug} already up to date"),
                UpgradeOutcome::Skipped { slug, reason } => println!("skipped {slug}: {reason}"),
                UpgradeOutcome::Conflicts { slug, files } => {
                    println!("conflict in {slug}:");
                    for file in files {
                        println!("  {}", file.path);
                    }
                    println!("  the skill was left untouched; resolve it manually or roll back");
                }
            }
        }
    }
    Ok(if conflicts > 0 {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}

fn cmd_market(action: MarketCmd, json: bool) -> Result<ExitCode> {
    match action {
        MarketCmd::List => {
            let rows: Vec<_> = heron_source::MARKETS
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "name": m.name,
                        "capability": m.capability,
                        "enabledByDefault": m.enabled_by_default,
                        "url": m.base_url,
                    })
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else {
                for m in heron_source::MARKETS {
                    let cap = match m.capability {
                        heron_source::Capability::Indexed => "searchable",
                        heron_source::Capability::Guided => "browser only",
                    };
                    let flag = if m.enabled_by_default { " " } else { "*" };
                    println!(
                        "{flag} {:<12} {:<10} {:<14} {}",
                        m.id, cap, m.name, m.base_url
                    );
                }
                println!();
                println!("* = disabled by default (decision D1)");
            }
        }
        MarketCmd::Search {
            market,
            query,
            limit,
        } => {
            let adapter = heron_source::adapter(&market)?;
            let items = adapter.search(&query, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else if items.is_empty() {
                println!("no results for {query}");
            } else {
                for item in &items {
                    let installable = matches!(
                        adapter.install_plan(&item.key),
                        Ok(heron_source::InstallPlan::Git { .. })
                    );
                    let mark = if installable { "+" } else { "~" };
                    println!(
                        "{mark} {:<40} {}",
                        item.key,
                        item.summary.chars().take(60).collect::<String>()
                    );
                    println!("  {}", item.url);
                }
                println!();
                println!("+ installable by Heron   ~ open in browser");
            }
        }
        MarketCmd::Plan { market, key } => {
            let adapter = heron_source::adapter(&market)?;
            let plan = adapter.install_plan(&key)?;
            match plan {
                heron_source::InstallPlan::Git {
                    ref url,
                    ref subpath,
                    ..
                } => {
                    println!("heron install {url} --subpath {subpath}");
                }
                heron_source::InstallPlan::Guided { ref open_url, .. } => {
                    println!("open {open_url}");
                    println!("then paste the skill's git URL back: heron install <url>");
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_lock(ctx: &mut Ctx, action: LockCmd, json: bool) -> Result<ExitCode> {
    match action {
        LockCmd::Export { dir, print } => {
            let lock = heron_service::lockfile::export(ctx)?;
            if print {
                println!("{}", serde_json::to_string_pretty(&lock).unwrap());
            } else {
                // `--dir` receives the file, never becomes one itself.
                let target = dir.join(heron_service::lockfile::LOCK_FILE_NAME);
                let path = heron_service::lockfile::write(&target, &lock)?;
                println!("wrote {} ({} skills)", path.display(), lock.skills.len());
            }
        }
        LockCmd::Diff { file } => {
            let lock = heron_service::lockfile::read(&file)?;
            let drift = heron_service::lockfile::diff(ctx, &lock)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&drift).unwrap());
            } else if drift.is_empty() {
                println!(
                    "in sync with {} ({} skills)",
                    file.display(),
                    lock.skills.len()
                );
            } else {
                for item in &drift {
                    println!("{:<28} {:?}", item.slug, item.kind);
                }
            }
        }
        LockCmd::Apply { file, dry_run } => {
            let lock = heron_service::lockfile::read(&file)?;
            if dry_run {
                let drift = heron_service::lockfile::diff(ctx, &lock)?;
                for item in &drift {
                    match item.kind {
                        heron_service::lockfile::DriftKind::Missing => {
                            let entry = &lock.skills[&item.slug];
                            println!("would install {} from {}", item.slug, entry.source);
                        }
                        kind => println!("{} {:?} (needs attention)", item.slug, kind),
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
            let report = heron_service::lockfile::apply(ctx, &lock)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!(
                    "installed {}  up to date {}  drift {}",
                    report.installed.len(),
                    report.up_to_date.len(),
                    report.drift.len()
                );
                for d in &report.drift {
                    println!("  drift: {} {:?}", d.slug, d.kind);
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_projects(
    ctx: &mut Ctx,
    home: &Path,
    roots: Vec<PathBuf>,
    register: bool,
    depth: usize,
    json: bool,
) -> Result<ExitCode> {
    let scan_roots = if roots.is_empty() {
        vec![home.to_path_buf()]
    } else {
        roots
    };
    let found = heron_service::projects::discover(home, &scan_roots, depth);
    if json {
        println!("{}", serde_json::to_string_pretty(&found).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    if found.is_empty() {
        println!("no project roots with client skill directories found");
        return Ok(ExitCode::SUCCESS);
    }
    for project in &found {
        let shown = heron_platform::paths::display(&project.root);
        println!("{:<48} {}", shown, project.clients.join(", "));
    }
    if register {
        let mut registered = 0;
        for project in &found {
            if heron_service::projects::register(ctx, &project.root, &project.clients)? {
                registered += 1;
            }
        }
        println!();
        println!(
            "registered {registered} project(s); projections into them stay off until enabled"
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_diff(ctx: &Ctx, slug: &str, against: Option<String>, json: bool) -> Result<ExitCode> {
    let report = heron_service::diff::diff_skill(ctx, slug, against.as_deref())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    if report.files.is_empty() {
        println!("{slug} matches {}\n", report.against);
        return Ok(ExitCode::SUCCESS);
    }
    for file in &report.files {
        println!("✎ {}", file.path);
        let before = file.before.clone().unwrap_or_default();
        let after = file.after.clone().unwrap_or_default();
        for line in after.lines() {
            if !before.lines().any(|b| b == line) {
                println!("  + {line}");
            }
        }
        for line in before.lines() {
            if !after.lines().any(|a| a == line) {
                println!("  - {line}");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_conflict(ctx: &Ctx, slug: &str, json: bool) -> Result<ExitCode> {
    let payload = heron_service::diff::conflict_payload(ctx, slug)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return Ok(ExitCode::SUCCESS);
    }
    println!("{slug}: {} conflicting file(s)", payload.files.len());
    for file in &payload.files {
        println!("  {}", file.path);
    }
    println!();
    println!("resolve with: heron resolve {slug} --take theirs   (or --take ours)");
    Ok(ExitCode::SUCCESS)
}

fn cmd_resolve(ctx: &mut Ctx, slug: &str, take: &str, json: bool) -> Result<ExitCode> {
    let payload = heron_service::diff::conflict_payload(ctx, slug)?;
    let resolved: Vec<(String, String)> = payload
        .files
        .iter()
        .map(|file| {
            let content = match take {
                "ours" => file.before.clone().unwrap_or_default(),
                _ => file.after.clone().unwrap_or_default(),
            };
            (file.path.clone(), content)
        })
        .collect();
    let outcome = heron_service::diff::resolve(ctx, slug, &resolved)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
    } else {
        match &outcome {
            heron_service::upgrade::UpgradeOutcome::Upgraded { to, .. } => {
                println!("resolved {slug} and upgraded to {}", &to[..8.min(to.len())])
            }
            heron_service::upgrade::UpgradeOutcome::Conflicts { files, .. } => {
                println!(
                    "resolved {slug}, but {} file(s) still conflict",
                    files.len()
                )
            }
            other => println!("{other:?}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}
