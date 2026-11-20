use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::Utc;
use image::imageops::FilterType;
use image::ImageFormat;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use uuid::Uuid;

pub struct DbState {
    pub conn: Mutex<Connection>,
    pub vault_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldFile {
    pub id: String,
    pub original_path: String,
    pub vault_path: Option<String>,
    pub display_name: String,
    pub extension: String,
    pub category: String,
    pub is_dir: bool,
    pub size_bytes: i64,
    pub is_vault_copy: bool,
    pub thumbnail_b64: Option<String>,
    pub pinned: bool,
    pub available: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldFilesBatchResult {
    pub added: Vec<HeldFile>,
    pub skipped_duplicates: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub notes: Option<String>,
    pub category: String,
    pub completed: bool,
    pub priority: i32,
    pub due_at: Option<String>,
    pub reminder_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    pub id: String,
    pub title: String,
    pub remind_at: String,
    pub dismissed: bool,
    pub repeat_rule: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickFolder {
    pub id: String,
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub available: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavouriteApp {
    pub id: String,
    pub name: String,
    pub executable_path: String,
    pub arguments: Option<String>,
    pub icon_b64: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
}

pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("enabled", "true"),
    ("auto_hide_fullscreen", "true"),
    ("theme", "dark"),
    ("clock_format", "12h"),
    ("accent_color", "silver"),
    ("collapsed_height", "36"),
    ("monitor_index", "0"),
    ("global_shortcut", "Alt+Space"),
    ("vault_copy_by_default", "false"),
    ("auto_collapse_after_drop", "true"),
];

impl DbState {
    pub fn initialize(app_data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir)
            .map_err(|e| format!("Failed to create app data directory: {e}"))?;

        let vault_dir = app_data_dir.join("vault");
        fs::create_dir_all(&vault_dir)
            .map_err(|e| format!("Failed to create vault directory: {e}"))?;

        let db_path = app_data_dir.join("glassdesk.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open SQLite database: {e}"))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("Failed to enable WAL mode: {e}"))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| format!("Failed to set synchronous mode: {e}"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| format!("Failed to enable foreign_keys: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| format!("Failed to set busy_timeout: {e}"))?;

        run_migrations(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            vault_dir,
        })
    }

    pub fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "Database mutex poisoned".to_string())
    }
}

fn run_migrations(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS held_files (
            id TEXT PRIMARY KEY,
            original_path TEXT NOT NULL UNIQUE,
            vault_path TEXT,
            display_name TEXT NOT NULL,
            extension TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT 'other',
            is_dir INTEGER NOT NULL DEFAULT 0,
            size_bytes INTEGER NOT NULL DEFAULT 0,
            is_vault_copy INTEGER NOT NULL DEFAULT 0,
            thumbnail_b64 TEXT,
            pinned INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            notes TEXT,
            category TEXT NOT NULL DEFAULT 'Personal',
            completed INTEGER NOT NULL DEFAULT 0,
            priority INTEGER NOT NULL DEFAULT 1,
            due_at TEXT,
            reminder_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS reminders (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            remind_at TEXT NOT NULL,
            dismissed INTEGER NOT NULL DEFAULT 0,
            repeat_rule TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS folders (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            icon TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS favourite_applications (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            executable_path TEXT NOT NULL UNIQUE,
            arguments TEXT,
            icon_b64 TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_held_files_pinned ON held_files(pinned DESC, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed, priority DESC, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_reminders_due ON reminders(dismissed, remind_at ASC);
        "#,
    )
    .map_err(|e| format!("Failed to run database migrations: {e}"))?;

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
        params![now],
    )
    .map_err(|e| format!("Failed to record schema migration: {e}"))?;

    for (k, v) in DEFAULT_SETTINGS {
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![k, v, now],
        )
        .map_err(|e| format!("Failed to seed default setting '{k}': {e}"))?;
    }

    Ok(())
}

// ============================================================================
// File & Thumbnail Helpers
// ============================================================================

pub fn is_path_available(path: &str) -> bool {
    Path::new(path).exists()
}

pub fn classify_file_category(extension: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "other";
    }
    match extension {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "ico" | "heic" | "tiff" => {
            "images"
        }
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" | "flv" | "m4v" => "videos",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf"
        | "csv" | "odt" | "ods" | "odp" | "epub" | "tex" | "json" | "xml" | "yaml" | "yml" => {
            "documents"
        }
        _ => "other",
    }
}

pub fn copy_to_vault(source_path: &Path, vault_dir: &Path) -> Result<PathBuf, String> {
    if !source_path.exists() {
        return Err(format!(
            "Source file does not exist: {}",
            source_path.display()
        ));
    }
    if source_path.is_dir() {
        return Err("Vault copy supports individual files only (not directories)".to_string());
    }

    fs::create_dir_all(vault_dir)
        .map_err(|e| format!("Failed to ensure vault directory: {e}"))?;

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let unique_name = format!("{}_{}", Uuid::new_v4().simple(), file_name);
    let dest_path = vault_dir.join(unique_name);

    fs::copy(source_path, &dest_path)
        .map_err(|e| format!("Failed to copy file to vault: {e}"))?;

    Ok(dest_path)
}

pub fn generate_image_thumbnail(path: &Path) -> Option<String> {
    const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;

    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
    ) {
        return None;
    }

    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_IMAGE_BYTES {
        return None;
    }

    let img = image::open(path).ok()?;
    let thumb = img.resize_to_fill(64, 64, FilterType::Lanczos3);

    let mut buffer = Cursor::new(Vec::new());
    thumb.write_to(&mut buffer, ImageFormat::Png).ok()?;

    let encoded = BASE64_STANDARD.encode(buffer.into_inner());
    Some(format!("data:image/png;base64,{encoded}"))
}

// ============================================================================
// Held Files CRUD
// ============================================================================

pub fn add_held_file(
    state: &DbState,
    original_path: &str,
    copy_into_vault: bool,
) -> Result<HeldFile, String> {
    let normalized = original_path.trim().to_string();
    let src = Path::new(&normalized);
    if !src.exists() {
        return Err(format!("Path does not exist or is inaccessible: {normalized}"));
    }

    // Duplicate path check
    {
        let conn = state.lock_conn()?;
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM held_files WHERE original_path = ?1",
                params![normalized],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed duplicate check: {e}"))?;

        if existing.is_some() {
            return Err("DUPLICATE".to_string());
        }
    }

    let is_dir = src.is_dir();
    let display_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&normalized)
        .to_string();

    let extension = if is_dir {
        "folder".to_string()
    } else {
        src.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default()
    };

    let category = classify_file_category(&extension, is_dir).to_string();
    let size_bytes = if is_dir {
        0
    } else {
        fs::metadata(src).map(|m| m.len() as i64).unwrap_or(0)
    };

    let vault_path = if copy_into_vault && !is_dir {
        let copied = copy_to_vault(src, &state.vault_dir)?;
        Some(copied.to_string_lossy().to_string())
    } else {
        None
    };

    let thumb_source = vault_path.as_deref().map(Path::new).unwrap_or(src);
    let thumbnail_b64 = if !is_dir {
        generate_image_thumbnail(thumb_source)
    } else {
        None
    };

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let is_vault_copy = vault_path.is_some();

    let conn = state.lock_conn()?;
    conn.execute(
        r#"
        INSERT INTO held_files (
            id, original_path, vault_path, display_name, extension, category,
            is_dir, size_bytes, is_vault_copy, thumbnail_b64, pinned, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)
        "#,
        params![
            id,
            normalized,
            vault_path,
            display_name,
            extension,
            category,
            is_dir as i32,
            size_bytes,
            is_vault_copy as i32,
            thumbnail_b64,
            now
        ],
    )
    .map_err(|e| format!("Failed to insert held file: {e}"))?;

    Ok(HeldFile {
        id,
        original_path: normalized,
        vault_path,
        display_name,
        extension,
        category,
        is_dir,
        size_bytes,
        is_vault_copy,
        thumbnail_b64,
        pinned: false,
        available: true,
        created_at: now,
    })
}

pub fn list_held_files(state: &DbState) -> Result<Vec<HeldFile>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, original_path, vault_path, display_name, extension, category,
                   is_dir, size_bytes, is_vault_copy, thumbnail_b64, pinned, created_at
            FROM held_files
            ORDER BY pinned DESC, created_at DESC
            "#,
        )
        .map_err(|e| format!("Failed to prepare held_files query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            let original_path: String = row.get(1)?;
            let vault_path: Option<String> = row.get(2)?;
            let effective_path = vault_path.as_deref().unwrap_or(&original_path);
            let available = is_path_available(effective_path);

            Ok(HeldFile {
                id: row.get(0)?,
                original_path,
                vault_path,
                display_name: row.get(3)?,
                extension: row.get(4)?,
                category: row.get(5)?,
                is_dir: row.get::<_, i32>(6)? != 0,
                size_bytes: row.get(7)?,
                is_vault_copy: row.get::<_, i32>(8)? != 0,
                thumbnail_b64: row.get(9)?,
                pinned: row.get::<_, i32>(10)? != 0,
                available,
                created_at: row.get(11)?,
            })
        })
        .map_err(|e| format!("Failed to query held_files: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect held_files: {e}"))
}

pub fn toggle_pin_held_file(state: &DbState, id: &str) -> Result<bool, String> {
    let conn = state.lock_conn()?;
    let current: i32 = conn
        .query_row(
            "SELECT pinned FROM held_files WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Held file not found: {e}"))?;

    let next = if current == 0 { 1 } else { 0 };
    conn.execute(
        "UPDATE held_files SET pinned = ?1 WHERE id = ?2",
        params![next, id],
    )
    .map_err(|e| format!("Failed to toggle pin: {e}"))?;

    Ok(next != 0)
}

pub fn copy_existing_held_file_to_vault(state: &DbState, id: &str) -> Result<HeldFile, String> {
    let (original_path, existing_vault): (String, Option<String>) = {
        let conn = state.lock_conn()?;
        conn.query_row(
            "SELECT original_path, vault_path FROM held_files WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("Held file not found: {e}"))?
    };

    if existing_vault.is_none() {
        let copied = copy_to_vault(Path::new(&original_path), &state.vault_dir)?;
        let copied_str = copied.to_string_lossy().to_string();
        let conn = state.lock_conn()?;
        conn.execute(
            "UPDATE held_files SET vault_path = ?1, is_vault_copy = 1 WHERE id = ?2",
            params![copied_str, id],
        )
        .map_err(|e| format!("Failed to update vault path: {e}"))?;
    }

    list_held_files(state)?
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| "Held file missing after vault copy".to_string())
}

pub fn remove_held_file(state: &DbState, id: &str) -> Result<(), String> {
    let conn = state.lock_conn()?;
    let vault_path: Option<String> = conn
        .query_row(
            "SELECT vault_path FROM held_files WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to query held file: {e}"))?
        .flatten();

    conn.execute("DELETE FROM held_files WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete held file: {e}"))?;

    if let Some(vp) = vault_path {
        let path = Path::new(&vp);
        if path.starts_with(&state.vault_dir) && path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

/// Clears all held file references from SQLite (never deletes original user files;
/// cleans up only GLASSDESK's internal vault copies).
pub fn clear_held_files(state: &DbState) -> Result<usize, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare("SELECT vault_path FROM held_files WHERE vault_path IS NOT NULL")
        .map_err(|e| format!("Failed to query vault paths: {e}"))?;
    let vault_paths: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| format!("Failed to read vault paths: {e}"))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let deleted = conn
        .execute("DELETE FROM held_files", [])
        .map_err(|e| format!("Failed to clear held files: {e}"))?;

    for vp in vault_paths {
        let path = Path::new(&vp);
        if path.starts_with(&state.vault_dir) && path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    Ok(deleted)
}

// ============================================================================
// Tasks CRUD
// ============================================================================

fn normalize_task_category(cat: Option<String>) -> String {
    match cat.as_deref().map(str::trim) {
        Some("College") => "College".to_string(),
        Some("Personal") => "Personal".to_string(),
        Some("Projects") => "Projects".to_string(),
        Some("Other") => "Other".to_string(),
        _ => "Personal".to_string(),
    }
}

pub fn create_task(
    state: &DbState,
    title: &str,
    notes: Option<String>,
    category: Option<String>,
    priority: i32,
    due_at: Option<String>,
    reminder_at: Option<String>,
) -> Result<TaskItem, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Task title cannot be empty".to_string());
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let cat = normalize_task_category(category);
    let prio = priority.clamp(0, 3);
    let conn = state.lock_conn()?;

    conn.execute(
        r#"
        INSERT INTO tasks (
            id, title, notes, category, completed, priority, due_at, reminder_at, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?8)
        "#,
        params![id, trimmed, notes, cat, prio, due_at, reminder_at, now],
    )
    .map_err(|e| format!("Failed to create task: {e}"))?;

    Ok(TaskItem {
        id,
        title: trimmed.to_string(),
        notes,
        category: cat,
        completed: false,
        priority: prio,
        due_at,
        reminder_at,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub fn list_tasks(state: &DbState) -> Result<Vec<TaskItem>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, title, notes, category, completed, priority, due_at, reminder_at, created_at, updated_at
            FROM tasks
            ORDER BY completed ASC, priority DESC, due_at ASC, created_at DESC
            "#,
        )
        .map_err(|e| format!("Failed to prepare tasks query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(TaskItem {
                id: row.get(0)?,
                title: row.get(1)?,
                notes: row.get(2)?,
                category: row.get(3)?,
                completed: row.get::<_, i32>(4)? != 0,
                priority: row.get(5)?,
                due_at: row.get(6)?,
                reminder_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| format!("Failed to query tasks: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect tasks: {e}"))
}

pub fn update_task(
    state: &DbState,
    id: &str,
    title: Option<String>,
    notes: Option<String>,
    category: Option<String>,
    completed: Option<bool>,
    priority: Option<i32>,
    due_at: Option<String>,
    reminder_at: Option<String>,
) -> Result<TaskItem, String> {
    let conn = state.lock_conn()?;
    let mut existing = conn
        .query_row(
            "SELECT id, title, notes, category, completed, priority, due_at, reminder_at, created_at, updated_at FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok(TaskItem {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    notes: row.get(2)?,
                    category: row.get(3)?,
                    completed: row.get::<_, i32>(4)? != 0,
                    priority: row.get(5)?,
                    due_at: row.get(6)?,
                    reminder_at: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        )
        .map_err(|e| format!("Task not found: {e}"))?;

    if let Some(t) = title {
        let trimmed = t.trim().to_string();
        if !trimmed.is_empty() {
            existing.title = trimmed;
        }
    }
    if notes.is_some() {
        existing.notes = notes;
    }
    if let Some(cat) = category {
        existing.category = normalize_task_category(Some(cat));
    }
    if let Some(c) = completed {
        existing.completed = c;
    }
    if let Some(p) = priority {
        existing.priority = p.clamp(0, 3);
    }
    if due_at.is_some() {
        existing.due_at = due_at.filter(|s| !s.trim().is_empty());
    }
    if reminder_at.is_some() {
        existing.reminder_at = reminder_at.filter(|s| !s.trim().is_empty());
    }
    existing.updated_at = Utc::now().to_rfc3339();

    conn.execute(
        r#"
        UPDATE tasks
        SET title = ?1, notes = ?2, category = ?3, completed = ?4, priority = ?5,
            due_at = ?6, reminder_at = ?7, updated_at = ?8
        WHERE id = ?9
        "#,
        params![
            existing.title,
            existing.notes,
            existing.category,
            existing.completed as i32,
            existing.priority,
            existing.due_at,
            existing.reminder_at,
            existing.updated_at,
            id
        ],
    )
    .map_err(|e| format!("Failed to update task: {e}"))?;

    Ok(existing)
}

pub fn delete_task(state: &DbState, id: &str) -> Result<(), String> {
    let conn = state.lock_conn()?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete task: {e}"))?;
    Ok(())
}

// ============================================================================
// Reminders CRUD
// ============================================================================

pub fn create_reminder(
    state: &DbState,
    title: &str,
    remind_at: &str,
    repeat_rule: Option<String>,
) -> Result<ReminderItem, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let conn = state.lock_conn()?;

    conn.execute(
        r#"
        INSERT INTO reminders (id, title, remind_at, dismissed, repeat_rule, created_at)
        VALUES (?1, ?2, ?3, 0, ?4, ?5)
        "#,
        params![id, title.trim(), remind_at, repeat_rule, now],
    )
    .map_err(|e| format!("Failed to create reminder: {e}"))?;

    Ok(ReminderItem {
        id,
        title: title.trim().to_string(),
        remind_at: remind_at.to_string(),
        dismissed: false,
        repeat_rule,
        created_at: now,
    })
}

pub fn list_reminders(state: &DbState) -> Result<Vec<ReminderItem>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, title, remind_at, dismissed, repeat_rule, created_at
            FROM reminders
            ORDER BY dismissed ASC, remind_at ASC
            "#,
        )
        .map_err(|e| format!("Failed to prepare reminders query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ReminderItem {
                id: row.get(0)?,
                title: row.get(1)?,
                remind_at: row.get(2)?,
                dismissed: row.get::<_, i32>(3)? != 0,
                repeat_rule: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query reminders: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect reminders: {e}"))
}

pub fn dismiss_reminder(state: &DbState, id: &str, dismissed: bool) -> Result<(), String> {
    let conn = state.lock_conn()?;
    conn.execute(
        "UPDATE reminders SET dismissed = ?1 WHERE id = ?2",
        params![dismissed as i32, id],
    )
    .map_err(|e| format!("Failed to update reminder: {e}"))?;
    Ok(())
}

pub fn delete_reminder(state: &DbState, id: &str) -> Result<(), String> {
    let conn = state.lock_conn()?;
    conn.execute("DELETE FROM reminders WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete reminder: {e}"))?;
    Ok(())
}

// ============================================================================
// Quick Folders & Favourite Applications CRUD
// ============================================================================

pub fn add_folder(
    state: &DbState,
    name: &str,
    path: &str,
    icon: Option<String>,
) -> Result<QuickFolder, String> {
    let normalized = path.trim().to_string();
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let conn = state.lock_conn()?;

    let next_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        r#"
        INSERT INTO folders (id, name, path, icon, sort_order, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![id, name.trim(), normalized, icon, next_order, now],
    )
    .map_err(|e| format!("Failed to add folder: {e}"))?;

    Ok(QuickFolder {
        id,
        name: name.trim().to_string(),
        available: is_path_available(&normalized),
        path: normalized,
        icon,
        sort_order: next_order,
        created_at: now,
    })
}

pub fn list_folders(state: &DbState) -> Result<Vec<QuickFolder>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare("SELECT id, name, path, icon, sort_order, created_at FROM folders ORDER BY sort_order ASC")
        .map_err(|e| format!("Failed to prepare folders query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            let path: String = row.get(2)?;
            let available = is_path_available(&path);
            Ok(QuickFolder {
                id: row.get(0)?,
                name: row.get(1)?,
                path,
                icon: row.get(3)?,
                sort_order: row.get(4)?,
                available,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query folders: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect folders: {e}"))
}

pub fn delete_folder(state: &DbState, id: &str) -> Result<(), String> {
    let conn = state.lock_conn()?;
    conn.execute("DELETE FROM folders WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete folder: {e}"))?;
    Ok(())
}

pub fn add_favourite_app(
    state: &DbState,
    name: &str,
    executable_path: &str,
    arguments: Option<String>,
    icon_b64: Option<String>,
) -> Result<FavouriteApp, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let conn = state.lock_conn()?;

    let next_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM favourite_applications",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        r#"
        INSERT INTO favourite_applications (id, name, executable_path, arguments, icon_b64, sort_order, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            id,
            name.trim(),
            executable_path.trim(),
            arguments,
            icon_b64,
            next_order,
            now
        ],
    )
    .map_err(|e| format!("Failed to add favourite application: {e}"))?;

    Ok(FavouriteApp {
        id,
        name: name.trim().to_string(),
        executable_path: executable_path.trim().to_string(),
        arguments,
        icon_b64,
        sort_order: next_order,
        created_at: now,
    })
}

pub fn list_favourite_apps(state: &DbState) -> Result<Vec<FavouriteApp>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, name, executable_path, arguments, icon_b64, sort_order, created_at
            FROM favourite_applications
            ORDER BY sort_order ASC
            "#,
        )
        .map_err(|e| format!("Failed to prepare favourite_applications query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(FavouriteApp {
                id: row.get(0)?,
                name: row.get(1)?,
                executable_path: row.get(2)?,
                arguments: row.get(3)?,
                icon_b64: row.get(4)?,
                sort_order: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query favourite_applications: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect favourite_applications: {e}"))
}

pub fn delete_favourite_app(state: &DbState, id: &str) -> Result<(), String> {
    let conn = state.lock_conn()?;
    conn.execute(
        "DELETE FROM favourite_applications WHERE id = ?1",
        params![id],
    )
    .map_err(|e| format!("Failed to delete favourite application: {e}"))?;
    Ok(())
}

// ============================================================================
// Settings Key-Value CRUD
// ============================================================================

pub fn get_all_settings(state: &DbState) -> Result<HashMap<String, String>, String> {
    let conn = state.lock_conn()?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(|e| format!("Failed to prepare settings query: {e}"))?;

    let mut map = HashMap::new();
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| format!("Failed to query settings: {e}"))?;

    for pair in rows {
        let (k, v) = pair.map_err(|e| format!("Failed to read setting row: {e}"))?;
        map.insert(k, v);
    }
    Ok(map)
}

pub fn set_setting(state: &DbState, key: &str, value: &str) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let conn = state.lock_conn()?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
        params![key, value, now],
    )
    .map_err(|e| format!("Failed to save setting '{key}': {e}"))?;
    Ok(())
}

pub fn restore_default_settings(state: &DbState) -> Result<HashMap<String, String>, String> {
    let now = Utc::now().to_rfc3339();
    {
        let conn = state.lock_conn()?;
        conn.execute("DELETE FROM settings", [])
            .map_err(|e| format!("Failed to reset settings: {e}"))?;
        for (k, v) in DEFAULT_SETTINGS {
            conn.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
                params![k, v, now],
            )
            .map_err(|e| format!("Failed to restore default setting '{k}': {e}"))?;
        }
    }
    get_all_settings(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (DbState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("glassdesk_test_{}", Uuid::new_v4()));
        let state = DbState::initialize(&dir).expect("init db");
        (state, dir)
    }

    #[test]
    fn test_tasks_and_settings_crud() {
        let (state, dir) = temp_db();

        let task = create_task(
            &state,
            "Finish OS Lab",
            Some("Chapter 4".to_string()),
            Some("College".to_string()),
            2,
            Some("2026-12-01".to_string()),
            None,
        )
        .expect("create task");
        assert_eq!(task.category, "College");
        assert!(!task.completed);

        let updated = update_task(
            &state,
            &task.id,
            None,
            None,
            None,
            Some(true),
            None,
            None,
            None,
        )
        .expect("update task");
        assert!(updated.completed);

        let all = list_tasks(&state).expect("list tasks");
        assert_eq!(all.len(), 1);

        set_setting(&state, "theme", "light").expect("set theme");
        let settings = get_all_settings(&state).expect("get settings");
        assert_eq!(settings.get("theme").map(String::as_str), Some("light"));

        let restored = restore_default_settings(&state).expect("restore defaults");
        assert_eq!(restored.get("theme").map(String::as_str), Some("dark"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_held_files_and_vault_safety() {
        let (state, dir) = temp_db();
        let sample = dir.join("notes.txt");
        fs::write(&sample, "hello glassdesk").expect("write sample");

        let held = add_held_file(&state, sample.to_str().unwrap(), false).expect("hold file");
        assert_eq!(held.category, "documents");
        assert!(!held.is_vault_copy);

        // Duplicate rejected
        let dup = add_held_file(&state, sample.to_str().unwrap(), false);
        assert!(dup.is_err());

        // Copy to vault
        let vaulted = copy_existing_held_file_to_vault(&state, &held.id).expect("copy to vault");
        assert!(vaulted.is_vault_copy);
        let vault_path = vaulted.vault_path.clone().unwrap();
        assert!(Path::new(&vault_path).exists());

        // Remove from shelf removes vault copy but NEVER deletes original
        remove_held_file(&state, &held.id).expect("remove held file");
        assert!(sample.exists(), "Original file must never be deleted");
        assert!(!Path::new(&vault_path).exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
