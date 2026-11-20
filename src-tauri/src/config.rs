use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub shape: String,              // "notch" | "island"
    pub position: String,           // "center" | "left" | "right"
    pub theme: String,              // "liquid-glass" | "pure-black" | "black" | "graphite" | "acrylic" | "candy-glass" | "rose" | "lavender" | "peach" | "sakura" | "pure-white"
    pub glass_blur: u32,            // 0..32
    pub corner_radius: u32,         // 12..28
    pub hover_delay_ms: u32,        // 0..1500
    pub auto_hide: bool,
    pub hide_from_capture: bool,
    pub fullscreen_mode: String,    // "slim" | "hide" | "normal"
    pub resting_face: String,       // "clock" | "stats" | "media" | "pet"
    pub visualizer_style: String,   // "rounded" | "pixel"
    pub last_active_tab: String,
    pub clock_24h: bool,
    pub weather_city: String,
    pub weather_lat: f64,
    pub weather_lon: f64,
    pub clipboard_retention_hours: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shape: "island".to_string(),
            position: "center".to_string(),
            theme: "liquid-glass".to_string(),
            glass_blur: 20,
            corner_radius: 22,
            hover_delay_ms: 140,
            auto_hide: false,
            hide_from_capture: false,
            fullscreen_mode: "slim".to_string(),
            resting_face: "clock".to_string(),
            visualizer_style: "rounded".to_string(),
            last_active_tab: "music".to_string(),
            clock_24h: false,
            weather_city: "Chennai".to_string(),
            weather_lat: 13.0827,
            weather_lon: 80.2707,
            clipboard_retention_hours: 10,
        }
    }
}

pub fn app_data_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("com", "notchglass", "NotchGlass") {
        let dir = dirs.data_dir().to_path_buf();
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    let fallback = PathBuf::from(".");
    fallback
}

pub fn config_path() -> PathBuf {
    app_data_dir().join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) {
            return cfg;
        }
    }
    let cfg = AppConfig::default();
    let _ = save_config(&cfg);
    cfg
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let serialized = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(path, serialized).map_err(|e| e.to_string())
}
