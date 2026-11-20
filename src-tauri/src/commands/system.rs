use crate::win_api::{
    self, ActiveWindowInfo, AudioStatus, BatteryStatus, MonitorDescriptor, NetworkStatus,
    SystemSnapshot,
};
use tauri::{AppHandle, Manager};

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
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main overlay window not found".to_string())?;
    win_api::set_overlay_bounds(&window, height, focusable, monitor_index)
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
