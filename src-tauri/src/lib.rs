//! Tauri shell.
//!
//! This crate deliberately contains **no business logic**: every command validates nothing and
//! delegates to `heron-service`, which is also what the CLI uses. That keeps the GUI and the CLI
//! behaviourally identical (implementation doc §7.5).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use heron_core::{HeronError, Msg};
use heron_platform::dirs::Dirs;
use heron_scan::walker::{Hit, ScanMonitor, ScanStats};
use heron_service::install::InstallRequest;
use heron_service::project::ProjectRequest;
use heron_service::scan::ScanRequest;
use heron_service::{adopt, doctor, install, upgrade, verify, Ctx};
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};

/// A background check does not live in `AppState`'s mutex: the worker thread reads these atomics
/// while the IPC threads keep answering everything else, which is the whole point of backgrounding
/// it (implementation doc §7.5). One per long read-only task: scan, verify, doctor.
#[derive(Default)]
struct TaskFlags {
    /// A run owns the task. A second start is refused rather than queued behind it.
    running: Arc<AtomicBool>,
    /// Raised by the matching `*_cancel` command; the run checks it between units of work.
    cancel: Arc<AtomicBool>,
}

impl TaskFlags {
    /// The atomics a worker thread needs, so it never has to touch the `AppState` mutex again.
    fn share(&self) -> TaskFlags {
        TaskFlags {
            running: self.running.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

struct AppState {
    ctx: Mutex<Ctx>,
    home: PathBuf,
    scan: TaskFlags,
    verify: TaskFlags,
    doctor: TaskFlags,
    /// The status-item icon, once the UI has handed down its menu. Kept alive here: a tray that
    /// is dropped disappears from the menu bar.
    tray: Mutex<Option<TrayIcon>>,
    /// Whether a tray exists, read by the window's close handler. Deliberately an atomic rather
    /// than the mutex above: `on_window_event` runs on the main thread, while building a tray
    /// blocks that same thread waiting for a main-thread task — locking here could deadlock.
    tray_up: Arc<AtomicBool>,
}

fn err(e: HeronError) -> Msg {
    e.as_msg()
}

fn msg(e: Msg) -> Msg {
    e
}

/// Newly found skills go out in batches this wide, so a busy stretch of the walk is one event
/// rather than one per directory. A root boundary flushes whatever is waiting regardless.
const HIT_BATCH_INTERVAL: Duration = Duration::from_millis(300);

/// `scan://progress`. The field names are the contract with the UI: camelCase, verbatim.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanProgressDto {
    phase: &'static str,
    root_index: usize,
    root_total: usize,
    current_root: Option<String>,
    dirs_visited: usize,
    found: usize,
    /// `None` wherever there is no honest denominator: a single root (nobody knows how big it is),
    /// and every stage after the traversal. The UI runs its indeterminate animation instead.
    percent: Option<f64>,
    /// The directory cap was hit, so this result set is incomplete.
    truncated: bool,
}

/// The `done` event: the progress envelope plus the finished scan's numbers, so the UI has no
/// second call to make. `last_scan_at` is epoch milliseconds — when this scan completed.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanDoneDto {
    #[serde(flatten)]
    progress: ScanProgressDto,
    duration_ms: u64,
    last_scan_at: i64,
    skills: usize,
    legacy_commands: usize,
    nested_suspects: usize,
    needs_attention: usize,
    roots_missing: Vec<String>,
}

/// One entry of a `scan://hit` batch: what a card needs, in the same camelCase field names the UI
/// reads out of `detected_list` (`clientId`, `observedSlug`), so a row that streamed in and the row
/// the same scan later persists are one and the same row, not two cards.
///
/// `path` stays a real path, never the `~`-shortened display form: the UI keys on it, hands it back
/// to `detected_meta` and `adopt_paths`, and those open the file by that name.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanHitDto {
    path: String,
    real_path: String,
    kind: &'static str,
    client_id: Option<String>,
    scope: Option<&'static str>,
    project_path: Option<String>,
    observed_slug: Option<String>,
    name: Option<String>,
    description: Option<String>,
    frontmatter_ok: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanHitsDto {
    hits: Vec<ScanHitDto>,
    /// How many have been found in total, so a batch is never mistaken for the whole result.
    found: usize,
}

/// What `scan` answers straight away: the task is running, or one already was.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanStartDto {
    started: bool,
    running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanCancelDto {
    /// `false` when there was no scan in flight to stop.
    canceling: bool,
}

/// Share of the scan that is provably finished: whole roots only, one decimal.
fn root_percent(started: usize, total: usize) -> f64 {
    let done = started.saturating_sub(1) as f64 / total as f64;
    ((done * 100.0) * 10.0).round() / 10.0
}

/// Progress taken from the walker's live counters. Only the traversal has a denominator; the
/// stages after it report `percent: null` rather than an invented one.
fn live_progress(phase: &'static str, stats: &ScanStats, found: usize) -> ScanProgressDto {
    ScanProgressDto {
        phase,
        root_index: stats.root_index,
        root_total: stats.root_total,
        current_root: stats
            .current_root
            .as_deref()
            .map(heron_platform::paths::display),
        dirs_visited: stats.dirs_visited,
        found,
        percent: (stats.current_root.is_some() && stats.root_total > 1)
            .then(|| root_percent(stats.root_index, stats.root_total)),
        truncated: stats.truncated,
    }
}

/// The terminal `done` envelope. The walk's own counters still describe the last root, so the
/// numbers are re-anchored: every root is behind us now, hence 100.
fn done_progress(stats: &ScanStats, found: usize) -> ScanProgressDto {
    ScanProgressDto {
        phase: "done",
        root_index: stats.root_total,
        root_total: stats.root_total,
        current_root: None,
        dirs_visited: stats.dirs_visited,
        found,
        percent: Some(100.0),
        truncated: stats.truncated,
    }
}

fn hit_dto(hit: &Hit) -> ScanHitDto {
    let real = |p: &std::path::Path| p.to_string_lossy().to_string();
    ScanHitDto {
        path: real(&hit.path),
        real_path: real(&hit.real_path),
        kind: hit.kind.as_str(),
        client_id: hit.client.clone(),
        scope: hit.scope.map(|s| s.as_str()),
        project_path: hit.project.as_ref().map(|p| real(p)),
        observed_slug: hit.slug.clone(),
        name: hit
            .frontmatter
            .as_ref()
            .and_then(|f| f.name.clone())
            .filter(|s| !s.is_empty()),
        description: hit
            .frontmatter
            .as_ref()
            .and_then(|f| f.description.clone())
            .filter(|s| !s.is_empty()),
        frontmatter_ok: hit.frontmatter_ok(),
    }
}

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Which hits have already gone out over `scan://hit`, and when.
#[derive(Default)]
struct Forwarded {
    sent: usize,
    since_batch: Option<Instant>,
    root_index: usize,
}

impl Forwarded {
    /// One tick of the walker's throttle: always a progress event, a hit batch only when the
    /// batch window has passed or the walk moved on to another root.
    fn tick(&mut self, app: &tauri::AppHandle, stats: &ScanStats, hits: &[Hit]) {
        let _ = app.emit("scan://progress", live_progress("walk", stats, hits.len()));
        let root_boundary = stats.root_index != self.root_index;
        self.root_index = stats.root_index;
        if hits.len() <= self.sent {
            return;
        }
        let due = self
            .since_batch
            .map(|t| t.elapsed() >= HIT_BATCH_INTERVAL)
            .unwrap_or(true);
        if !(root_boundary || due) {
            return;
        }
        let batch = hits[self.sent..].iter().map(hit_dto).collect();
        let _ = app.emit(
            "scan://hit",
            ScanHitsDto {
                hits: batch,
                found: hits.len(),
            },
        );
        self.sent = hits.len();
        self.since_batch = Some(Instant::now());
    }
}

/// Clears the running flag whatever happens, so a panic on this thread cannot lock the UI out of
/// scanning for the rest of the session.
struct RunningGuard(Arc<AtomicBool>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn scan_task(app: tauri::AppHandle, extra_roots: Vec<String>, flags: TaskFlags) {
    let _guard = RunningGuard(flags.running);
    if let Err(e) = run_scan(&app, extra_roots, &flags.cancel) {
        // The invoke already answered, so the only way back to the UI is an event.
        let _ = app.emit("scan://error", err(e));
    }
}

/// The four stages of a scan, on a thread that is allowed to take its time. The database lock is
/// held inside `prepare` and `persist` only — never while the filesystem is being walked.
fn run_scan(
    app: &tauri::AppHandle,
    extra_roots: Vec<String>,
    cancel: &AtomicBool,
) -> Result<(), HeronError> {
    let state = app.state::<AppState>();
    let request = ScanRequest {
        extra_roots: extra_roots.into_iter().map(PathBuf::from).collect(),
        ..ScanRequest::default()
    };

    let plan = {
        let mut ctx = state.ctx.lock().unwrap();
        heron_service::scan::prepare(&mut ctx, &state.home, &request)?
    };
    let root_total = plan.roots().len();
    let _ = app.emit(
        "scan://progress",
        ScanProgressDto {
            phase: "prepare",
            root_index: 0,
            root_total,
            current_root: None,
            dirs_visited: 0,
            found: 0,
            percent: (root_total > 1).then_some(0.0),
            truncated: false,
        },
    );

    let mut forwarded = Forwarded::default();
    // The block is what ends the hook's borrow of `forwarded`; the walk itself needs both.
    let result = {
        let mut hook = |stats: &ScanStats, hits: &[Hit]| forwarded.tick(app, stats, hits);
        let mut monitor = ScanMonitor {
            cancel: Some(cancel),
            on_progress: Some(&mut hook),
        };
        heron_service::scan::walk(&plan, &mut monitor)
    };
    let result = result?;

    let stats = &result.stats;
    let found = result.hits.len();
    let _ = app.emit("scan://progress", live_progress("persist", stats, found));
    {
        let mut ctx = state.ctx.lock().unwrap();
        // A cancelled walk only saw part of the tree: pruning against that would delete the
        // detections it never reached, so it upserts and keeps everything else.
        heron_service::scan::persist(&mut ctx, &result, !stats.canceled)?;
    }

    if stats.canceled {
        let _ = app.emit("scan://progress", live_progress("canceled", stats, found));
        return Ok(());
    }

    let report = heron_service::scan::summarize(&result);
    let _ = app.emit("scan://progress", live_progress("summary", stats, found));
    let _ = app.emit(
        "scan://progress",
        ScanDoneDto {
            progress: done_progress(stats, found),
            duration_ms: stats.duration_ms,
            last_scan_at: epoch_ms(),
            skills: report.skills,
            legacy_commands: report.legacy_commands,
            nested_suspects: report.nested_suspects,
            needs_attention: report.needs_attention,
            roots_missing: report.roots_missing,
        },
    );
    Ok(())
}

/// Claim a task slot: `false` when one is already in flight — a second click is refused, it does
/// not queue behind the first. Whoever claims it re-arms the cancel flag, so a cancel meant for
/// the previous run cannot kill this one.
fn claim_task(flags: &TaskFlags) -> bool {
    if flags.running.swap(true, Ordering::SeqCst) {
        return false;
    }
    flags.cancel.store(false, Ordering::SeqCst);
    true
}

/// Scan the machine in the background and answer immediately. Progress, newly found skills and
/// the final counts all arrive as `scan://progress` / `scan://hit` events; the invoke never waits
/// for the walk, which is what used to freeze the window.
#[tauri::command]
async fn scan(app: tauri::AppHandle, roots: Option<Vec<String>>) -> Result<ScanStartDto, Msg> {
    let flags = app.state::<AppState>().scan.share();
    if !claim_task(&flags) {
        return Ok(ScanStartDto {
            started: false,
            running: true,
        });
    }
    let extra_roots = roots.unwrap_or_default();
    // The handle is deliberately not awaited: dropping it does not stop the task, it only stops
    // anyone from asking about it — the events are the answer.
    let _task = tauri::async_runtime::spawn_blocking(move || scan_task(app, extra_roots, flags));
    Ok(ScanStartDto {
        started: true,
        running: true,
    })
}

/// Ask the scan in flight to stop. What it already wrote stays; the task ends with a
/// `phase: "canceled"` progress event.
#[tauri::command]
fn scan_cancel(state: State<'_, AppState>) -> ScanCancelDto {
    ScanCancelDto {
        canceling: request_cancel(&state.scan),
    }
}

/// Raising cancel is all this does — the task decides when it is out, and says so itself. The
/// answer is whether there was anything running to stop.
fn request_cancel(flags: &TaskFlags) -> bool {
    let running = flags.running.load(Ordering::SeqCst);
    flags.cancel.store(true, Ordering::SeqCst);
    running
}

#[derive(Serialize)]
struct GitInfoDto {
    version: String,
}

#[tauri::command]
fn preflight(state: State<'_, AppState>) -> serde_json::Value {
    let pre = heron_platform::probe::preflight();
    serde_json::json!({
        "git": pre.git.map(|g| GitInfoDto { version: g.version }),
        "platform": heron_core::PlatformKind::current(),
        // The same home `bootstrap` rooted every target path at, so the UI can shorten that
        // prefix to `~` for display only (rule §4.2); real paths stay real.
        "home": state.home.to_string_lossy(),
    })
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
        // Skills the library projected into a client directory are already taken over: they stay
        // out of the discovery list even when an earlier scan indexed them.
        heron_store::repos::detected::list_by_state(ctx.db.conn(), &wanted)
            .and_then(|rows| heron_service::scan::without_projection_outputs(&ctx, rows))
    };
    rows.map_err(err)
}

#[tauri::command]
fn detected_meta(paths: Vec<String>) -> Vec<heron_service::scan::DetectedMeta> {
    heron_service::scan::describe(&paths)
}

#[tauri::command]
async fn adopt_paths(
    app: tauri::AppHandle,
    paths: Vec<String>,
    dry_run: bool,
    keep_original: bool,
    placement: Option<String>,
) -> Result<serde_json::Value, Msg> {
    // A non-`async` command runs on the thread that owns the window, so walking the store here
    // would freeze the UI for the duration of both clicks (preview and confirm). Do the disk work
    // on a blocking thread and hold `Ctx` only inside it — the same fix `scan` already uses.
    let work = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut ctx = state.ctx.lock().unwrap();
        adopt_paths_inner(
            &mut ctx,
            &paths,
            dry_run,
            keep_original,
            placement.as_deref(),
        )
    });
    work.await
        .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

fn adopt_paths_inner(
    ctx: &mut Ctx,
    paths: &[String],
    dry_run: bool,
    keep_original: bool,
    placement: Option<&str>,
) -> Result<serde_json::Value, Msg> {
    let rows =
        heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate").map_err(err)?;
    // Hash the store once for the whole batch, so dedup is a lookup per row instead of re-walking
    // every skill directory for every selection.
    let index = adopt::DedupIndex::build(ctx).map_err(err)?;
    let mut plans = Vec::new();
    for path in paths {
        if let Some(row) = rows.iter().find(|r| &r.path == path) {
            plans.push(adopt::plan_with_index(ctx, row, &index).map_err(err)?);
        }
    }
    if dry_run {
        return serde_json::to_value(&plans)
            .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()));
    }
    let placement = match placement {
        Some(value) => Some(adopt::AdoptPlacement::parse(value).ok_or_else(|| {
            Msg::new("E_INTERNAL").with("detail", format!("unknown placement {value}"))
        })?),
        None if keep_original => Some(adopt::AdoptPlacement::KeepOriginal),
        None => None,
    };
    let mut report = adopt::AdoptReport {
        adopted: vec![],
        skipped: vec![],
        errors: vec![],
    };
    for plan in &plans {
        // Without an explicit choice each entry goes the way its plan suggested: adopting a link the
        // user made elsewhere must not silently re-point it.
        let how = placement.unwrap_or(plan.suggested);
        // One failing path must not swallow the rest of the selection. Swallowing the error only
        // ever showed up as "nothing happened": an empty toast and an unchanged list.
        match adopt::execute(ctx, plan, how) {
            Ok(outcome) => {
                report.adopted.extend(outcome.adopted);
                report.skipped.extend(outcome.skipped);
                report.errors.extend(outcome.errors);
            }
            Err(e) => {
                let msg = err(e);
                report.errors.push(format!(
                    "{}: {} {}",
                    plan.detail.path,
                    msg.code,
                    msg.params
                        .get("detail")
                        .map(String::as_str)
                        .unwrap_or_default()
                ));
            }
        }
    }
    serde_json::to_value(&report).map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))
}

#[tauri::command]
fn skills_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::SkillRow>, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    // Rows adopted before adopt learned to read the file have an empty description. Filling them
    // from the content on disk is a no-op once the index is complete, so it can ride along here
    // instead of waiting for a repair the user has to think of.
    heron_service::adopt::repair_descriptions(&mut ctx).map_err(err)?;
    heron_store::repos::skill::list(ctx.db.conn(), false).map_err(err)
}

#[tauri::command]
fn targets_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::TargetRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_store::repos::target::list(ctx.db.conn()).map_err(err)
}

/// The client matrix with the presence verdicts of the last scan, so the overview can grey out a
/// landing point whose client is not on this machine without joining anything in the frontend.
#[tauri::command]
fn clients_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::ClientRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_store::repos::client::list(ctx.db.conn()).map_err(err)
}

#[tauri::command]
fn project_skill(
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
    // The target is mandatory: there is no default projection target, since the store is a source,
    // not a destination, and guessing a client directory would write where the user never pointed.
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
fn unproject_skill(state: State<'_, AppState>, slug: String, target_id: String) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::project::unproject(&mut ctx, &slug, &target_id).map_err(err)
}

/// Delete a skill from the library. `dry_run` answers with the plan (links + trash path) so the
/// dialog can tell the user exactly what will move before anything happens.
#[tauri::command]
fn skill_remove(
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

/// `verify://progress`. The field names are the contract with the UI: camelCase, verbatim.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VerifyProgressDto {
    phase: &'static str,
    done: usize,
    total: usize,
    /// `None` wherever there is no honest denominator: the `prepare` tick, whose row count is not
    /// known yet, and a store with no projections at all. The UI animates instead.
    percent: Option<f64>,
    /// Projections whose verdict is not `visible`, so far.
    findings: usize,
}

/// The terminal `verify://done` event: the progress envelope plus the reports, so the UI has no
/// second call to make. `rows` is exactly what `verify::verify` answers with, which keeps the
/// overview's `visibility` list working. A cancelled pass arrives too, with `phase: "canceled"`
/// and only the rows it managed to check.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VerifyDoneDto {
    #[serde(flatten)]
    progress: VerifyProgressDto,
    duration_ms: u64,
    rows: Vec<verify::VerifyReport>,
}

/// What `verify_start` answers straight away: the check is running, or one already was.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyStartDto {
    started: bool,
    running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyCancelDto {
    /// `false` when there was no check in flight to stop.
    canceling: bool,
}

/// Share of a check that is provably finished, one decimal — and only when a denominator exists to
/// divide by, so a bar is never shown for a number nobody actually knows.
fn tick_percent(done: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| ((done as f64 / total as f64 * 100.0) * 10.0).round() / 10.0)
}

/// Which phase has already gone out as a progress event, and when.
#[derive(Default)]
struct TickForward {
    phase: Option<&'static str>,
    since_emit: Option<Instant>,
}

impl TickForward {
    /// A tick gets its own event when the phase changes — which includes the terminal one, so even
    /// a sub-millisecond check leaves the UI with a start and an end — or once the batch window
    /// has passed. Anything closer is a bar that would not have moved anyway.
    fn due(&mut self, phase: &'static str) -> bool {
        let changed = self.phase != Some(phase);
        self.phase = Some(phase);
        let elapsed = self
            .since_emit
            .map(|t| t.elapsed() >= HIT_BATCH_INTERVAL)
            .unwrap_or(true);
        if changed || elapsed {
            self.since_emit = Some(Instant::now());
            return true;
        }
        false
    }
}

fn verify_progress(tick: verify::VerifyTick) -> VerifyProgressDto {
    VerifyProgressDto {
        phase: tick.phase,
        done: tick.done,
        total: tick.total,
        percent: tick_percent(tick.done, tick.total),
        findings: tick.findings,
    }
}

/// Verify visibility in the background and answer immediately. This used to be a synchronous
/// command, i.e. every projection path probed on the thread that owns the window.
#[tauri::command]
async fn verify_start(app: tauri::AppHandle) -> Result<VerifyStartDto, Msg> {
    let flags = app.state::<AppState>().verify.share();
    if !claim_task(&flags) {
        return Ok(VerifyStartDto {
            started: false,
            running: true,
        });
    }
    // Deliberately not awaited: the events are the answer.
    let _task = tauri::async_runtime::spawn_blocking(move || verify_task(app, flags));
    Ok(VerifyStartDto {
        started: true,
        running: true,
    })
}

fn verify_task(app: tauri::AppHandle, flags: TaskFlags) {
    let _guard = RunningGuard(flags.running);
    if let Err(e) = run_verify(&app, &flags.cancel) {
        // The invoke already answered, so the only way back to the UI is an event.
        let _ = app.emit("verify://error", err(e));
    }
}

fn run_verify(app: &tauri::AppHandle, cancel: &AtomicBool) -> Result<(), HeronError> {
    let state = app.state::<AppState>();
    let started = Instant::now();
    let _ = app.emit(
        "verify://progress",
        VerifyProgressDto {
            phase: "prepare",
            done: 0,
            total: 0,
            percent: None,
            findings: 0,
        },
    );

    let mut forwarded = TickForward::default();
    // The last tick the service handed over: once the call has returned it is the only place the
    // number of checked rows lives, which a stopped pass needs in order to say how far it got.
    let mut last = verify::VerifyTick {
        phase: "checking",
        done: 0,
        total: 0,
        findings: 0,
    };
    // The `Ctx` is locked on this thread rather than through `with_ctx`, which hands out a `&Ctx`
    // where refreshing the cache needs `&mut`.
    let outcome = {
        let mut ctx = state.ctx.lock().unwrap();
        verify::verify_with_hook(&mut ctx, None, &mut |tick| {
            last = tick;
            if forwarded.due(tick.phase) {
                let _ = app.emit("verify://progress", verify_progress(tick));
            }
            // Cancel is answered here as well as in the service loop, so a stretch of ticks that
            // were too close together to emit cannot swallow the user's click.
            !cancel.load(Ordering::SeqCst)
        })?
    };

    // A stopped pass still gets its `done` event, carrying what it checked: the UI's run has to
    // settle on something, and the rows it never looked at keep their cached verdict.
    let _ = app.emit(
        "verify://done",
        VerifyDoneDto {
            progress: VerifyProgressDto {
                phase: if outcome.canceled { "canceled" } else { "done" },
                ..verify_progress(last)
            },
            duration_ms: started.elapsed().as_millis() as u64,
            rows: outcome.rows,
        },
    );
    Ok(())
}

/// Ask the verify in flight to stop. What it already checked stays in the cache.
#[tauri::command]
fn verify_cancel(state: State<'_, AppState>) -> VerifyCancelDto {
    VerifyCancelDto {
        canceling: request_cancel(&state.verify),
    }
}

/// Answer on the main thread, do the work on a blocking one.
///
/// A non-`async` command runs inside the IPC handler, i.e. on the thread that owns the window, so
/// anything slow there freezes the UI — the same mistake `scan` already fixed by returning before
/// the walk. Checks that touch the filesystem or spawn git belong behind `spawn_blocking`; the
/// `AppHandle` is how the worker thread gets back to the shared `Ctx`.
async fn with_ctx<T, F>(app: tauri::AppHandle, f: F) -> Result<T, Msg>
where
    T: Send + 'static,
    F: FnOnce(&Ctx) -> Result<T, HeronError> + Send + 'static,
{
    let work = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ctx = state.ctx.lock().unwrap();
        f(&ctx)
    });
    let result = work
        .await
        .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?;
    result.map_err(err)
}

#[tauri::command]
async fn outdated_list(app: tauri::AppHandle) -> Result<Vec<upgrade::UpgradeCandidate>, Msg> {
    with_ctx(app, |ctx| {
        // Network and per-skill git: every enabled source is fetched and every managed skill
        // directory is opened and diffed against HEAD. Minutes of work for a large library.
        upgrade::refresh_sources(ctx)?;
        upgrade::outdated(ctx, heron_core::PinPolicy::Semver)
    })
    .await
}

#[tauri::command]
fn upgrade_skills(
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
        // `keep_local` carries 设置 → 通用 → 更新时保留本地改动: true = three-way merge, false =
        // hard overwrite that discards local edits.
        outcomes.push(upgrade::upgrade(&mut ctx, slug, keep_local).map_err(err)?);
    }
    Ok(outcomes)
}

#[tauri::command]
fn rollback_skill(state: State<'_, AppState>, slug: String, to: String) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    upgrade::rollback(&mut ctx, &slug, &to).map_err(err)?;
    Ok(())
}

/// `doctor://progress`, same envelope as `verify://progress` and the same camelCase contract.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoctorProgressDto {
    phase: &'static str,
    done: usize,
    total: usize,
    /// `None` wherever there is no honest denominator — an empty store has nothing to count down.
    percent: Option<f64>,
    /// Issues found so far.
    findings: usize,
}

/// The terminal `doctor://done` event: the progress envelope plus the report, so the UI has no
/// second call to make. A cancelled run arrives here too, with `phase: "canceled"` and whatever it
/// had found by then.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoctorDoneDto {
    #[serde(flatten)]
    progress: DoctorProgressDto,
    duration_ms: u64,
    report: doctor::DoctorReport,
}

/// What `doctor_start` answers straight away: the check is running, or one already was.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorStartDto {
    started: bool,
    running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorCancelDto {
    /// `false` when there was no check in flight to stop.
    canceling: bool,
}

fn doctor_progress(tick: doctor::DoctorTick) -> DoctorProgressDto {
    DoctorProgressDto {
        phase: tick.phase,
        done: tick.done,
        total: tick.total,
        percent: tick_percent(tick.done, tick.total),
        findings: tick.findings,
    }
}

/// Run the doctor in the background and answer immediately. It used to hash every managed skill
/// directory on the caller's thread, which is exactly the freeze `scan` already fixed.
#[tauri::command]
async fn doctor_start(app: tauri::AppHandle) -> Result<DoctorStartDto, Msg> {
    let flags = app.state::<AppState>().doctor.share();
    if !claim_task(&flags) {
        return Ok(DoctorStartDto {
            started: false,
            running: true,
        });
    }
    // Deliberately not awaited: the events are the answer.
    let _task = tauri::async_runtime::spawn_blocking(move || doctor_task(app, flags));
    Ok(DoctorStartDto {
        started: true,
        running: true,
    })
}

fn doctor_task(app: tauri::AppHandle, flags: TaskFlags) {
    let _guard = RunningGuard(flags.running);
    if let Err(e) = run_doctor(&app, &flags.cancel) {
        let _ = app.emit("doctor://error", err(e));
    }
}

fn run_doctor(app: &tauri::AppHandle, cancel: &AtomicBool) -> Result<(), HeronError> {
    let state = app.state::<AppState>();
    let started = Instant::now();
    let mut forwarded = TickForward::default();
    // The service reads all three lists before its first tick, so the denominator is real from the
    // first event; no `prepare` tick is invented for a number nobody has counted yet.
    let mut last = doctor::DoctorTick {
        phase: "detected",
        done: 0,
        total: 0,
        findings: 0,
    };
    let report = {
        let ctx = state.ctx.lock().unwrap();
        doctor::run_with_hook(&ctx, &mut |tick| {
            last = tick;
            if forwarded.due(tick.phase) {
                let _ = app.emit("doctor://progress", doctor_progress(tick));
            }
            !cancel.load(Ordering::SeqCst)
        })?
    };
    // Like verify, a stopped run still settles on a `done` event: the partial report is an answer,
    // and the `canceled` flag inside it is what tells the UI not to read it as a clean bill.
    let _ = app.emit(
        "doctor://done",
        DoctorDoneDto {
            // The terminal tick is the only place the counts live once the call has returned.
            progress: doctor_progress(last),
            duration_ms: started.elapsed().as_millis() as u64,
            report,
        },
    );
    Ok(())
}

/// Ask the doctor in flight to stop. It keeps the issues it had already found.
#[tauri::command]
fn doctor_cancel(state: State<'_, AppState>) -> DoctorCancelDto {
    DoctorCancelDto {
        canceling: request_cancel(&state.doctor),
    }
}

/// A `source` row for a built-in market, used both to seed the table and to persist a toggle.
/// The market catalogue in `heron-source` stays the source of truth; the row only carries the
/// user's on/off choice (and the capability for display parity).
fn market_row(id: &str, enabled: bool) -> Option<heron_store::models::SourceRow> {
    let market = heron_source::MARKETS.iter().find(|m| m.id == id)?;
    Some(heron_store::models::SourceRow {
        id: market.id.to_owned(),
        name: market.name.to_owned(),
        kind: "market".to_owned(),
        adapter: Some(market.id.to_owned()),
        url: Some(market.base_url.to_owned()),
        ref_name: None,
        subpath: None,
        path: None,
        priority: 0,
        trusted: true,
        enabled,
        capability: Some(
            match market.capability {
                heron_source::Capability::Indexed => "indexed",
                heron_source::Capability::Guided => "guided",
            }
            .to_owned(),
        ),
        last_synced_at: None,
        last_commit: None,
        last_probe_ok: None,
    })
}

/// Seed the `source` table from the built-in markets on first run, so a toggle has a row to update.
/// Existing rows keep the user's persisted choice (upsert runs only when the id is absent).
fn bootstrap_markets(ctx: &mut heron_service::Ctx) {
    for market in heron_source::MARKETS {
        let exists = heron_store::repos::source::get(ctx.db.conn(), market.id)
            .ok()
            .flatten()
            .is_some();
        if exists {
            continue;
        }
        if let Some(row) = market_row(market.id, market.enabled_by_default) {
            let _ = ctx
                .db
                .write(|tx| heron_store::repos::source::upsert(tx, &row));
        }
    }
}

#[tauri::command]
fn markets_list(state: State<'_, AppState>) -> Vec<serde_json::Value> {
    let ctx = state.ctx.lock().unwrap();
    heron_source::MARKETS
        .iter()
        .map(|m| {
            // The persisted toggle wins; fall back to the built-in default before first seed.
            let enabled = heron_store::repos::source::get(ctx.db.conn(), m.id)
                .ok()
                .flatten()
                .map(|row| row.enabled)
                .unwrap_or(m.enabled_by_default);
            serde_json::json!({
                "id": m.id,
                "name": m.name,
                "capability": m.capability,
                "enabled_by_default": m.enabled_by_default,
                "enabled": enabled,
                "base_url": m.base_url,
            })
        })
        .collect()
}

#[tauri::command]
fn market_set_enabled(
    state: State<'_, AppState>,
    market: String,
    enabled: bool,
) -> Result<(), Msg> {
    if heron_source::MARKETS.iter().all(|m| m.id != market) {
        return Err(Msg::new("E_MARKET_UNAVAILABLE").with("market", market));
    }
    let row = market_row(&market, enabled).expect("market checked above");
    let mut ctx = state.ctx.lock().unwrap();
    ctx.db
        .write(|tx| heron_store::repos::source::upsert(tx, &row))
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
fn favorite_list(state: State<'_, AppState>) -> Result<Vec<heron_store::models::FavoriteRow>, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_store::repos::favorite::list(ctx.db.conn()).map_err(err)
}

#[tauri::command]
fn favorite_add(
    state: State<'_, AppState>,
    name: String,
    url: String,
    description: String,
    market: Option<String>,
) -> Result<heron_store::models::FavoriteRow, Msg> {
    let url = url.trim().to_owned();
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(Msg::new("E_INTERNAL").with("detail", "favorite_name_required"));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Msg::new("E_INTERNAL").with("detail", "favorite_url_required"));
    }
    let row = heron_store::models::FavoriteRow {
        id: 0,
        name,
        url,
        description: description.trim().to_owned(),
        market: market
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty()),
        created_at: String::new(),
    };
    let mut ctx = state.ctx.lock().unwrap();
    ctx.db
        .write(|tx| heron_store::repos::favorite::save(tx, &row))
        .map_err(err)
}

#[tauri::command]
fn favorite_remove(state: State<'_, AppState>, id: i64) -> Result<(), Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    ctx.db
        .write(|tx| heron_store::repos::favorite::remove(tx, id))
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn market_search(
    market: String,
    query: String,
) -> Result<Vec<heron_source::market::MarketItem>, Msg> {
    // ureq 是同步阻塞的 HTTP 客户端，一次搜索可能跑满超时（含重试）。放在 blocking 线程池里执行，
    // 否则命令会在主线程上阻塞，点击搜索时整个窗口都会卡住；同时各来源才能真正并发。
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = heron_source::adapter(&market).map_err(err)?;
        adapter.search(&query, 20).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

#[tauri::command]
async fn market_plan(
    market: String,
    key: String,
) -> Result<heron_source::market::InstallPlan, Msg> {
    // 同上：解析安装计划也可能发网络请求，移到 blocking 线程，避免卡住主线程。
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = heron_source::adapter(&market).map_err(err)?;
        adapter.install_plan(&key).map_err(err)
    })
    .await
    .map_err(|e| Msg::new("E_INTERNAL").with("detail", e.to_string()))?
}

/// 设置页的代理配置：读取保存的代理地址、启用状态与实际生效地址。
#[tauri::command]
fn net_get_proxy(state: State<'_, AppState>) -> Result<heron_service::net::ProxyConfig, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::net::get(&ctx).map_err(err)
}

/// 写入并立即生效：存进数据库的同时更新进程级代理，之后的 git 拉取与市场搜索都会用它。
#[tauri::command]
fn net_set_proxy(
    state: State<'_, AppState>,
    url: Option<String>,
    enabled: bool,
) -> Result<heron_service::net::ProxyConfig, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::net::set(&ctx, url.as_deref(), enabled).map_err(err)
}

#[tauri::command]
async fn install_skill(
    app: tauri::AppHandle,
    input: String,
    ref_override: Option<String>,
    subpath: Option<String>,
    as_slug: Option<String>,
    dry_run: bool,
) -> Result<serde_json::Value, Msg> {
    // git clone 是阻塞操作：与市场搜索同理移到 blocking 线程池，否则安装 / 预览时整个窗口会卡住。
    // State<'_, _> 不能跨 await，故改传 'static + Send 的 AppHandle，在闭包内重新取回受管理的 AppState。
    let request = InstallRequest {
        input,
        ref_override,
        subpath_override: subpath,
        as_slug,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        // fetch 阶段完全不碰状态锁：Dirs 是 Clone 的，先短暂取锁克隆出来就放开，
        // 慢网络只拖住这一次安装，其它命令（列表、扫描、设置）照常响应。
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
fn conflict_payload_cmd(
    state: State<'_, AppState>,
    slug: String,
) -> Result<heron_service::diff::ConflictPayload, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::diff::conflict_payload(&ctx, &slug).map_err(err)
}

#[tauri::command]
fn diff_skill_cmd(
    state: State<'_, AppState>,
    slug: String,
    against: Option<String>,
) -> Result<heron_service::diff::DiffReport, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::diff::diff_skill(&ctx, &slug, against.as_deref()).map_err(err)
}

#[tauri::command]
fn resolve_conflict(
    state: State<'_, AppState>,
    slug: String,
    resolutions: Vec<(String, String)>,
) -> Result<upgrade::UpgradeOutcome, Msg> {
    let mut ctx = state.ctx.lock().unwrap();
    heron_service::diff::resolve(&mut ctx, &slug, &resolutions).map_err(err)
}

#[tauri::command]
fn native_scan_status() -> Result<Vec<heron_service::native_scan::NativeScanStatus>, Msg> {
    heron_service::native_scan::status().map_err(err)
}

#[tauri::command]
fn native_scan_plan(
    state: State<'_, AppState>,
    client: String,
) -> Result<heron_service::native_scan::InjectPlan, Msg> {
    let ctx = state.ctx.lock().unwrap();
    heron_service::native_scan::plan(&ctx, &state.home, &client).map_err(err)
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

/// One row of the tray menu. The UI sends the whole list because the labels are UI copy: only the
/// webview knows which language is active, which is the same reason the menu bar is built there.
/// The ids, though, belong to this file — `show` and `quit` are answered by the shell, everything
/// else goes back to the UI as a `tray://action` event and runs the action the menu bar runs.
#[derive(serde::Deserialize)]
struct TrayItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    separator: bool,
}

/// Bring the window back from behind the tray.
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn tray_menu(app: &tauri::AppHandle, items: &[TrayItem]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for item in items {
        if item.separator {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        } else {
            menu.append(
                &MenuItemBuilder::with_id(item.id.as_str(), item.text.as_str()).build(app)?,
            )?;
        }
    }
    Ok(menu)
}

/// Install — or, on a language change, rebuild — the status-item icon. Called by the UI once the
/// locale is known, so the first paint does not have a half-English tray menu.
#[tauri::command]
fn tray_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    items: Vec<TrayItem>,
) -> Result<(), Msg> {
    let tray_error = |e: tauri::Error| {
        eprintln!("heron: tray: {e}");
        Msg::new("E_INTERNAL")
    };
    let menu = tray_menu(&app, &items).map_err(tray_error)?;
    // The mutex stays held across the calls below: they round-trip to the main thread, which never
    // takes this lock, so they cannot deadlock — and holding it is what keeps two concurrent
    // installs (a locale switch landing mid-install) from producing two icons in the menu bar.
    let mut guard = state.tray.lock().unwrap();
    if let Some(tray) = guard.as_ref() {
        return tray.set_menu(Some(menu)).map_err(tray_error);
    }
    // macOS tints a template image from its alpha channel and drops the colours, so it gets the
    // bare silhouette; everywhere else the tray keeps the colour app icon.
    #[cfg(target_os = "macos")]
    let icon = include_bytes!("../icons/tray-template.png");
    #[cfg(not(target_os = "macos"))]
    let icon = include_bytes!("../icons/32x32.png");
    let icon = Image::from_bytes(icon).map_err(tray_error)?;

    let builder = TrayIconBuilder::new()
        .menu(&menu)
        // Left click raises the window, right click opens the menu.
        .show_menu_on_left_click(false)
        .tooltip("Heron")
        .icon(icon)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            id => {
                let _ = app.emit("tray://action", id.to_string());
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);

    let tray = builder.build(&app).map_err(tray_error)?;
    *guard = Some(tray);
    state.tray_up.store(true, Ordering::SeqCst);
    Ok(())
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
    // 把设置页里存过的代理值恢复到进程，否则第一次安装会用不上用户明明填好的代理。
    let _ = heron_service::net::restore(&ctx);
    let _ = heron_service::clients::bootstrap(&mut ctx.db, &home);
    // A release that added clients must show them before the first scan, otherwise the new rows
    // only appear after the user happens to press 扫描 — and the grid silently keeps the old shape.
    let _ = heron_service::clients::sync_catalog(&mut ctx.db, &home, false);
    bootstrap_markets(&mut ctx);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // 开机启动（设置页的开关）：macOS 用 LaunchAgent 注册，比 AppleScript 的登录项更稳，
        // 不需要「自动化」授权弹窗；Windows 写注册表、Linux 写 autostart desktop 文件。
        // 不需要额外启动参数，故 args 传 None。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            ctx: Mutex::new(ctx),
            home,
            scan: TaskFlags::default(),
            verify: TaskFlags::default(),
            doctor: TaskFlags::default(),
            tray: Mutex::new(None),
            tray_up: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            preflight,
            scan,
            scan_cancel,
            detected_list,
            detected_meta,
            adopt_paths,
            skills_list,
            targets_list,
            clients_list,
            project_skill,
            unproject_skill,
            skill_remove,
            verify_start,
            verify_cancel,
            outdated_list,
            upgrade_skills,
            rollback_skill,
            doctor_start,
            doctor_cancel,
            markets_list,
            market_set_enabled,
            market_search,
            market_plan,
            net_get_proxy,
            net_set_proxy,
            favorite_list,
            favorite_add,
            favorite_remove,
            install_skill,
            lock_export,
            lock_diff,
            projects_discover,
            conflict_payload_cmd,
            diff_skill_cmd,
            resolve_conflict,
            native_scan_status,
            native_scan_plan,
            tray_install,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Closing the window hides it: the app keeps living in the tray, and quitting is
                // only ever the tray menu's job. Without a tray there would be nothing to come
                // back from, so then the red button has to do what it always did.
                if !window.state::<AppState>().tray_up.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running Heron");

    app.run(|_handle, _event| {
        // A hidden-to-tray app has no window left in the Dock; macOS still sends Reopen when the
        // icon is clicked, which is the one way back that does not go through the tray menu.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = _event
        {
            show_main(_handle);
        }
    });
}

#[allow(dead_code)]
fn _msg(m: Msg) -> Msg {
    msg(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron_scan::classify::{HitKind, Scope};

    fn stats(root_index: usize, root_total: usize, walking: bool) -> ScanStats {
        ScanStats {
            dirs_visited: 4_321,
            root_index,
            root_total,
            current_root: walking.then(|| PathBuf::from("/Users/me/.claude/skills")),
            ..ScanStats::default()
        }
    }

    /// The field names are the UI's contract; renaming one here is a breaking change.
    #[test]
    fn progress_event_has_exactly_the_agreed_fields() {
        let value = serde_json::to_value(live_progress("walk", &stats(2, 3, true), 17)).unwrap();
        let mut keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "currentRoot",
                "dirsVisited",
                "found",
                "percent",
                "phase",
                "rootIndex",
                "rootTotal",
                "truncated"
            ]
        );
        assert_eq!(value["phase"], "walk");
        assert_eq!(value["rootIndex"], 2);
        assert_eq!(value["rootTotal"], 3);
        assert_eq!(value["dirsVisited"], 4321);
        assert_eq!(value["found"], 17);
        assert_eq!(value["truncated"], false);
    }

    /// One root has no known size, and neither has any stage after the traversal. A percentage
    /// there would be a lie, so it is null and the UI moves without a number.
    #[test]
    fn percent_is_reported_only_where_it_is_real() {
        let percent = |phase: &'static str, stats: ScanStats, found: usize| {
            serde_json::to_value(live_progress(phase, &stats, found)).unwrap()["percent"].clone()
        };
        assert_eq!(
            percent("walk", stats(1, 1, true), 3),
            serde_json::Value::Null
        );
        assert_eq!(
            percent("persist", stats(3, 3, false), 3),
            serde_json::Value::Null
        );
        // Multiple roots: whole roots only, and never ahead of the root being walked.
        assert_eq!(percent("walk", stats(1, 4, true), 0), 0.0);
        assert_eq!(percent("walk", stats(3, 4, true), 9), 50.0);
        assert_eq!(
            percent("canceled", stats(2, 4, false), 9),
            serde_json::Value::Null
        );
    }

    #[test]
    fn done_event_carries_the_final_numbers_and_the_cap_flag() {
        let mut walked = stats(3, 3, false);
        walked.truncated = true;
        walked.duration_ms = 8_150;
        walked.roots_missing.push(PathBuf::from("/gone"));
        let report = heron_service::scan::ScanReport {
            skills: 42,
            legacy_commands: 3,
            nested_suspects: 1,
            needs_attention: 2,
            dirs_visited: walked.dirs_visited,
            duration_ms: walked.duration_ms,
            truncated: walked.truncated,
            roots_missing: vec!["~/gone".to_string()],
        };
        let value = serde_json::to_value(ScanDoneDto {
            progress: done_progress(&walked, 46),
            duration_ms: report.duration_ms,
            last_scan_at: 1_763_000_000_000,
            skills: report.skills,
            legacy_commands: report.legacy_commands,
            nested_suspects: report.nested_suspects,
            needs_attention: report.needs_attention,
            roots_missing: report.roots_missing,
        })
        .unwrap();
        assert_eq!(value["phase"], "done");
        assert_eq!(value["percent"], 100.0);
        assert_eq!(value["found"], 46);
        // The 50k directory cap has to reach the UI: an incomplete result cannot look finished.
        assert_eq!(value["truncated"], true);
        assert_eq!(value["lastScanAt"], 1_763_000_000_000_i64);
        assert_eq!(value["durationMs"], 8_150);
        assert_eq!(value["legacyCommands"], 3);
        assert_eq!(value["nestedSuspects"], 1);
        assert_eq!(value["needsAttention"], 2);
        assert_eq!(value["rootsMissing"][0], "~/gone");
    }

    #[test]
    fn hit_batch_uses_the_camel_case_entry_shape() {
        let path = PathBuf::from("/Users/me/.claude/skills/wechat-mp");
        let hit = Hit {
            path: path.clone(),
            real_path: path,
            kind: HitKind::Skill,
            client: Some("claude".to_string()),
            scope: Some(Scope::Global),
            project: None,
            slug: Some("wechat-mp".to_string()),
            frontmatter: None,
            issues: vec![],
        };
        let value = serde_json::to_value(ScanHitsDto {
            hits: vec![hit_dto(&hit)],
            found: 1,
        })
        .unwrap();
        assert_eq!(value["found"], 1);
        let entry = &value["hits"][0];
        assert_eq!(entry["kind"], "skill");
        assert_eq!(entry["scope"], "global");
        // The names the UI reads, and the real path it keys on: a streamed row has to be the same
        // row as the one `detected_list` returns for the same file.
        assert_eq!(entry["clientId"], "claude");
        assert_eq!(entry["observedSlug"], "wechat-mp");
        assert_eq!(entry["path"], "/Users/me/.claude/skills/wechat-mp");
        assert_eq!(entry["frontmatterOk"], false);
        assert!(entry["projectPath"].is_null());
        assert!(entry["name"].is_null());
    }

    #[test]
    fn a_second_task_is_refused_and_the_next_one_re_arms_cancel() {
        let flags = TaskFlags::default();
        assert!(claim_task(&flags), "the first click owns the task slot");
        assert!(
            !claim_task(&flags),
            "the second is refused instead of queued behind it"
        );

        // The `*_cancel` commands only raise the flag; the task clears running on its way out.
        flags.cancel.store(true, Ordering::SeqCst);
        assert!(
            request_cancel(&flags),
            "there is a task in flight to cancel"
        );
        {
            let _guard = RunningGuard(flags.running.clone());
        }
        assert!(!flags.running.load(Ordering::SeqCst));
        assert!(claim_task(&flags), "the next click is accepted");
        assert!(
            !flags.cancel.load(Ordering::SeqCst),
            "claiming a task re-arms cancel, so the old request cannot kill the new one"
        );

        let idle = TaskFlags::default();
        assert!(
            !request_cancel(&idle),
            "cancelling nothing says so, it is not an error"
        );
    }

    /// The two checks are a contract with the UI exactly as `scan://*` is: one wrong field name and
    /// a progress bar silently never moves.
    #[test]
    fn the_verify_and_doctor_events_have_exactly_the_agreed_fields() {
        fn keys(value: &serde_json::Value) -> Vec<&str> {
            let mut names: Vec<&str> = value
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            names.sort_unstable();
            names
        }
        let progress = ["done", "findings", "percent", "phase", "total"];

        let tick = verify::VerifyTick {
            phase: "checking",
            done: 3,
            total: 9,
            findings: 1,
        };
        let value = serde_json::to_value(verify_progress(tick)).unwrap();
        assert_eq!(keys(&value), progress);
        assert_eq!(value["percent"], 33.3, "one decimal, and real");
        assert_eq!(
            keys(
                &serde_json::to_value(VerifyDoneDto {
                    progress: VerifyProgressDto {
                        phase: "canceled",
                        ..verify_progress(tick)
                    },
                    duration_ms: 1234,
                    rows: vec![],
                })
                .unwrap()
            ),
            [
                "done",
                "durationMs",
                "findings",
                "percent",
                "phase",
                "rows",
                "total"
            ]
        );

        let dtick = doctor::DoctorTick {
            phase: "content",
            done: 4,
            total: 7,
            findings: 2,
        };
        assert_eq!(
            keys(&serde_json::to_value(doctor_progress(dtick)).unwrap()),
            progress
        );
        assert_eq!(
            keys(
                &serde_json::to_value(DoctorDoneDto {
                    progress: doctor_progress(dtick),
                    duration_ms: 42,
                    report: doctor::DoctorReport::default(),
                })
                .unwrap()
            ),
            [
                "done",
                "durationMs",
                "findings",
                "percent",
                "phase",
                "report",
                "total"
            ]
        );
        // The report itself carries the stop flag, so a UI that only reads `report` and never
        // looks at the envelope's phase still knows the answer is partial.
        let report = serde_json::to_value(doctor::DoctorReport {
            issues: vec![],
            canceled: true,
        })
        .unwrap();
        assert_eq!(keys(&report), ["canceled", "issues"]);
        assert_eq!(report["canceled"], true);

        assert_eq!(
            keys(
                &serde_json::to_value(VerifyStartDto {
                    started: false,
                    running: true
                })
                .unwrap()
            ),
            ["running", "started"]
        );
        assert_eq!(
            keys(&serde_json::to_value(DoctorCancelDto { canceling: true }).unwrap()),
            ["canceling"]
        );

        // No denominator, no percentage: the field is there and null, so the UI animates.
        let prepare = serde_json::to_value(verify_progress(verify::VerifyTick {
            phase: "prepare",
            done: 0,
            total: 0,
            findings: 0,
        }))
        .unwrap();
        assert!(prepare["percent"].is_null());
    }
}
