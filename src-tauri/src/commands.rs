//! Thin Tauri IPC commands delegating 1:1 to `heron-service` (ADR-0010).

use std::path::PathBuf;

use heron_core::Msg;
use heron_service::install::InstallRequest;
use heron_service::project::ProjectRequest;
use heron_service::{adopt, install, upgrade};
use serde::Serialize;
use tauri::{Manager, State};

use crate::state::{err, with_ctx, AppState};

#[derive(Serialize)]
struct GitInfoDto {
    version: String,
}

/// Environment probe run once on UI boot: git availability, platform, and user home directory.
#[tauri::command]
pub(crate) fn preflight(state: State<'_, AppState>) -> serde_json::Value {
    let pre = heron_platform::probe::preflight();
    serde_json::json!({
        "git": pre.git.map(|g| GitInfoDto { version: g.version }),
        "platform": heron_core::PlatformKind::current(),
        "home": state.home.to_string_lossy(),
    })
}

#[tauri::command]
pub(crate) fn detected_list(
    state: State<'_, AppState>,
    state_filter: Option<String>,
) -> Result<Vec<heron_store::models::DetectedRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::scan::list_detected(&ctx, state_filter.as_deref()).map_err(err)
}

#[tauri::command]
pub(crate) fn detected_meta(paths: Vec<String>) -> Vec<heron_service::scan::DetectedMeta> {
    heron_service::scan::describe(&paths)
}

/// Preview or execute batch adoption off the UI thread.
#[tauri::command]
pub(crate) async fn adopt_paths(
    app: tauri::AppHandle,
    paths: Vec<String>,
    dry_run: bool,
    keep_original: bool,
    placement: Option<String>,
) -> Result<serde_json::Value, Msg> {
    let work = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut ctx = state.ctx.lock().unwrap();
        let plans = adopt::plan_paths(&ctx, &paths).map_err(err)?;
        if dry_run {
            return serde_json::to_value(&plans)
                .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
        }
        let parsed_placement = match placement.as_deref() {
            Some(value) => Some(adopt::AdoptPlacement::parse(value).ok_or_else(|| {
                Msg::new("E_INTERNAL").with("detail", format!("unknown placement {value}"))
            })?),
            None => None,
        };
        let report = adopt::execute_paths(&mut ctx, &plans, keep_original, parsed_placement);
        serde_json::to_value(&report)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
    });
    work.await
        .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) async fn skills_list(
    app: tauri::AppHandle,
) -> Result<Vec<heron_store::models::SkillRow>, Msg> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut ctx = state.ctx.lock().unwrap();
        heron_service::adopt::list_skills(&mut ctx).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) fn targets_list(
    state: State<'_, AppState>,
) -> Result<Vec<heron_store::models::TargetRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::clients::list_targets(&ctx.db).map_err(err)
}

#[tauri::command]
pub(crate) fn clients_list(
    state: State<'_, AppState>,
) -> Result<Vec<heron_store::models::ClientRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::clients::list_clients(&ctx.db).map_err(err)
}

#[tauri::command]
pub(crate) fn project_skill(
    state: State<'_, AppState>,
    slug: String,
    target_id: String,
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
    let request = ProjectRequest {
        slug,
        target_id,
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
pub(crate) fn unproject_skill(
    state: State<'_, AppState>,
    slug: String,
    target_id: String,
) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::project::unproject(&mut ctx, &slug, &target_id).map_err(err)
}

#[tauri::command]
pub(crate) fn target_unproject_all(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<Vec<String>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::project::unproject_target(&mut ctx, &target_id).map_err(err)
}

#[tauri::command]
pub(crate) fn target_set_enabled(
    state: State<'_, AppState>,
    target_id: String,
    enabled: bool,
) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::clients::set_target_enabled(&mut ctx.db, &target_id, enabled).map_err(err)
}

#[tauri::command]
pub(crate) fn skill_remove(
    state: State<'_, AppState>,
    slug: String,
    dry_run: bool,
) -> Result<serde_json::Value, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let plan = heron_service::remove::plan(&ctx, &slug).map_err(err)?;
    if dry_run {
        return serde_json::to_value(&plan)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
    }
    let outcome = heron_service::remove::apply(&mut ctx, &plan).map_err(err)?;
    serde_json::to_value(&outcome).map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
}

/// Check for upstream updates, running git network fetches outside `state.ctx.lock()`.
#[tauri::command]
pub(crate) async fn outdated_list(
    app: tauri::AppHandle,
) -> Result<Vec<upgrade::UpgradeCandidate>, Msg> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (cache_root, remotes) = {
            let ctx = state.ctx.lock().unwrap();
            upgrade::collect_remotes(&ctx).map_err(err)?
        };
        upgrade::fetch_remotes(&cache_root, &remotes);
        let ctx = state.ctx.lock().unwrap();
        upgrade::outdated(&ctx, heron_core::PinPolicy::Semver).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) fn upgrade_skills(
    state: State<'_, AppState>,
    slugs: Vec<String>,
    dry_run: bool,
    keep_local: bool,
) -> Result<Vec<upgrade::UpgradeOutcome>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    if dry_run {
        return Ok(vec![]);
    }
    let mut outcomes = Vec::new();
    for slug in &slugs {
        outcomes.push(upgrade::upgrade(&mut ctx, slug, keep_local).map_err(err)?);
    }
    Ok(outcomes)
}

#[tauri::command]
pub(crate) fn rollback_skill(
    state: State<'_, AppState>,
    slug: String,
    to: String,
) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    upgrade::rollback(&mut ctx, &slug, &to).map_err(err)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn markets_list(
    app: tauri::AppHandle,
) -> Result<Vec<heron_service::markets::MarketInfo>, Msg> {
    with_ctx(app, |ctx| Ok(heron_service::markets::list(ctx))).await
}

#[tauri::command]
pub(crate) fn market_set_enabled(
    state: State<'_, AppState>,
    market: String,
    enabled: bool,
) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::markets::set_enabled(&mut ctx, &market, enabled).map_err(err)
}

#[tauri::command]
pub(crate) fn favorite_list(
    state: State<'_, AppState>,
) -> Result<Vec<heron_store::models::FavoriteRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::favorites::list(&ctx).map_err(err)
}

#[tauri::command]
pub(crate) fn favorite_add(
    state: State<'_, AppState>,
    name: String,
    url: String,
    description: String,
    market: Option<String>,
) -> Result<heron_store::models::FavoriteRow, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::favorites::add(&mut ctx, &name, &url, &description, market.as_deref())
        .map_err(err)
}

#[tauri::command]
pub(crate) fn favorite_update(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    url: String,
    description: String,
    market: Option<String>,
) -> Result<heron_store::models::FavoriteRow, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::favorites::update(&mut ctx, id, &name, &url, &description, market.as_deref())
        .map_err(err)
}

#[tauri::command]
pub(crate) fn favorite_remove(state: State<'_, AppState>, id: i64) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::favorites::remove(&mut ctx, id).map_err(err)
}

#[tauri::command]
pub(crate) async fn market_search(
    market: String,
    query: String,
) -> Result<Vec<heron_source::market::MarketItem>, Msg> {
    tauri::async_runtime::spawn_blocking(move || {
        heron_service::markets::search(&market, &query, 20).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) async fn market_plan(
    market: String,
    key: String,
) -> Result<heron_source::market::InstallPlan, Msg> {
    tauri::async_runtime::spawn_blocking(move || {
        heron_service::markets::install_plan(&market, &key).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) fn net_get_proxy(
    state: State<'_, AppState>,
) -> Result<heron_service::net::ProxyConfig, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::net::get(&ctx).map_err(err)
}

#[tauri::command]
pub(crate) fn net_set_proxy(
    state: State<'_, AppState>,
    url: Option<String>,
    enabled: bool,
) -> Result<heron_service::net::ProxyConfig, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::net::set(&ctx, url.as_deref(), enabled).map_err(err)
}

/// Preview or install a remote/local skill, running network prefetch outside `state.ctx.lock()`.
#[tauri::command]
pub(crate) async fn install_skill(
    app: tauri::AppHandle,
    input: String,
    ref_override: Option<String>,
    subpath: Option<String>,
    as_slug: Option<String>,
    dry_run: bool,
) -> Result<serde_json::Value, Msg> {
    let request = InstallRequest {
        input,
        ref_override,
        subpath_override: subpath,
        as_slug,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let dirs = { state.ctx.lock().unwrap().dirs.clone() };
        install::prefetch(&dirs, &request).map_err(err)?;
        let mut ctx = state.ctx.lock().unwrap();
        if dry_run {
            let preview = install::preview(&ctx, &request).map_err(err)?;
            return serde_json::to_value(preview)
                .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
        }
        let report = install::install(&mut ctx, &request).map_err(err)?;
        serde_json::to_value(report)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
pub(crate) fn conflict_payload_cmd(
    state: State<'_, AppState>,
    slug: String,
) -> Result<heron_service::diff::ConflictPayload, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::diff::conflict_payload(&ctx, &slug).map_err(err)
}

#[tauri::command]
pub(crate) fn diff_skill_cmd(
    state: State<'_, AppState>,
    slug: String,
    against: Option<String>,
) -> Result<heron_service::diff::DiffReport, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::diff::diff_skill(&ctx, &slug, against.as_deref()).map_err(err)
}

#[tauri::command]
pub(crate) fn resolve_conflict(
    state: State<'_, AppState>,
    slug: String,
    resolutions: Vec<(String, String)>,
) -> Result<upgrade::UpgradeOutcome, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::diff::resolve(&mut ctx, &slug, &resolutions).map_err(err)
}

#[tauri::command]
pub(crate) fn native_scan_status() -> Result<Vec<heron_service::native_scan::NativeScanStatus>, Msg>
{
    heron_service::native_scan::status().map_err(err)
}

#[tauri::command]
pub(crate) fn native_scan_plan(
    state: State<'_, AppState>,
    client: String,
) -> Result<heron_service::native_scan::InjectPlan, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::native_scan::plan(&ctx, &state.home, &client).map_err(err)
}

#[tauri::command]
pub(crate) fn lock_export(
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
pub(crate) fn lock_diff(
    state: State<'_, AppState>,
    file: String,
) -> Result<Vec<heron_service::lockfile::Drift>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    let lock = heron_service::lockfile::read(&PathBuf::from(file)).map_err(err)?;
    heron_service::lockfile::diff(&ctx, &lock).map_err(err)
}

#[tauri::command]
pub(crate) fn projects_discover(
    state: State<'_, AppState>,
    roots: Vec<String>,
    register: bool,
) -> Result<Vec<heron_service::projects::DiscoveredProject>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    let scan_roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
    heron_service::projects::discover_and_stamp(&mut ctx, &state.home, &scan_roots, register)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn projects_skills(
    state: State<'_, AppState>,
    root: String,
) -> Result<Vec<heron_service::projects::ProjectSkill>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    let specs = heron_service::clients::loaded_specs(&ctx.db).map_err(err)?;
    Ok(heron_service::projects::list_skills(
        &PathBuf::from(root),
        &specs,
    ))
}
