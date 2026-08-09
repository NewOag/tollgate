//! System tray icon + menu. The gateway keeps serving traffic while the
//! window is hidden, so closing the window (the `x` button) hides it
//! instead of quitting — only the tray's "Quit" item (or Cmd+Q, which
//! raises the same `RunEvent::ExitRequested`) actually shuts down.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

const SHOW_ID: &str = "show";
const HIDE_ID: &str = "hide";
const QUIT_ID: &str = "quit";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW_ID, "显示主窗口", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, HIDE_ID, "隐藏主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    let mut builder = TrayIconBuilder::new().menu(&menu).on_menu_event(|app, event| match event.id.as_ref() {
        SHOW_ID => show_main_window(app),
        HIDE_ID => hide_main_window(app),
        QUIT_ID => app.exit(0),
        _ => {}
    });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Called from the main window's `CloseRequested` handler so the `x`
/// button hides the window rather than closing it.
pub fn hide_instead_of_close(window: &tauri::WebviewWindow, api: &tauri::CloseRequestApi) {
    api.prevent_close();
    let _ = window.hide();
}

/// Called once from `RunEvent::ExitRequested` (tray "Quit" or Cmd+Q):
/// drains the embedded gateway and closes the store, then lets the exit
/// proceed for real. Guarded so the second, self-triggered `app.exit(0)`
/// doesn't re-enter the drain.
pub fn handle_exit_requested(app: &AppHandle, api: &tauri::ExitRequestApi) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    api.prevent_exit();

    let state = app.state::<AppState>();
    state.graceful_shutdown();

    app.exit(0);
}
