//! Tauri shell.
//!
//! This crate deliberately contains **no business logic**: every command validates nothing and
//! delegates to `heron-service`, which is also what the CLI uses. That keeps the GUI and the CLI
//! behaviourally identical (implementation doc §7.5).

use std::path::PathBuf;
use std::sync::Mutex;

use heron_core::{HeronError, Msg};
use heron_platform::dirs::Dirs;
use heron_service::install::InstallRequest;
use heron_service::project::ProjectRequest;
use heron_service::scan::ScanRequest;
use heron_service::{adopt, doctor, install, upgrade, verify, Ctx};
use serde::Serialize;
use tauri::State;

struct AppState {
    ctx: Mutex<Ctx>,
    home: PathBuf,
}

fn err(e: HeronError) -> Msg {
    e.as_msg()
}

fn msg(e: Msg) -> Msg {
    e
}

#[derive(Serialize)]
struct GitInfoDto {
    version: String,
}

#[tauri::command]
fn preflight() -> serde_json::Value {
    let pre = heron_platform::probe::preflight();
    serde_json::json!({
        "git": pre.git.map(|g| GitInfoDto { version: g.version }),
        "platform": heron_core::PlatformKind::current(),
    })
}

#[tauri::command]
fn scan(
    state: State<'_, AppState>,
    roots: Option<Vec<String>>,
) -> Result<heron_service::scan::ScanReport, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let request = ScanRequest {
        extra_roots: roots
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ..ScanRequest::default()
    };
    heron_service::scan::run(&mut ctx, &state.home, &request).map_err(err)
}

#[tauri::command]
fn detected_list(
    state: State<'_, AppState>,
    state_filter: Option<String>,
) -> Result<Vec<heron_store::models::DetectedRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    let wanted = state_filter.unwrap_or_else(|| "candidate".to_owned());
    let rows = if wanted == "all" {
        heron_store::repos::detected::list_all(ctx.db.conn())
    } else {
        heron_store::repos::detected::list_by_state(ctx.db.conn(), &wanted)
    };
    rows.map_err(err)
}

#[tauri::command]
fn adopt_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
    dry_run: bool,
    keep_original: bool,
) -> Result<serde_json::Value, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let rows =
        heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate").map_err(err)?;
    let mut plans = Vec::new();
    for path in &paths {
        if let Some(row) = rows.iter().find(|r| &r.path == path) {
            plans.push(adopt::plan(&ctx, row).map_err(err)?);
        }
    }
    if dry_run {
        return serde_json::to_value(&plans)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
    }
    let placement = if keep_original {
        adopt::AdoptPlacement::KeepOriginal
    } else {
        adopt::AdoptPlacement::Replace
    };
    let mut report = adopt::AdoptReport {
        adopted: vec![],
        skipped: vec![],
        errors: vec![],
    };
    for plan in &plans {
        let outcome = adopt::execute(&mut ctx, plan, placement).map_err(err)?;
        report.adopted.extend(outcome.adopted);
        report.skipped.extend(outcome.skipped);
        report.errors.extend(outcome.errors);
    }
    serde_json::to_value(&report).map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
}

#[tauri::command]
fn skills_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::SkillRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_store::repos::skill::list(ctx.db.conn(), false).map_err(err)
}

#[tauri::command]
fn targets_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::TargetRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_store::repos::target::list(ctx.db.conn()).map_err(err)
}

#[tauri::command]
fn project_skill(
    state: State<'_, AppState>,
    slug: String,
    target_id: Option<String>,
    mode: Option<String>,
    dry_run: bool,
) -> Result<serde_json::Value, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let mode = match mode.as_deref() {
        Some("copy") => Some(heron_core::ProjectionMode::Copy),
        Some("symlink") => Some(heron_core::ProjectionMode::Symlink),
        Some("junction") => Some(heron_core::ProjectionMode::Junction),
        Some("pointer") => Some(heron_core::ProjectionMode::Pointer),
        _ => None,
    };
    let resolved_target = match target_id {
        Some(t) => t,
        None => heron_store::repos::target::get_default(ctx.db.conn())
            .map_err(err)?
            .map(|t| t.id)
            .ok_or_else(|| Msg::new("E_TARGET_NOT_FOUND").with("target", "default"))?,
    };
    let request = ProjectRequest {
        slug,
        target_id: resolved_target,
        mode,
        link_name: None,
    };
    let (plan, projection) = heron_service::project::plan(&ctx, &request).map_err(err)?;
    if dry_run {
        return Ok(serde_json::json!({ "plan": plan }));
    }
    let outcome =
        heron_service::project::apply(&mut ctx, &request, &plan, &projection).map_err(err)?;
    serde_json::to_value(&outcome).map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
}

#[tauri::command]
fn unproject_skill(state: State<'_, AppState>, slug: String, target_id: String) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::project::unproject(&mut ctx, &slug, &target_id).map_err(err)
}

#[tauri::command]
fn verify_visibility(state: State<'_, AppState>) -> Result<Vec<verify::VerifyReport>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    verify::verify(&ctx, None).map_err(err)
}

#[tauri::command]
fn outdated_list(state: State<'_, AppState>) -> Result<Vec<upgrade::UpgradeCandidate>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    upgrade::refresh_sources(&ctx).map_err(err)?;
    upgrade::outdated(&ctx, heron_core::PinPolicy::Semver).map_err(err)
}

#[tauri::command]
fn upgrade_skills(
    state: State<'_, AppState>,
    slugs: Vec<String>,
    dry_run: bool,
) -> Result<Vec<upgrade::UpgradeOutcome>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    if dry_run {
        return Ok(vec![]);
    }
    let mut outcomes = Vec::new();
    for slug in &slugs {
        outcomes.push(upgrade::upgrade(&mut ctx, slug).map_err(err)?);
    }
    Ok(outcomes)
}

#[tauri::command]
fn rollback_skill(state: State<'_, AppState>, slug: String, to: String) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    upgrade::rollback(&mut ctx, &slug, &to).map_err(err)?;
    Ok(())
}

#[tauri::command]
fn doctor_run(state: State<'_, AppState>) -> Result<doctor::DoctorReport, Msg> {
    let ctx = state.ctx.lock().unwrap();
    doctor::run(&ctx).map_err(err)
}

#[tauri::command]
fn markets_list() -> Vec<serde_json::Value> {
    heron_source::MARKETS
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "name": m.name,
                "capability": m.capability,
                "enabled_by_default": m.enabled_by_default,
                "base_url": m.base_url,
            })
        })
        .collect()
}

#[tauri::command]
fn market_search(
    market: String,
    query: String,
) -> Result<Vec<heron_source::market::MarketItem>, Msg> {
    let adapter = heron_source::adapter(&market).map_err(err)?;
    adapter.search(&query, 20).map_err(err)
}

#[tauri::command]
fn market_plan(market: String, key: String) -> Result<heron_source::market::InstallPlan, Msg> {
    let adapter = heron_source::adapter(&market).map_err(err)?;
    adapter.install_plan(&key).map_err(err)
}

#[tauri::command]
fn install_skill(
    state: State<'_, AppState>,
    input: String,
    ref_override: Option<String>,
    subpath: Option<String>,
    as_slug: Option<String>,
    dry_run: bool,
) -> Result<serde_json::Value, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let request = InstallRequest {
        input,
        ref_override,
        subpath_override: subpath,
        as_slug,
    };
    if dry_run {
        let preview = install::preview(&ctx, &request).map_err(err)?;
        return serde_json::to_value(preview)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
    }
    let report = install::install(&mut ctx, &request).map_err(err)?;
    serde_json::to_value(report).map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
}

#[tauri::command]
fn lock_export(
    state: State<'_, AppState>,
    dir: String,
    write: bool,
) -> Result<serde_json::Value, Msg> {
    let ctx = state.ctx.lock().unwrap();
    let lock = heron_service::lockfile::export(&ctx).map_err(err)?;
    if !write {
        return serde_json::to_value(&lock)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
    }
    let target = PathBuf::from(dir).join(heron_service::lockfile::LOCK_FILE_NAME);
    let path = heron_service::lockfile::write(&target, &lock).map_err(err)?;
    Ok(serde_json::json!({ "path": path.to_string_lossy() }))
}

#[tauri::command]
fn lock_diff(
    state: State<'_, AppState>,
    file: String,
) -> Result<Vec<heron_service::lockfile::Drift>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    let lock = heron_service::lockfile::read(&PathBuf::from(file)).map_err(err)?;
    heron_service::lockfile::diff(&ctx, &lock).map_err(err)
}

#[tauri::command]
fn projects_discover(
    state: State<'_, AppState>,
    roots: Vec<String>,
    register: bool,
) -> Result<Vec<heron_service::projects::DiscoveredProject>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let scan_roots: Vec<PathBuf> = if roots.is_empty() {
        vec![state.home.clone()]
    } else {
        roots.into_iter().map(PathBuf::from).collect()
    };
    let found = heron_service::projects::discover(&state.home, &scan_roots, 3);
    if register {
        for project in &found {
            heron_service::projects::register(&mut ctx, &project.root, &project.clients)
                .map_err(err)?;
        }
    }
    Ok(found)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dirs = match Dirs::resolve_with_override("heron", None) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("heron: cannot resolve data directory: {e}");
            std::process::exit(1);
        }
    };
    let _ = dirs.ensure();
    let db = heron_store::db::Db::open(&dirs.db_path()).expect("index database");
    let mut ctx = Ctx::new(dirs, db, "gui");
    let _ = heron_service::clients::bootstrap(&mut ctx.db, &home);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            ctx: Mutex::new(ctx),
            home,
        })
        .invoke_handler(tauri::generate_handler![
            preflight,
            scan,
            detected_list,
            adopt_paths,
            skills_list,
            targets_list,
            project_skill,
            unproject_skill,
            verify_visibility,
            outdated_list,
            upgrade_skills,
            rollback_skill,
            doctor_run,
            markets_list,
            market_search,
            market_plan,
            install_skill,
            lock_export,
            lock_diff,
            projects_discover,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Heron");
}

#[allow(dead_code)]
fn _msg(m: Msg) -> Msg {
    msg(m)
}
