use crate::database::{self, DbState, HeldFile, HoldFilesBatchResult, QuickFolder};
use crate::win_api;
use tauri::State;

#[tauri::command]
pub fn hold_file(
    state: State<'_, DbState>,
    path: String,
    copy_to_vault: Option<bool>,
) -> Result<HeldFile, String> {
    let vault = copy_to_vault.unwrap_or(false);
    database::add_held_file(&state, &path, vault)
}

#[tauri::command]
pub fn hold_files_batch(
    state: State<'_, DbState>,
    paths: Vec<String>,
    copy_to_vault: Option<bool>,
) -> Result<HoldFilesBatchResult, String> {
    let vault = copy_to_vault.unwrap_or(false);
    let mut added = Vec::new();
    let mut skipped_duplicates = Vec::new();
    let mut failed = Vec::new();

    for path in paths {
        match database::add_held_file(&state, &path, vault) {
            Ok(item) => added.push(item),
            Err(e) if e == "DUPLICATE" => skipped_duplicates.push(path),
            Err(_) => failed.push(path),
        }
    }

    Ok(HoldFilesBatchResult {
        added,
        skipped_duplicates,
        failed,
    })
}

#[tauri::command]
pub fn list_held_files(state: State<'_, DbState>) -> Result<Vec<HeldFile>, String> {
    database::list_held_files(&state)
}

#[tauri::command]
pub fn toggle_pin_held_file(state: State<'_, DbState>, id: String) -> Result<bool, String> {
    database::toggle_pin_held_file(&state, &id)
}

#[tauri::command]
pub fn copy_held_file_to_vault(state: State<'_, DbState>, id: String) -> Result<HeldFile, String> {
    database::copy_existing_held_file_to_vault(&state, &id)
}

#[tauri::command]
pub fn remove_held_file(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::remove_held_file(&state, &id)
}

#[tauri::command]
pub fn clear_held_files(state: State<'_, DbState>) -> Result<usize, String> {
    database::clear_held_files(&state)
}

#[tauri::command]
pub fn check_file_available(path: String) -> bool {
    database::is_path_available(&path)
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    win_api::open_file_default(&path)
}

#[tauri::command]
pub fn reveal_file_in_explorer(path: String) -> Result<(), String> {
    win_api::reveal_in_explorer(&path)
}

#[tauri::command]
pub fn add_quick_folder(
    state: State<'_, DbState>,
    name: String,
    path: String,
    icon: Option<String>,
) -> Result<QuickFolder, String> {
    database::add_folder(&state, &name, &path, icon)
}

#[tauri::command]
pub fn list_quick_folders(state: State<'_, DbState>) -> Result<Vec<QuickFolder>, String> {
    database::list_folders(&state)
}

#[tauri::command]
pub fn delete_quick_folder(state: State<'_, DbState>, id: String) -> Result<(), String> {
    database::delete_folder(&state, &id)
}
