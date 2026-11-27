//! Shared application state and background-task coordination primitives for the Tauri shell.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use heron_core::{HeronError, Msg};
use heron_service::Ctx;
use tauri::tray::TrayIcon;
use tauri::Manager;

/// Lock-free coordination flags for one background job (`scan`, `verify`, or `doctor`).
///
/// Kept outside `AppState.ctx`'s mutex so worker threads can poll cancellation and IPC handlers
/// can refuse duplicate starts without waiting on database I/O.
#[derive(Default)]
pub(crate) struct TaskFlags {
    /// `true` while a worker owns the task slot; a second start is refused rather than queued.
    pub(crate) running: Arc<AtomicBool>,
    /// Raised by the matching `*_cancel` command; the worker checks it between units of work.
    pub(crate) cancel: Arc<AtomicBool>,
}

impl TaskFlags {
    /// Clone the underlying atomics so a spawned worker never needs to borrow `AppState`.
    pub(crate) fn share(&self) -> TaskFlags {
        TaskFlags {
            running: self.running.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

/// Global Tauri-managed state shared across all IPC commands and window/tray handlers.
pub(crate) struct AppState {
    pub(crate) ctx: Mutex<Ctx>,
    pub(crate) home: PathBuf,
    pub(crate) scan: TaskFlags,
    pub(crate) verify: TaskFlags,
    pub(crate) doctor: TaskFlags,
    /// Status-item icon kept alive for the lifetime of the app; dropping it removes the tray icon.
    pub(crate) tray: Mutex<Option<TrayIcon>>,
    /// Atomic mirror of whether a tray icon exists, read by `on_window_event` on the main thread
    /// without touching `tray`'s mutex (preventing main-thread deadlock).
    pub(crate) tray_up: Arc<AtomicBool>,
}

/// RAII guard that clears a task's `running` flag on normal exit or panic.
pub(crate) struct RunningGuard(pub(crate) Arc<AtomicBool>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Atomically claim a task slot and re-arm its cancel flag; returns `false` if already running.
pub(crate) fn claim_task(flags: &TaskFlags) -> bool {
    if flags.running.swap(true, Ordering::SeqCst) {
        return false;
    }
    flags.cancel.store(false, Ordering::SeqCst);
    true
}

/// Signal cancellation to a running task; returns whether a task was actually running.
pub(crate) fn request_cancel(flags: &TaskFlags) -> bool {
    let running = flags.running.load(Ordering::SeqCst);
    flags.cancel.store(true, Ordering::SeqCst);
    running
}

/// Convert a service-layer [`HeronError`] into the wire [`Msg`] expected by the UI.
pub(crate) fn err(e: HeronError) -> Msg {
    e.as_msg()
}

/// Execute a blocking read-only `Ctx` closure on Tauri's blocking pool so the UI thread never stalls.
pub(crate) async fn with_ctx<T, F>(app: tauri::AppHandle, f: F) -> Result<T, Msg>
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_task_is_refused_and_the_next_one_re_arms_cancel() {
        let flags = TaskFlags::default();
        assert!(claim_task(&flags), "the first click owns the task slot");
        assert!(
            !claim_task(&flags),
            "the second is refused instead of queued behind it"
        );

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
}
