use crate::db::Database;
use arboard::Clipboard;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn classify_text(text: &str) -> &'static str {
    let t = text.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        "link"
    } else if (t.len() > 3 && t.as_bytes()[1] == b':' && (t.as_bytes()[2] == b'\\' || t.as_bytes()[2] == b'/'))
        || t.starts_with("\\\\")
    {
        "file"
    } else {
        "text"
    }
}

pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_string()).map_err(|e| e.to_string())
}

pub fn spawn_clipboard_watcher(app: AppHandle, db: Arc<Database>) {
    std::thread::spawn(move || {
        let mut last_seen = String::new();
        loop {
            if let Ok(mut cb) = Clipboard::new() {
                if let Ok(text) = cb.get_text() {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() && trimmed != last_seen && trimmed.len() <= 20_000 {
                        last_seen = trimmed.clone();
                        let kind = classify_text(&trimmed);
                        if let Ok(inserted) = db.upsert_clipboard(kind, &trimmed) {
                            if inserted {
                                let _ = app.emit("clipboard-updated", ());
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(900));
        }
    });
}
