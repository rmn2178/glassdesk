mod commands;
mod database;
mod win_api;

use database::DbState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

fn toggle_overlay_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(true);
        if visible {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = win_api::apply_overlay_window_styles(&window);
            let _ = app.emit("glassdesk://shortcut-toggle", true);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_overlay_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;

            let db_state = DbState::initialize(&app_data_dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let settings = database::get_all_settings(&db_state).unwrap_or_default();
            let collapsed_height: u32 = settings
                .get("collapsed_height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(36);
            let monitor_index: Option<usize> = settings
                .get("monitor_index")
                .and_then(|v| v.parse().ok());
            let shortcut_str = settings
                .get("global_shortcut")
                .cloned()
                .unwrap_or_else(|| "Alt+Space".to_string());

            app.manage(db_state);

            if let Some(window) = app.get_webview_window("main") {
                let _ = win_api::apply_overlay_window_styles(&window);
                let _ =
                    win_api::set_overlay_bounds(&window, collapsed_height, false, monitor_index);
            }

            // Native Windows System Tray for Show/Hide & clean Quit
            let toggle_item =
                MenuItem::with_id(app, "toggle", "Show / Hide GLASSDESK", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit GLASSDESK", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&toggle_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("GLASSDESK — Windows Dynamic Island")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => toggle_overlay_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            let _tray = tray_builder.build(app)?;

            // Register global toggle shortcut (non-fatal if already bound)
            let _ = app.global_shortcut().register(shortcut_str.as_str());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Files & Quick Folders
            commands::files::hold_file,
            commands::files::hold_files_batch,
            commands::files::list_held_files,
            commands::files::toggle_pin_held_file,
            commands::files::copy_held_file_to_vault,
            commands::files::remove_held_file,
            commands::files::clear_held_files,
            commands::files::check_file_available,
            commands::files::open_file,
            commands::files::reveal_file_in_explorer,
            commands::files::add_quick_folder,
            commands::files::list_quick_folders,
            commands::files::delete_quick_folder,
            // Tasks & Reminders
            commands::tasks::create_task,
            commands::tasks::list_tasks,
            commands::tasks::update_task,
            commands::tasks::delete_task,
            commands::tasks::create_reminder,
            commands::tasks::list_reminders,
            commands::tasks::dismiss_reminder,
            commands::tasks::delete_reminder,
            // Windows 11 Native System & Overlay
            commands::system::get_system_snapshot,
            commands::system::get_active_window,
            commands::system::get_battery_status,
            commands::system::get_keyboard_layout,
            commands::system::get_audio_status,
            commands::system::set_system_volume,
            commands::system::toggle_system_mute,
            commands::system::get_network_status,
            commands::system::is_fullscreen_active,
            commands::system::list_monitors,
            commands::system::set_overlay_bounds,
            commands::system::set_overlay_visible,
            commands::system::open_windows_settings,
            // Settings, Autostart & Favourite Apps
            commands::settings::get_all_settings,
            commands::settings::set_setting,
            commands::settings::restore_default_settings,
            commands::settings::update_global_shortcut,
            commands::settings::get_autostart_enabled,
            commands::settings::set_autostart_enabled,
            commands::settings::add_favourite_app,
            commands::settings::list_favourite_apps,
            commands::settings::delete_favourite_app,
            commands::settings::launch_favourite_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GLASSDESK Tauri application");
}
