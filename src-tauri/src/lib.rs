mod capture;
mod mic;
mod settings;
mod tasks;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_window(app);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let toggle_mic = Shortcut::new(Some(Modifiers::ALT), Code::Space);
    let toggle_win = Shortcut::new(Some(Modifiers::ALT), Code::KeyJ);
    let stop_speaking = Shortcut::new(Some(Modifiers::ALT), Code::Period);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &toggle_win {
                        toggle_window(app);
                    } else if shortcut == &toggle_mic {
                        let _ = app.emit("shortcut", "toggle-mic");
                    } else if shortcut == &stop_speaking {
                        let _ = app.emit("shortcut", "stop-speaking");
                    }
                })
                .build(),
        )
        .manage(tasks::TaskStore::default())
        .manage(capture::MicState::default())
        .setup(move |app| {
            tasks::load_index(app.handle());

            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
                let _ = apply_vibrancy(&win, NSVisualEffectMaterial::HudWindow, Some(NSVisualEffectState::Active), Some(18.0));
            }

            for sc in [toggle_mic, toggle_win, stop_speaking] {
                if let Err(e) = app.global_shortcut().register(sc) {
                    eprintln!("Could not register shortcut {sc:?}: {e}");
                }
            }

            let show = MenuItem::with_id(app, "show", "Show Jarvis    ⌥J", true, None::<&str>)?;
            let mic = MenuItem::with_id(app, "mic", "Talk / mute    ⌥Space", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Jarvis", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &mic, &quit])?;
            let mut tray = TrayIconBuilder::with_id("jarvis").tooltip("Jarvis").menu(&menu).on_menu_event(|app, ev| {
                match ev.id.as_ref() {
                    "show" => show_window(app),
                    "mic" => {
                        show_window(app);
                        let _ = app.emit("shortcut", "toggle-mic");
                    }
                    "quit" => app.exit(0),
                    _ => {}
                }
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Jarvis running in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            tasks::list_tasks,
            tasks::start_task,
            tasks::start_image,
            tasks::import_attachments,
            tasks::read_image,
            tasks::reveal_path,
            tasks::followup_task,
            tasks::cancel_task,
            tasks::read_report,
            tasks::reveal_task,
            tasks::append_note,
            tasks::read_notes,
            tasks::codex_status,
            tasks::codex_models,
            mic::mic_status,
            mic::request_mic,
            mic::open_mic_settings,
            capture::mic_start,
            capture::mic_stop,
            capture::list_mics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Jarvis")
        .run(|app, event| {
            // Clicking the Dock icon brings the hidden window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_window(app);
            }
            let _ = (app, event);
        });
}
