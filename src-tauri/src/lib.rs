//! Tauri desktop shell entry point.
//!
//! All domain logic lives in `heron-service` (ADR-0010); this crate only wires state, background
//! event streaming (`tasks`), system tray (`tray`), and thin IPC handlers (`commands`).

mod commands;
mod single_instance;
mod state;
mod tasks;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use heron_platform::dirs::Dirs;
use heron_service::Ctx;
use tauri::Manager;

use commands::*;
use state::{AppState, TaskFlags};
use tasks::checks::{doctor_cancel, doctor_start, verify_cancel, verify_start};
use tasks::scan::{scan, scan_cancel};
use tray::{show_main, tray_install};

/// Bootstrap storage/catalog state and start the Tauri application event loop.
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

    // Enforce a single desktop instance on macOS and Windows: if Heron is already running, wake
    // its existing window and exit before opening SQLite or creating a second webview.
    let mut instance_guard = match single_instance::acquire_or_notify(&dirs.data) {
        single_instance::AcquireResult::Primary(guard) => guard,
        single_instance::AcquireResult::NotifiedExisting { .. } => return,
    };

    let db = heron_store::db::Db::open(&dirs.db_path()).expect("index database");
    let mut ctx = Ctx::new(dirs, db, "gui");

    // Restore persisted network proxy and ensure built-in client/market catalogues exist on boot.
    let _ = heron_service::net::restore(&ctx);
    let _ = heron_service::clients::bootstrap(&mut ctx.db, &home);
    let _ = heron_service::clients::sync_catalog(&mut ctx.db, &home, false);
    heron_service::markets::bootstrap(&mut ctx);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            target_unproject_all,
            target_set_enabled,
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
            projects_skills,
            conflict_payload_cmd,
            diff_skill_cmd,
            resolve_conflict,
            native_scan_status,
            native_scan_plan,
            tray_install,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray when a tray icon is active; otherwise allow the window to close.
                if !window.state::<AppState>().tray_up.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running Heron");

    let handle = app.handle().clone();
    instance_guard.spawn_listener(move || show_main(&handle));

    app.run(move |_handle, _event| {
        // Keep the single-instance lock alive for the entire lifetime of the event loop.
        let _keep_lock = &instance_guard;
        // Clicking the Dock icon on macOS raises/restores the existing window.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main(_handle);
        }
    });
}
