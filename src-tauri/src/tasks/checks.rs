//! Background visibility verification (`verify://*`) and health check (`doctor://*`) tasks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use heron_core::{HeronError, Msg};
use heron_service::{doctor, verify};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::state::{claim_task, err, request_cancel, AppState, RunningGuard, TaskFlags};
use crate::tasks::{tick_percent, TaskCancelDto, TaskProgressDto, TaskStartDto, TickForward};

/// Terminal `verify://done` event combining the progress envelope and verified rows.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VerifyDoneDto {
    #[serde(flatten)]
    progress: TaskProgressDto,
    duration_ms: u64,
    rows: Vec<verify::VerifyReport>,
}

/// Terminal `doctor://done` event combining the progress envelope and diagnostic report.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoctorDoneDto {
    #[serde(flatten)]
    progress: TaskProgressDto,
    duration_ms: u64,
    report: doctor::DoctorReport,
}

fn verify_progress(tick: verify::VerifyTick) -> TaskProgressDto {
    TaskProgressDto {
        phase: tick.phase,
        done: tick.done,
        total: tick.total,
        percent: tick_percent(tick.done, tick.total),
        findings: tick.findings,
    }
}

fn doctor_progress(tick: doctor::DoctorTick) -> TaskProgressDto {
    TaskProgressDto {
        phase: tick.phase,
        done: tick.done,
        total: tick.total,
        percent: tick_percent(tick.done, tick.total),
        findings: tick.findings,
    }
}

/// Start a background visibility verification pass; results stream over `verify://*` events.
#[tauri::command]
pub(crate) async fn verify_start(app: tauri::AppHandle) -> Result<TaskStartDto, Msg> {
    let flags = app.state::<AppState>().verify.share();
    if !claim_task(&flags) {
        return Ok(TaskStartDto {
            started: false,
            running: true,
        });
    }
    let _task = tauri::async_runtime::spawn_blocking(move || verify_task(app, flags));
    Ok(TaskStartDto {
        started: true,
        running: true,
    })
}

/// Request cancellation of the active verify pass.
#[tauri::command]
pub(crate) fn verify_cancel(state: State<'_, AppState>) -> TaskCancelDto {
    TaskCancelDto {
        canceling: request_cancel(&state.verify),
    }
}

fn verify_task(app: tauri::AppHandle, flags: TaskFlags) {
    let _guard = RunningGuard(flags.running);
    if let Err(e) = run_verify(&app, &flags.cancel) {
        let _ = app.emit("verify://error", err(e));
    }
}

fn run_verify(app: &tauri::AppHandle, cancel: &AtomicBool) -> Result<(), HeronError> {
    let state = app.state::<AppState>();
    let started = Instant::now();
    let _ = app.emit(
        "verify://progress",
        TaskProgressDto {
            phase: "prepare",
            done: 0,
            total: 0,
            percent: None,
            findings: 0,
        },
    );

    let mut forwarded = TickForward::default();
    let mut last = verify::VerifyTick {
        phase: "checking",
        done: 0,
        total: 0,
        findings: 0,
    };

    // Snapshot under a short lock, probe the filesystem lock-free, then commit cache under a short lock.
    let plan = {
        let ctx = state.ctx.lock().unwrap();
        verify::prepare(&ctx, None)?
    };
    let (outcome, cache) = verify::execute_plan(plan, &mut |tick| {
        last = tick;
        if forwarded.due(tick.phase) {
            let _ = app.emit("verify://progress", verify_progress(tick));
        }
        !cancel.load(Ordering::SeqCst)
    })?;
    {
        let mut ctx = state.ctx.lock().unwrap();
        verify::commit(&mut ctx, &cache)?;
    }

    let _ = app.emit(
        "verify://done",
        VerifyDoneDto {
            progress: TaskProgressDto {
                phase: if outcome.canceled { "canceled" } else { "done" },
                ..verify_progress(last)
            },
            duration_ms: started.elapsed().as_millis() as u64,
            rows: outcome.rows,
        },
    );
    Ok(())
}

/// Start a background doctor check-up; results stream over `doctor://*` events.
#[tauri::command]
pub(crate) async fn doctor_start(app: tauri::AppHandle) -> Result<TaskStartDto, Msg> {
    let flags = app.state::<AppState>().doctor.share();
    if !claim_task(&flags) {
        return Ok(TaskStartDto {
            started: false,
            running: true,
        });
    }
    let _task = tauri::async_runtime::spawn_blocking(move || doctor_task(app, flags));
    Ok(TaskStartDto {
        started: true,
        running: true,
    })
}

/// Request cancellation of the active doctor run.
#[tauri::command]
pub(crate) fn doctor_cancel(state: State<'_, AppState>) -> TaskCancelDto {
    TaskCancelDto {
        canceling: request_cancel(&state.doctor),
    }
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
    let mut last = doctor::DoctorTick {
        phase: "detected",
        done: 0,
        total: 0,
        findings: 0,
    };

    let plan = {
        let ctx = state.ctx.lock().unwrap();
        doctor::prepare(&ctx)?
    };
    let report = doctor::execute_plan(plan, &mut |tick| {
        last = tick;
        if forwarded.due(tick.phase) {
            let _ = app.emit("doctor://progress", doctor_progress(tick));
        }
        !cancel.load(Ordering::SeqCst)
    })?;

    let _ = app.emit(
        "doctor://done",
        DoctorDoneDto {
            progress: doctor_progress(last),
            duration_ms: started.elapsed().as_millis() as u64,
            report,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    progress: TaskProgressDto {
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

        let report = serde_json::to_value(doctor::DoctorReport {
            issues: vec![],
            canceled: true,
        })
        .unwrap();
        assert_eq!(keys(&report), ["canceled", "issues"]);
        assert_eq!(report["canceled"], true);

        assert_eq!(
            keys(
                &serde_json::to_value(TaskStartDto {
                    started: false,
                    running: true
                })
                .unwrap()
            ),
            ["running", "started"]
        );
        assert_eq!(
            keys(&serde_json::to_value(TaskCancelDto { canceling: true }).unwrap()),
            ["canceling"]
        );

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
