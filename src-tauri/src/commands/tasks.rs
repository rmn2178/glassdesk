use crate::database::{self, DbState, NotificationItem, ReminderItem, TaskItem};
use tauri::State;

#[tauri::command]
pub fn create_task(
    state: State<'_, DbState>,
    title: String,
    notes: Option<String>,
    category: Option<String>,
    priority: Option<i32>,
    due_at: Option<String>,
    reminder_at: Option<String>,
) -> Result<TaskItem, String> {
    database::create_task(
        &state,
        &title,
        notes,
        category,
        priority.unwrap_or(1),
        due_at,
        reminder_at,
    )
}

#[tauri::command]
pub fn list_tasks(state: State<'_, DbState>) -> Result<Vec<TaskItem>, String> {
    database::list_tasks(&state)
}

#[tauri::command]
pub fn update_task(
    state: State<'_, DbState>,
    id: String,
    title: Option<String>,
    notes: Option<String>,
    category: Option<String>,
    completed: Option<bool>,
    priority: Option<i32>,
    due_at: Option<String>,
    reminder_at: Option<String>,
) -> Result<TaskItem, String> {
    database::update_task(
        &state,
        &id,
        title,
        notes,
        category,
        completed,
        priority,
        due_at,
        reminder_at,
    )
}

#[tauri::command]
pub fn delete_task(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::delete_task(&state, &id)
}

#[tauri::command]
pub fn create_reminder(
    state: State<'_, DbState>,
    title: String,
    remind_at: String,
    repeat_rule: Option<String>,
) -> Result<ReminderItem, String> {
    database::create_reminder(&state, &title, &remind_at, repeat_rule)
}

#[tauri::command]
pub fn list_reminders(state: State<'_, DbState>) -> Result<Vec<ReminderItem>, String> {
    database::list_reminders(&state)
}

#[tauri::command]
pub fn dismiss_reminder(
    state: State<'_, DbState>,
    id: String,
    dismissed: bool,
) -> Result<(), String> {
    database::dismiss_reminder(&state, &id, dismissed)
}

#[tauri::command]
pub fn delete_reminder(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::delete_reminder(&state, &id)
}

#[tauri::command]
pub fn push_notification(
    state: State<'_, DbState>,
    title: String,
    message: String,
    category: Option<String>,
) -> Result<NotificationItem, String> {
    database::push_notification(&state, &title, &message, category.as_deref().unwrap_or("system"))
}

#[tauri::command]
pub fn list_notifications(state: State<'_, DbState>) -> Result<Vec<NotificationItem>, String> {
    database::list_notifications(&state)
}

#[tauri::command]
pub fn mark_notification_read(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::mark_notification_read(&state, &id)
}

#[tauri::command]
pub fn delete_notification(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::delete_notification(&state, &id)
}

#[tauri::command]
pub fn clear_notifications(state: State<'_, DbState>) -> Result<usize, String> {
    database::clear_notifications(&state)
}
