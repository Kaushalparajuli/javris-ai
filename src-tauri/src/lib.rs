mod briefings;
mod browser;
mod capture;
mod chats;
mod google;
mod mic;
mod search;
mod settings;
mod tasks;
mod wakeword;

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
        .plugin(tauri_plugin_notification::init())
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
        .manage(browser::BrowserState::default())
        .manage(wakeword::WakeState::default())
        .setup(move |app| {
            tasks::load_index(app.handle());
            tauri::async_runtime::spawn(briefings::run_scheduler(app.handle().clone()));
            tauri::async_runtime::spawn(browser::run_reaper(app.handle().clone()));
            if settings::load(app.handle()).wake_word {
                if let Err(e) = wakeword::start(app.handle()) {
                    eprintln!("Couldn't start listening for \"hey Jarvis\": {e}");
                }
            }

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
            tasks::install_codex,
            tasks::write_document,
            tasks::new_document,
            tasks::read_document,
            tasks::save_document,
            tasks::save_export,
            tasks::read_import,
            tasks::import_document,
            tasks::save_to_source,
            tasks::export_pdf,
            chats::list_chats,
            search::search,
            briefings::list_briefings,
            browser::start_browse,
            browser::browser_status,
            browser::install_browser_tools,
            browser::open_browser_profile,
            browser::browser_frame,
            browser::browser_input,
            browser::close_browser,
            wakeword::wake_word_set,
            google::google_status,
            google::google_connect,
            google::google_disconnect,
            google::calendar_list,
            google::calendar_create,
            google::mail_search,
            google::mail_read,
            google::mail_draft,
            google::mail_send_draft,
            briefings::save_briefing,
            briefings::delete_briefing,
            briefings::run_briefing_now,
            chats::load_chat,
            chats::new_chat,
            chats::save_chat,
            chats::rename_chat,
            chats::delete_chat,
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
            // Don't leave Jarvis's hidden browser running after Jarvis quits.
            if let tauri::RunEvent::Exit = event {
                browser::shutdown(app);
            }
            let _ = (app, event);
        });
}
