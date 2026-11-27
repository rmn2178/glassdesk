//! User-facing dock text. The language follows
//! the Windows display language unless the user overrides it in Settings.
//! `&` marks the menu access key.

use std::sync::atomic::{AtomicU8, Ordering};

use nosih_config::Language;
use nosih_shell::system::{UiLanguage, ui_language};

mod ar;
mod bn;
mod en;
mod es;
mod fr;
mod hi;
mod pt;
mod zh;

pub struct Strings {
    pub open: &'static str,
    pub new_window: &'static str,
    pub keep_in_dock: &'static str,
    pub show_in_explorer: &'static str,
    pub quit_app: &'static str,
    pub end_task: &'static str,
    pub end_task_failed: &'static str,
    pub empty_bin: &'static str,
    pub turn_hiding_on: &'static str,
    pub turn_hiding_off: &'static str,
    pub quit_dock: &'static str,
    pub start: &'static str,
    pub remove: &'static str,
    pub close_window: &'static str,
    pub ungroup: &'static str,
    pub settings: &'static str,
    pub run_as_admin: &'static str,
    pub show_desktop: &'static str,
    pub task_manager: &'static str,
    pub taskbar_settings: &'static str,
    pub usage_title: &'static str,
    pub usage_refresh: &'static str,
    pub usage_refreshing: &'static str,
    pub usage_sign_in: &'static str,
    pub usage_claude_sign_in: &'static str,
    pub usage_unavailable: &'static str,
    pub usage_loading: &'static str,
    pub usage_stale: &'static str,
    pub usage_resets: &'static str,
    pub usage_updated: &'static str,
    pub usage_used: &'static str,
    pub media_title: &'static str,
    pub media_no_output: &'static str,
    pub media_no_progress: &'static str,
    pub folder_empty: &'static str,
    pub folder_unavailable: &'static str,
    pub change_folder: &'static str,
    pub remove_from_dock: &'static str,
    pub shell_operation_failed: &'static str,
    pub keyboard_settings: &'static str,
    pub keyboard: &'static str,
    pub jump_pinned: &'static str,
    pub jump_frequent: &'static str,
    pub jump_recent: &'static str,
    pub jump_tasks: &'static str,
    pub cc_wifi: &'static str,
    pub cc_bluetooth: &'static str,
    pub cc_focus: &'static str,
    pub cc_dark_mode: &'static str,
    pub cc_light_mode: &'static str,
    pub cc_snip: &'static str,
    pub cc_display: &'static str,
    pub cc_sound: &'static str,
    pub cc_on: &'static str,
    pub cc_off: &'static str,
    pub cc_muted: &'static str,
    pub cc_ethernet: &'static str,
    pub cc_lock: &'static str,
    pub cc_settings: &'static str,
    pub notif_title: &'static str,
    pub notif_clear_all: &'static str,
    pub notif_empty: &'static str,
    pub notif_settings: &'static str,
    pub notif_now: &'static str,
    pub notif_network_offline: &'static str,
    pub notif_network_online: &'static str,
    pub notif_bt_on: &'static str,
    pub notif_bt_off: &'static str,
    pub sys_about: &'static str,
    pub sys_sleep: &'static str,
    pub sys_restart: &'static str,
    pub sys_shutdown: &'static str,
    pub sys_force_quit: &'static str,
    pub power_saver: &'static str,
    pub power_balanced: &'static str,
    pub power_performance: &'static str,
    pub spotlight_placeholder: &'static str,
    pub spotlight_empty: &'static str,
    pub spotlight_math_hint: &'static str,
    pub spotlight_cmd_hint: &'static str,
    pub spotlight_app_hint: &'static str,
    pub hud_volume: &'static str,
    pub hud_muted: &'static str,
    pub hud_brightness: &'static str,
    pub app_menu_file: &'static str,
    pub app_menu_window: &'static str,
    pub app_menu_hide: &'static str,
    pub app_menu_hide_others: &'static str,
    pub app_menu_quit: &'static str,
    pub app_menu_new_window: &'static str,
    pub app_menu_close_window: &'static str,
    pub app_menu_minimize: &'static str,
    pub app_menu_zoom: &'static str,
    pub app_menu_center: &'static str,
    pub app_menu_bring_all: &'static str,
}

/// 0 = not chosen yet (follow Windows). Nonzero values identify static catalogs.
static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    let chosen = match language {
        Language::Auto => ui_language(),
        Language::Es => UiLanguage::Spanish,
        Language::En => UiLanguage::English,
        Language::Fr => UiLanguage::French,
        Language::Zh => UiLanguage::Chinese,
        Language::Hi => UiLanguage::Hindi,
        Language::Ar => UiLanguage::Arabic,
        Language::Bn => UiLanguage::Bengali,
        Language::Pt => UiLanguage::Portuguese,
    };
    let stored = match chosen {
        UiLanguage::Spanish => 1,
        UiLanguage::English => 2,
        UiLanguage::French => 3,
        UiLanguage::Chinese => 4,
        UiLanguage::Hindi => 5,
        UiLanguage::Arabic => 6,
        UiLanguage::Bengali => 7,
        UiLanguage::Portuguese => 8,
    };
    LANGUAGE.store(stored, Ordering::Relaxed);
}

fn chosen_language() -> u8 {
    let language = LANGUAGE.load(Ordering::Relaxed);
    if language == 0 {
        set_language(Language::Auto);
        LANGUAGE.load(Ordering::Relaxed)
    } else {
        language
    }
}

pub fn text() -> &'static Strings {
    match chosen_language() {
        1 => &es::TEXT,
        3 => &fr::TEXT,
        4 => &zh::TEXT,
        5 => &hi::TEXT,
        6 => &ar::TEXT,
        7 => &bn::TEXT,
        8 => &pt::TEXT,
        _ => &en::TEXT,
    }
}

/// The language the text is in, never `Auto`. Part of the menu bar's
/// rendered-content key.
pub fn language() -> Language {
    match chosen_language() {
        1 => Language::Es,
        3 => Language::Fr,
        4 => Language::Zh,
        5 => Language::Hi,
        6 => Language::Ar,
        7 => Language::Bn,
        8 => Language::Pt,
        _ => Language::En,
    }
}
