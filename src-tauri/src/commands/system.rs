use crate::database::{self, DbState, StorageOptimizationReport};
use crate::win_api::{
    self, ActiveWindowInfo, AudioStatus, BatteryStatus, MediaSessionInfo, MonitorDescriptor,
    NetworkStatus, SystemSnapshot,
};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_system_snapshot() -> SystemSnapshot {
    win_api::get_system_snapshot()
}

#[tauri::command]
pub fn get_active_window() -> ActiveWindowInfo {
    win_api::get_active_window()
}

#[tauri::command]
pub fn get_battery_status() -> BatteryStatus {
    win_api::get_battery_status()
}

#[tauri::command]
pub fn get_keyboard_layout() -> String {
    win_api::get_keyboard_layout_code()
}

#[tauri::command]
pub fn get_audio_status() -> AudioStatus {
    win_api::get_audio_status()
}

#[tauri::command]
pub fn set_system_volume(level: f32) -> Result<AudioStatus, String> {
    win_api::set_volume(level)
}

#[tauri::command]
pub fn toggle_system_mute() -> Result<AudioStatus, String> {
    win_api::toggle_mute()
}

#[tauri::command]
pub fn get_network_status() -> NetworkStatus {
    win_api::get_network_status()
}

#[tauri::command]
pub fn is_fullscreen_active() -> bool {
    win_api::is_fullscreen_app_active()
}

#[tauri::command]
pub fn list_monitors(app: AppHandle) -> Result<Vec<MonitorDescriptor>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main overlay window not found".to_string())?;
    Ok(win_api::enumerate_monitors(&window))
}

#[tauri::command]
pub fn set_overlay_bounds(
    app: AppHandle,
    height: u32,
    focusable: bool,
    monitor_index: Option<usize>,
    floating_width: Option<u32>,
    horizontal_offset: Option<i32>,
    always_on_top: Option<bool>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main overlay window not found".to_string())?;
    win_api::set_overlay_bounds(
        &window,
        height,
        focusable,
        monitor_index,
        floating_width,
        horizontal_offset.unwrap_or(0),
        always_on_top.unwrap_or(true),
    )
}

#[tauri::command]
pub fn set_overlay_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main overlay window not found".to_string())?;
    if visible {
        window.show().map_err(|e| e.to_string())?;
        let _ = win_api::apply_overlay_window_styles(&window);
    } else {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_windows_settings(uri: String) -> Result<(), String> {
    win_api::open_windows_settings(&uri)
}

#[tauri::command]
pub fn trim_memory() {
    win_api::trim_working_set();
}

#[tauri::command]
pub fn optimize_storage(state: State<'_, DbState>) -> Result<StorageOptimizationReport, String> {
    let report = database::optimize_storage(&state)?;
    win_api::trim_working_set();
    Ok(report)
}

#[tauri::command]
pub fn get_media_session() -> MediaSessionInfo {
    win_api::get_media_session()
}

#[tauri::command]
pub fn media_control(action: String) -> Result<MediaSessionInfo, String> {
    win_api::media_control(&action)
}
