//! Mini Jarvis: a small always-on-top window with the orb, what Jarvis is doing, and any approval
//! waiting for a click, so Jarvis stays usable while the main window is hidden in the menu bar.
//! It holds no state of its own: the main window sends it a snapshot and it sends back clicks.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const WIDTH: f64 = 300.0;

fn ensure(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window("mini") {
        return Ok(w);
    }
    let w = WebviewWindowBuilder::new(app, "mini", WebviewUrl::App("index.html".into()))
        .title("Jarvis")
        .inner_size(WIDTH, 120.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    // Top right of the screen it opens on, below the menu bar.
    if let Ok(Some(m)) = w.current_monitor() {
        let s = m.scale_factor();
        let x = m.position().x as f64 + m.size().width as f64 - (WIDTH + 16.0) * s;
        let y = m.position().y as f64 + 44.0 * s;
        let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
    Ok(w)
}

/// Create the window hidden at launch, so showing it later is instant and it is already listening.
pub fn prepare(app: &AppHandle) {
    if let Err(e) = ensure(app) {
        eprintln!("Couldn't prepare the mini window: {e}");
    }
}

pub fn show(app: &AppHandle) {
    if let Ok(w) = ensure(app) {
        let _ = w.show();
    }
}

pub fn toggle(app: &AppHandle) {
    if let Ok(w) = ensure(app) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
        }
    }
}

#[tauri::command]
pub async fn mini_show(app: AppHandle) {
    show(&app);
}

#[tauri::command]
pub async fn mini_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("mini") {
        let _ = w.hide();
    }
}

/// The window is only as tall as its content.
#[tauri::command]
pub async fn mini_resize(app: AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window("mini") {
        let _ = w.set_size(tauri::LogicalSize::new(WIDTH, height.clamp(70.0, 520.0)));
    }
}

/// Bring the full window forward (from the mini window's "Open" button).
#[tauri::command]
pub async fn show_main(app: AppHandle) {
    crate::show_window(&app);
}
