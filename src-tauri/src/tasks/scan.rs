//! Background machine scan task and `scan://*` event DTOs.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use heron_core::{HeronError, Msg};
use heron_scan::walker::{Hit, ScanMonitor, ScanStats};
use heron_service::scan::ScanRequest;
use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::state::{claim_task, err, request_cancel, AppState, RunningGuard, TaskFlags};
use crate::tasks::{TaskCancelDto, TaskStartDto, HIT_BATCH_INTERVAL};

/// `scan://progress` payload (camelCase contract with the Vue store).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanProgressDto {
    phase: &'static str,
    root_index: usize,
    root_total: usize,
    current_root: Option<String>,
    dirs_visited: usize,
    found: usize,
    percent: Option<f64>,
    truncated: bool,
}

/// Terminal `scan://progress` payload when the walk finishes with full summary counts.
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

/// One streamed discovery entry inside `scan://hit`, carrying real paths for card deduplication.
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
    found: usize,
}

fn root_percent(started: usize, total: usize) -> f64 {
    let done = started.saturating_sub(1) as f64 / total as f64;
    ((done * 100.0) * 10.0).round() / 10.0
}

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

/// Tracks which hits have already been pushed over `scan://hit` to batch emissions.
#[derive(Default)]
struct Forwarded {
    sent: usize,
    since_batch: Option<Instant>,
    root_index: usize,
}

impl Forwarded {
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

fn scan_task(app: tauri::AppHandle, extra_roots: Vec<String>, flags: TaskFlags) {
    let _guard = RunningGuard(flags.running);
    if let Err(e) = run_scan(&app, extra_roots, &flags.cancel) {
        let _ = app.emit("scan://error", err(e));
    }
}

/// Run presence detection (parallel, off-lock), build the scan plan, walk disk, and persist.
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

    // Presence detection runs in parallel outside `state.ctx.lock()` so CLI probes never block UI.
    let matrix = heron_service::clients::builtin()?;
    let detection = heron_service::detect::run_parallel(
        &matrix,
        &state.home,
        &heron_service::detect::RealMachine,
        &heron_service::detect::DetectOptions::default(),
    );
    let plan = {
        let mut ctx = state.ctx.lock().unwrap();
        heron_service::scan::prepare_with_detection(&mut ctx, &state.home, &request, &detection)?
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
    let result = {
        let mut hook = |stats: &ScanStats, hits: &[Hit]| forwarded.tick(app, stats, hits);
        let mut monitor = ScanMonitor {
            cancel: Some(cancel),
            on_progress: Some(&mut hook),
        };
        heron_service::scan::walk(&plan, &mut monitor)
    }?;

    let stats = &result.stats;
    let found = result.hits.len();
    let _ = app.emit("scan://progress", live_progress("persist", stats, found));
    {
        let mut ctx = state.ctx.lock().unwrap();
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

/// Start a background machine scan and return immediately; progress arrives over `scan://*` events.
#[tauri::command]
pub(crate) async fn scan(
    app: tauri::AppHandle,
    roots: Option<Vec<String>>,
) -> Result<TaskStartDto, Msg> {
    let flags = app.state::<AppState>().scan.share();
    if !claim_task(&flags) {
        return Ok(TaskStartDto {
            started: false,
            running: true,
        });
    }
    let extra_roots = roots.unwrap_or_default();
    let _task = tauri::async_runtime::spawn_blocking(move || scan_task(app, extra_roots, flags));
    Ok(TaskStartDto {
        started: true,
        running: true,
    })
}

/// Request cancellation of the active scan.
#[tauri::command]
pub(crate) fn scan_cancel(state: State<'_, AppState>) -> TaskCancelDto {
    TaskCancelDto {
        canceling: request_cancel(&state.scan),
    }
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
        assert_eq!(entry["clientId"], "claude");
        assert_eq!(entry["observedSlug"], "wechat-mp");
        assert_eq!(entry["path"], "/Users/me/.claude/skills/wechat-mp");
        assert_eq!(entry["frontmatterOk"], false);
        assert!(entry["projectPath"].is_null());
        assert!(entry["name"].is_null());
    }
}
