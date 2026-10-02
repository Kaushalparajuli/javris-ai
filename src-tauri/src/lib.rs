mod briefings;
mod browser;
mod applog;
mod capture;
mod chats;
mod context;
mod fileindex;
mod google;
mod google_docs;
mod google_drive;
mod google_sheets;
mod meeting;
mod memory;
mod mac;
mod mini;
mod mic;
mod policy;
mod preview;
mod routines;
mod search;
mod settings;
mod tasks;
mod verify;
mod wakeword;
mod websearch;
mod workspaces;
mod youtube;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub(crate) fn show_window(app: &AppHandle) {
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
    let palette = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::Space);
    let mini_win = Shortcut::new(Some(Modifiers::ALT), Code::KeyM);

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
                        // Note what the user is looking at before Jarvis's window can take focus.
                        context::snapshot(app);
                        let _ = app.emit("shortcut", "toggle-mic");
                    } else if shortcut == &palette {
                        // Read what's in front before Jarvis's own window takes focus.
                        context::snapshot_blocking(app);
                        show_window(app);
                        let _ = app.emit("shortcut", "palette");
                    } else if shortcut == &mini_win {
                        mini::toggle(app);
                    } else if shortcut == &stop_speaking {
                        // Stop doubles as the kill switch for Mac control.
                        mac::halt();
                        let _ = app.emit("shortcut", "stop-speaking");
                    }
                })
                .build(),
        )
        .manage(tasks::TaskStore::default())
        .manage(capture::MicState::default())
        .manage(browser::BrowserState::default())
        .manage(wakeword::WakeState::default())
        .manage(context::ContextState::default())
        .setup(move |app| {
            tasks::load_index(app.handle());
            routines::install_builtin_skills(app.handle());
            memory::import_notes(app.handle());
            tauri::async_runtime::spawn(memory::embed_missing(app.handle().clone()));
            tauri::async_runtime::spawn(fileindex::rescan(app.handle().clone()));
            tauri::async_runtime::spawn(fileindex::run_scheduler(app.handle().clone()));
            tauri::async_runtime::spawn(briefings::run_scheduler(app.handle().clone()));
            tauri::async_runtime::spawn(routines::run_scheduler(app.handle().clone()));
            tauri::async_runtime::spawn(browser::run_reaper(app.handle().clone()));
            if settings::load(app.handle()).wake_word {
                if let Err(e) = wakeword::start(app.handle()) {
                    eprintln!("Couldn't start listening for \"hey Jarvis\": {e}");
                }
            }

            mini::prepare(app.handle());

            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
                let _ = apply_vibrancy(&win, NSVisualEffectMaterial::HudWindow, Some(NSVisualEffectState::Active), Some(18.0));
            }

            for sc in [toggle_mic, toggle_win, stop_speaking, palette, mini_win] {
                if let Err(e) = app.global_shortcut().register(sc) {
                    eprintln!("Could not register shortcut {sc:?}: {e}");
                }
            }

            let show = MenuItem::with_id(app, "show", "Show Jarvis    ⌥J", true, None::<&str>)?;
            let mic = MenuItem::with_id(app, "mic", "Talk / mute    ⌥Space", true, None::<&str>)?;
            let cmd = MenuItem::with_id(app, "palette", "Type a command    ⌥⇧Space", true, None::<&str>)?;
            let mini_item = MenuItem::with_id(app, "mini", "Mini Jarvis    ⌥M", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Jarvis", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &mic, &cmd, &mini_item, &quit])?;
            let mut tray = TrayIconBuilder::with_id("jarvis").tooltip("Jarvis").menu(&menu).on_menu_event(|app, ev| {
                match ev.id.as_ref() {
                    "show" => show_window(app),
                    "mic" => {
                        show_window(app);
                        let _ = app.emit("shortcut", "toggle-mic");
                    }
                    "palette" => {
                        show_window(app);
                        let _ = app.emit("shortcut", "palette");
                    }
                    "mini" => mini::toggle(app),
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
            applog::app_log,
            settings::get_settings,
            settings::save_settings,
            memory::memory_add,
            memory::memory_search,
            memory::memory_list,
            memory::memory_brief,
            memory::memory_update,
            memory::memory_delete,
            workspaces::list_workspaces,
            workspaces::get_active_workspace,
            workspaces::save_workspace,
            workspaces::create_project,
            workspaces::project_files,
            workspaces::project_add_files,
            workspaces::project_remove_file,
            workspaces::project_import_document,
            workspaces::project_sites,
            workspaces::project_new_site,
            preview::preview_info,
            preview::preview_files,
            preview::preview_read,
            workspaces::project_read_file,
            workspaces::delete_workspace,
            workspaces::set_active_workspace,
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
            tasks::codex_login,
            tasks::codex_login_cancel,
            tasks::codex_logout,
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
            google::calendar_get,
            google::calendar_update,
            google::calendar_delete,
            google::mail_search,
            google::mail_read,
            google::mail_draft,
            google::mail_send_draft,
            google_drive::drive_search,
            google_drive::drive_get,
            google_drive::drive_read,
            google_drive::drive_upload,
            google_docs::doc_create,
            google_docs::doc_read,
            google_docs::doc_append,
            google_docs::doc_replace_text,
            google_sheets::sheet_info,
            google_sheets::sheet_create,
            google_sheets::sheet_read,
            google_sheets::sheet_append,
            google_sheets::sheet_update,
            youtube::yt_search,
            youtube::yt_video,
            youtube::yt_playlist_items,
            youtube::yt_mine,
            websearch::web_search,
            briefings::save_briefing,
            briefings::delete_briefing,
            briefings::run_briefing_now,
            routines::list_routines,
            routines::save_routine,
            routines::delete_routine,
            routines::list_runs,
            routines::run_routine,
            routines::cancel_run,
            routines::answer_run,
            routines::list_know_how,
            routines::save_know_how,
            routines::delete_know_how,
            routines::learn_know_how,
            chats::load_chat,
            chats::new_chat,
            chats::save_chat,
            chats::rename_chat,
            chats::move_chat,
            chats::pin_chat,
            chats::set_chat_title,
            chats::delete_chat,
            tasks::codex_models,
            mic::mic_status,
            mic::request_mic,
            mic::open_mic_settings,
            capture::mic_start,
            capture::mic_stop,
            capture::list_mics,
            context::context_status,
            context::request_accessibility,
            context::get_context,
            context::replace_selection,
            context::request_screen_recording,
            context::look_at_screen,
            fileindex::index_status,
            fileindex::index_add_folder,
            fileindex::index_remove_folder,
            fileindex::index_refresh,
            fileindex::index_search,
            mac::mac_read,
            mac::mac_open,
            mac::mac_click,
            mac::mac_type,
            mac::mac_key,
            mac::mac_scroll,
            mac::mac_resume,
            mac::mac_halt,
            meeting::meeting_begin,
            meeting::meeting_append,
            meeting::meeting_finish,
            mini::mini_show,
            mini::mini_hide,
            mini::mini_resize,
            mini::show_main,
            policy::audit_log,
            policy::notify_approval,
            tasks::start_code_task,
            tasks::resolve_project,
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
