mod api_server;
mod clipboard;
mod config;
mod db;
mod hardware;
mod media;
mod pc_deck;
mod phone_transfer;
mod win32;

use api_server::{AgentSessionState, ApiStore, LiveTilePayload};
use config::{load_config, save_config, AppConfig};
use db::{ClipboardItem, Database, LauncherSlot, NotificationItem, ReminderItem, TodoItem};
use hardware::{HardwareMonitor, HardwareStats};
use media::{LyricsResponse, MediaInfo};
use pc_deck::{PcDeckManager, PcDeckStatus};
use phone_transfer::{PhoneTransferManager, PhoneTransferSession};
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use win32::{AudioState, PrivacySensors};

struct AppState {
    config: Mutex<AppConfig>,
    db: Arc<Database>,
    hw: HardwareMonitor,
    api_store: Arc<ApiStore>,
    phone_transfer: PhoneTransferManager,
    pc_deck: PcDeckManager,
}

fn position_window_top(window: &WebviewWindow, position: &str) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let mon_size = monitor.size();
        let mon_pos = monitor.position();
        let win_w = 640i32;
        let x = match position {
            "left" => mon_pos.x + 24,
            "right" => mon_pos.x + (mon_size.width as i32) - win_w - 24,
            _ => mon_pos.x + ((mon_size.width as i32) - win_w) / 2,
        };
        let _ = window.set_position(PhysicalPosition::new(x, mon_pos.y));
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn update_config(
    window: WebviewWindow,
    state: State<AppState>,
    new_config: AppConfig,
) -> Result<AppConfig, String> {
    save_config(&new_config)?;
    position_window_top(&window, &new_config.position);
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        win32::set_capture_exclusion(hwnd.0 as isize, new_config.hide_from_capture);
    }
    *state.config.lock().unwrap() = new_config.clone();
    Ok(new_config)
}

#[tauri::command]
fn set_click_through(window: WebviewWindow, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_hardware_stats(state: State<AppState>) -> HardwareStats {
    state.hw.sample()
}

#[tauri::command]
fn get_media_info() -> MediaInfo {
    media::get_media_info()
}

#[tauri::command]
fn send_media_command(cmd: String) -> Result<(), String> {
    media::media_command(&cmd)
}

#[tauri::command]
async fn get_synced_lyrics(artist: String, title: String) -> LyricsResponse {
    media::fetch_lyrics(artist, title).await
}

#[tauri::command]
fn get_audio_state() -> AudioState {
    win32::get_audio_state()
}

#[tauri::command]
fn set_master_volume(percent: u32) -> Result<(), String> {
    win32::set_master_volume(percent)
}

#[tauri::command]
fn toggle_mic_mute() -> Result<bool, String> {
    win32::toggle_mic_mute()
}

#[tauri::command]
fn pick_screen_color() -> Result<String, String> {
    let hex = win32::pick_cursor_hex_color()?;
    let _ = clipboard::set_clipboard_text(&hex);
    Ok(hex)
}

#[tauri::command]
fn lock_pc() {
    win32::lock_workstation();
}

#[tauri::command]
fn get_privacy_sensors() -> PrivacySensors {
    win32::get_privacy_sensors()
}

#[tauri::command]
fn list_clipboard(state: State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    let hours = state.config.lock().unwrap().clipboard_retention_hours;
    state.db.list_clipboard(hours)
}

#[tauri::command]
fn copy_clipboard_text(text: String) -> Result<(), String> {
    clipboard::set_clipboard_text(&text)
}

#[tauri::command]
fn toggle_clipboard_pin(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.toggle_clipboard_pin(id)
}

#[tauri::command]
fn delete_clipboard_item(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.delete_clipboard_item(id)
}

#[tauri::command]
fn clear_unpinned_clipboard(state: State<AppState>) -> Result<(), String> {
    state.db.clear_unpinned_clipboard()
}

#[tauri::command]
fn list_reminders(state: State<AppState>) -> Result<Vec<ReminderItem>, String> {
    state.db.list_reminders()
}

#[tauri::command]
fn add_reminder(
    state: State<AppState>,
    title: String,
    due_at: i64,
    repeat_rule: String,
) -> Result<ReminderItem, String> {
    state.db.add_reminder(&title, due_at, &repeat_rule)
}

#[tauri::command]
fn set_reminder_done(state: State<AppState>, id: i64, done: bool) -> Result<(), String> {
    state.db.update_reminder_done(id, done)
}

#[tauri::command]
fn snooze_reminder(state: State<AppState>, id: i64, minutes: i64) -> Result<(), String> {
    state.db.snooze_reminder(id, minutes)
}

#[tauri::command]
fn delete_reminder(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.delete_reminder(id)
}

#[tauri::command]
fn list_todos(state: State<AppState>) -> Result<Vec<TodoItem>, String> {
    state.db.list_todos()
}

#[tauri::command]
fn add_todo(state: State<AppState>, text: String) -> Result<TodoItem, String> {
    state.db.add_todo(&text)
}

#[tauri::command]
fn toggle_todo(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.toggle_todo(id)
}

#[tauri::command]
fn delete_todo(state: State<AppState>, id: i64) -> Result<(), String> {
    state.db.delete_todo(id)
}

#[tauri::command]
fn list_notifications(state: State<AppState>) -> Result<Vec<NotificationItem>, String> {
    state.db.list_notifications()
}

#[tauri::command]
fn clear_notifications(state: State<AppState>) -> Result<(), String> {
    state.db.clear_notifications()
}

#[tauri::command]
fn list_launcher_slots(state: State<AppState>) -> Result<Vec<LauncherSlot>, String> {
    state.db.list_launcher_slots()
}

#[tauri::command]
fn update_launcher_slot(
    state: State<AppState>,
    slot_index: i64,
    label: String,
    target_path: String,
) -> Result<(), String> {
    state.db.update_launcher_slot(slot_index, &label, &target_path)
}

#[tauri::command]
fn launch_target(target_path: String) -> Result<(), String> {
    let trimmed = target_path.trim();
    if trimmed.is_empty() {
        return Err("Empty launcher target".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", trimmed])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_agents(state: State<AppState>) -> Vec<AgentSessionState> {
    state.api_store.list_agents()
}

#[tauri::command]
fn list_live_tiles(state: State<AppState>) -> Vec<LiveTilePayload> {
    state.api_store.list_tiles()
}

#[tauri::command]
async fn start_phone_transfer(
    state: State<'_, AppState>,
    file_paths: Vec<String>,
) -> Result<PhoneTransferSession, String> {
    state.phone_transfer.start(file_paths).await
}

#[tauri::command]
fn stop_phone_transfer(state: State<AppState>) {
    state.phone_transfer.stop();
}

#[tauri::command]
async fn start_pc_deck(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PcDeckStatus, String> {
    state.pc_deck.start(app).await
}

#[tauri::command]
fn stop_pc_deck(state: State<AppState>) {
    state.pc_deck.stop();
}

#[tauri::command]
fn approve_pc_deck(state: State<AppState>, allow: bool) -> PcDeckStatus {
    state.pc_deck.approve(allow)
}

#[tauri::command]
fn get_pc_deck_status(state: State<AppState>) -> PcDeckStatus {
    state.pc_deck.status()
}

#[tauri::command]
async fn fetch_weather(lat: f64, lon: f64) -> Result<serde_json::Value, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m",
        lat, lon
    );
    let client = reqwest::Client::builder()
        .user_agent("NotchGlass/0.1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())
}

fn register_shortcuts(app: &AppHandle) {
    let shortcuts = [
        ("Ctrl+Alt+N", "hotkey-toggle-expand"),
        ("Ctrl+Alt+H", "hotkey-toggle-hide"),
        ("Ctrl+Alt+P", "hotkey-media-play-pause"),
        ("Ctrl+Alt+M", "hotkey-mic-mute"),
        ("Ctrl+Alt+C", "hotkey-color-picker"),
    ];
    for (combo, event_name) in shortcuts {
        let ev = event_name.to_string();
        let _ = app.global_shortcut().on_shortcut(combo, move |app_handle, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                match ev.as_str() {
                    "hotkey-media-play-pause" => {
                        let _ = media::media_command("play_pause");
                        let _ = app_handle.emit("hud-toast", "Media Play / Pause");
                    }
                    "hotkey-mic-mute" => {
                        if let Ok(muted) = win32::toggle_mic_mute() {
                            let msg = if muted { "Microphone Muted" } else { "Microphone Unmuted" };
                            let _ = app_handle.emit("hud-toast", msg);
                        }
                    }
                    "hotkey-color-picker" => {
                        if let Ok(hex) = win32::pick_cursor_hex_color() {
                            let _ = clipboard::set_clipboard_text(&hex);
                            let _ = app_handle.emit("hud-toast", format!("Picked {}", hex));
                        }
                    }
                    _ => {
                        let _ = app_handle.emit(&ev, ());
                    }
                }
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = load_config();
    let db = Arc::new(Database::init().expect("Failed to initialize SQLite database"));
    let api_store = Arc::new(ApiStore::new());

    let state = AppState {
        config: Mutex::new(cfg.clone()),
        db: db.clone(),
        hw: HardwareMonitor::new(),
        api_store: api_store.clone(),
        phone_transfer: PhoneTransferManager::new(),
        pc_deck: PcDeckManager::new(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();

            // Configure main overlay window
            if let Some(window) = app.get_webview_window("main") {
                position_window_top(&window, &cfg.position);
                #[cfg(target_os = "windows")]
                if let Ok(hwnd) = window.hwnd() {
                    win32::apply_overlay_styles(hwnd.0 as isize);
                    win32::set_capture_exclusion(hwnd.0 as isize, cfg.hide_from_capture);
                }
            }

            // Build system tray icon
            let toggle_i = MenuItem::with_id(app, "toggle", "Toggle Island", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit NotchGlass", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_i, &show_i, &quit_i])?;
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => {
                        let _ = app.emit("hotkey-toggle-expand", ());
                    }
                    "show" => {
                        let _ = app.emit("hotkey-toggle-hide", ());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Start background services
            clipboard::spawn_clipboard_watcher(handle.clone(), db.clone());
            api_server::spawn_loopback_api(handle.clone(), db.clone(), api_store.clone());
            register_shortcuts(&handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            update_config,
            set_click_through,
            get_hardware_stats,
            get_media_info,
            send_media_command,
            get_synced_lyrics,
            get_audio_state,
            set_master_volume,
            toggle_mic_mute,
            pick_screen_color,
            lock_pc,
            get_privacy_sensors,
            list_clipboard,
            copy_clipboard_text,
            toggle_clipboard_pin,
            delete_clipboard_item,
            clear_unpinned_clipboard,
            list_reminders,
            add_reminder,
            set_reminder_done,
            snooze_reminder,
            delete_reminder,
            list_todos,
            add_todo,
            toggle_todo,
            delete_todo,
            list_notifications,
            clear_notifications,
            list_launcher_slots,
            update_launcher_slot,
            launch_target,
            list_agents,
            list_live_tiles,
            start_phone_transfer,
            stop_phone_transfer,
            start_pc_deck,
            stop_pc_deck,
            approve_pc_deck,
            get_pc_deck_status,
            fetch_weather
        ])
        .run(tauri::generate_context!())
        .expect("error while running NotchGlass");
}
