use crate::database::{self, DbState, FavouriteApp};
use crate::win_api;
use std::collections::HashMap;
use std::process::Command;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[tauri::command]
pub fn get_all_settings(state: State<'_, DbState>) -> Result<HashMap<String, String>, String> {
    database::get_all_settings(&state)
}

#[tauri::command]
pub fn set_setting(state: State<'_, DbState>, key: String, value: String) -> Result<(), String> {
    database::set_setting(&state, &key, &value)
}

#[tauri::command]
pub fn restore_default_settings(
    state: State<'_, DbState>,
) -> Result<HashMap<String, String>, String> {
    database::restore_default_settings(&state)
}

#[tauri::command]
pub fn update_global_shortcut(
    app: AppHandle,
    state: State<'_, DbState>,
    shortcut: String,
) -> Result<(), String> {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return Err("Shortcut cannot be empty".to_string());
    }
    let _ = app.global_shortcut().unregister_all();
    app.global_shortcut()
        .register(trimmed)
        .map_err(|e| format!("Failed to register shortcut '{trimmed}': {e}"))?;
    database::set_setting(&state, "global_shortcut", trimmed)
}

#[tauri::command]
pub fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("Failed to read autostart state: {e}"))
}

#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("Failed to enable autostart: {e}"))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("Failed to disable autostart: {e}"))?;
    }
    manager
        .is_enabled()
        .map_err(|e| format!("Failed to verify autostart state: {e}"))
}

#[tauri::command]
pub fn add_favourite_app(
    state: State<'_, DbState>,
    name: String,
    executable_path: String,
    arguments: Option<String>,
    icon_b64: Option<String>,
) -> Result<FavouriteApp, String> {
    database::add_favourite_app(&state, &name, &executable_path, arguments, icon_b64)
}

#[tauri::command]
pub fn list_favourite_apps(state: State<'_, DbState>) -> Result<Vec<FavouriteApp>, String> {
    database::list_favourite_apps(&state)
}

#[tauri::command]
pub fn delete_favourite_app(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::delete_favourite_app(&state, &id)
}

#[tauri::command]
pub fn launch_favourite_app(
    executable_path: String,
    arguments: Option<String>,
) -> Result<(), String> {
    match arguments {
        Some(args) if !args.trim().is_empty() => {
            let split_args: Vec<&str> = args.split_whitespace().collect();
            Command::new(&executable_path)
                .args(split_args)
                .spawn()
                .map_err(|e| format!("Failed to launch '{executable_path}': {e}"))?;
            Ok(())
        }
        _ => win_api::open_file_default(&executable_path),
    }
}
