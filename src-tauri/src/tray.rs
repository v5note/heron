//! System tray icon lifecycle and menu construction.

use std::sync::atomic::Ordering;

use heron_core::Msg;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};

use crate::state::AppState;

/// One row of the tray menu supplied by the UI (which owns localized labels).
///
/// `show` and `quit` are handled directly by the shell; every other `id` is forwarded back to the
/// webview as a `tray://action` event.
#[derive(serde::Deserialize)]
pub(crate) struct TrayItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    separator: bool,
}

/// Restore and focus the main application window when invoked from the tray or macOS Dock reopen.
pub(crate) fn show_main(app: &tauri::AppHandle) {
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

/// Install — or, on a language switch, rebuild — the system tray icon and menu.
#[tauri::command]
pub(crate) fn tray_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    items: Vec<TrayItem>,
) -> Result<(), Msg> {
    let tray_error = |e: tauri::Error| {
        eprintln!("heron: tray: {e}");
        Msg::new("E_INTERNAL")
    };
    let menu = tray_menu(&app, &items).map_err(tray_error)?;
    let mut guard = state.tray.lock().unwrap();
    if let Some(tray) = guard.as_ref() {
        return tray.set_menu(Some(menu)).map_err(tray_error);
    }

    #[cfg(target_os = "macos")]
    let icon = include_bytes!("../icons/tray-template.png");
    #[cfg(not(target_os = "macos"))]
    let icon = include_bytes!("../icons/32x32.png");
    let icon = Image::from_bytes(icon).map_err(tray_error)?;

    let builder = TrayIconBuilder::new()
        .menu(&menu)
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
