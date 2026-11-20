use crate::config::app_data_dir;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: String, // "text" | "link" | "file"
    pub content: String,
    pub is_pinned: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    pub id: i64,
    pub title: String,
    pub due_at: i64,
    pub repeat_rule: String, // "once" | "daily" | "weekdays" | "weekly"
    pub is_done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: i64,
    pub text: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    pub id: i64,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub accent: String,
    pub received_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSlot {
    pub slot_index: i64,
    pub label: String,
    pub target_path: String,
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn init() -> Result<Self, String> {
        let db_path = app_data_dir().join("notchglass.db");
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT UNIQUE NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS reminders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                due_at INTEGER NOT NULL,
                repeat_rule TEXT NOT NULL DEFAULT 'once',
                is_done INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                app_name TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                accent TEXT NOT NULL DEFAULT '#ffffff',
                received_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS launcher_slots (
                slot_index INTEGER PRIMARY KEY,
                label TEXT NOT NULL,
                target_path TEXT NOT NULL
            );
            ",
        )
        .map_err(|e| e.to_string())?;

        // Seed 6 default launcher slots if empty
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM launcher_slots", [], |r| r.get(0))
            .unwrap_or(0);
        if count == 0 {
            let defaults = [
                (1, "Terminal", "wt.exe"),
                (2, "Explorer", "explorer.exe"),
                (3, "Notepad", "notepad.exe"),
                (4, "Calculator", "calc.exe"),
                (5, "Settings", "ms-settings:"),
                (6, "Browser", "https://github.com"),
            ];
            for (idx, label, target) in defaults {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO launcher_slots (slot_index, label, target_path) VALUES (?1, ?2, ?3)",
                    params![idx, label, target],
                );
            }
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_clipboard(&self, kind: &str, content: &str) -> Result<bool, String> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let mut hasher = Sha256::new();
        hasher.update(trimmed.as_bytes());
        let hash = hex::encode(hasher.finalize());
        let now = Utc::now().timestamp();

        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE content_hash = ?1",
                params![hash],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if exists > 0 {
            conn.execute(
                "UPDATE clipboard_items SET created_at = ?1 WHERE content_hash = ?2",
                params![now, hash],
            )
            .map_err(|e| e.to_string())?;
            Ok(false)
        } else {
            conn.execute(
                "INSERT INTO clipboard_items (kind, content, content_hash, is_pinned, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
                params![kind, trimmed, hash, now],
            )
            .map_err(|e| e.to_string())?;
            Ok(true)
        }
    }

    pub fn list_clipboard(&self, retention_hours: u32) -> Result<Vec<ClipboardItem>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let cutoff = Utc::now().timestamp() - (retention_hours as i64 * 3600);
        let _ = conn.execute(
            "DELETE FROM clipboard_items WHERE is_pinned = 0 AND created_at < ?1",
            params![cutoff],
        );

        let mut stmt = conn
            .prepare(
                "SELECT id, kind, content, is_pinned, created_at
                 FROM clipboard_items
                 ORDER BY is_pinned DESC, created_at DESC
                 LIMIT 80",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ClipboardItem {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    content: row.get(2)?,
                    is_pinned: row.get::<_, i64>(3)? != 0,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn toggle_clipboard_pin(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clipboard_items SET is_pinned = CASE WHEN is_pinned = 0 THEN 1 ELSE 0 END WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_clipboard_item(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM clipboard_items WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear_unpinned_clipboard(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM clipboard_items WHERE is_pinned = 0", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn add_reminder(&self, title: &str, due_at: i64, repeat_rule: &str) -> Result<ReminderItem, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO reminders (title, due_at, repeat_rule, is_done) VALUES (?1, ?2, ?3, 0)",
            params![title, due_at, repeat_rule],
        )
        .map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        Ok(ReminderItem {
            id,
            title: title.to_string(),
            due_at,
            repeat_rule: repeat_rule.to_string(),
            is_done: false,
        })
    }

    pub fn list_reminders(&self) -> Result<Vec<ReminderItem>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, title, due_at, repeat_rule, is_done FROM reminders ORDER BY is_done ASC, due_at ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ReminderItem {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    due_at: row.get(2)?,
                    repeat_rule: row.get(3)?,
                    is_done: row.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn update_reminder_done(&self, id: i64, done: bool) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE reminders SET is_done = ?1 WHERE id = ?2",
            params![if done { 1 } else { 0 }, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn snooze_reminder(&self, id: i64, minutes: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let next_due = Utc::now().timestamp() + minutes * 60;
        conn.execute(
            "UPDATE reminders SET due_at = ?1, is_done = 0 WHERE id = ?2",
            params![next_due, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_reminder(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM reminders WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_todos(&self) -> Result<Vec<TodoItem>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, text, completed FROM todos ORDER BY completed ASC, id DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TodoItem {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    completed: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn add_todo(&self, text: &str) -> Result<TodoItem, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO todos (text, completed) VALUES (?1, 0)", params![text])
            .map_err(|e| e.to_string())?;
        Ok(TodoItem {
            id: conn.last_insert_rowid(),
            text: text.to_string(),
            completed: false,
        })
    }

    pub fn toggle_todo(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE todos SET completed = CASE WHEN completed = 0 THEN 1 ELSE 0 END WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_todo(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM todos WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn add_notification(&self, app_name: &str, title: &str, body: &str, accent: &str) -> Result<NotificationItem, String> {
        let now = Utc::now().timestamp();
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO notifications (app_name, title, body, accent, received_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![app_name, title, body, accent, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(NotificationItem {
            id: conn.last_insert_rowid(),
            app_name: app_name.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            accent: accent.to_string(),
            received_at: now,
        })
    }

    pub fn list_notifications(&self) -> Result<Vec<NotificationItem>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, app_name, title, body, accent, received_at FROM notifications ORDER BY received_at DESC LIMIT 50")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(NotificationItem {
                    id: row.get(0)?,
                    app_name: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    accent: row.get(4)?,
                    received_at: row.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn clear_notifications(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM notifications", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_launcher_slots(&self) -> Result<Vec<LauncherSlot>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT slot_index, label, target_path FROM launcher_slots ORDER BY slot_index ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LauncherSlot {
                    slot_index: row.get(0)?,
                    label: row.get(1)?,
                    target_path: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn update_launcher_slot(&self, slot_index: i64, label: &str, target_path: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO launcher_slots (slot_index, label, target_path) VALUES (?1, ?2, ?3)
             ON CONFLICT(slot_index) DO UPDATE SET label = excluded.label, target_path = excluded.target_path",
            params![slot_index, label, target_path],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
