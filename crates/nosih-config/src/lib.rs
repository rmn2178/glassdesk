//! Persistent Nosih configuration stored in `%APPDATA%\Nosih\config.json`.

use std::{
    env,
    fs::{self, File, TryLockError},
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CURRENT_VERSION: u32 = 1;

/// Error reading or saving the configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("no se pudo acceder a `{path}`")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`{path}` no es una configuración válida")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("no se pudo serializar la configuración")]
    Serialize(#[from] serde_json::Error),
}

/// User settings. Missing fields use their default values, so adding fields
/// does not break existing configuration files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
    /// Pinned items in dock order: apps, and the ids of `groups`.
    pub pinned: Vec<String>,
    /// Pinned apps gathered in one dock icon each.
    pub groups: Vec<DockGroup>,
    pub recent: Vec<String>,
    /// The apps pinned to the Windows taskbar when the dock last looked, to
    /// bring pins and unpins made there (even while Nosih was closed) to the
    /// dock. `None` until it first looks.
    pub taskbar_pins: Option<Vec<String>>,
    #[serde(alias = "auto_hide")]
    pub auto_hide: bool,
    /// Hide only when the foreground app overlaps the dock; leave desktop visible.
    /// Missing in an existing file means the legacy always-visible mode. New
    /// installations and Reset use Config::default(), which enables this mode.
    #[serde(default)]
    pub smart_hide: bool,
    /// Icon side at rest, in logical pixels.
    pub icon_size: u32,
    pub magnification: bool,
    /// Side of the icon under the cursor, in logical pixels.
    pub magnified_size: u32,
    /// The screen edge the dock sits on. Bottom by default, as on macOS;
    /// never the top, which belongs to the menu bar.
    pub dock_position: DockPosition,
    pub dock_density: DockDensity,
    pub dock_auto_hide_speed: DockAutoHideSpeed,
    pub show_start: bool,
    pub show_trash: bool,
    pub show_downloads: bool,
    /// Keep Explorer pinned; otherwise it appears only while its windows are open.
    pub pin_explorer: bool,
    /// Genie effect when minimizing. On by default; while enabled, the dock
    /// turns off the system's own window transitions (see plan, "Efecto genio").
    pub genie: bool,
    /// How long the genie takes (issue #27). `Normal` is the original speed.
    pub genie_speed: GenieSpeed,
    /// Minimized windows get a live thumbnail in the right section of the
    /// dock. Off by default: they go into their app's icon instead.
    pub show_minimized: bool,
    /// Include shell-cloaked windows from Windows/tiling-WM workspaces.
    /// Off by default to preserve the current-desktop taskbar behavior.
    pub show_other_workspaces: bool,
    /// An app's right-click menu starts with its jump list (pinned and recent
    /// items, tasks), as on the Windows taskbar. Off, the menu has only the
    /// dock's own items and no jump list is read.
    pub jump_lists: bool,
    /// macOS-style bar across the top with the clock and status items.
    pub menu_bar: bool,
    /// Keep the menu bar off screen until the pointer touches the top edge.
    pub menu_bar_auto_hide: bool,
    /// Intelligent hiding of the menu bar, as the dock's (issue #61): it
    /// shows unless the window in front reaches under it. Settings offers it
    /// and `menu_bar_auto_hide` as one choice; auto-hide wins if both are on.
    pub menu_bar_smart_hide: bool,
    /// Show supported AI providers' plan usage in the menu bar.
    pub ai_usage: bool,
    /// Show what is playing (title, controls) in the menu bar. With this and
    /// `ai_usage` off there is no island at all (issue #28).
    pub menu_bar_music: bool,
    /// Visual style and placement of the music and AI usage island in the menu bar.
    pub menu_bar_island_style: IslandStyle,
    /// A bell at the right end of the menu bar that opens the Windows
    /// notification center (issue #55). Off by default, so the bar looks as
    /// it always did; the clock opens notifications either way unless
    /// `menu_bar_clock_target` says otherwise.
    pub menu_bar_notifications: bool,
    /// What clicking the clock opens: an app, a shortcut, a document or a URI
    /// (e.g. the Clock app or a calendar). `None` keeps the notification
    /// center and calendar, as on the Windows taskbar.
    pub menu_bar_clock_target: Option<String>,
    /// CPU, memory and network speed in the menu bar (issue #64). Off by
    /// default: off, nothing is sampled; on, every 2 s only while the bar
    /// is on screen.
    pub menu_bar_system_monitor: bool,
    /// Opt-in peripheral monitoring. Off means no accessory reader or queries.
    pub accessory_battery: bool,
    /// The dot under apps that have windows open.
    pub show_indicators: bool,
    /// The section of recently used apps that are not pinned.
    pub show_recents: bool,
    /// Icons bounce while the app they open is starting.
    pub bounce_on_open: bool,
    /// The mascot comes out for moments worth it (an AI plan resetting or
    /// running out, the charger, energy saver).
    pub mascot: bool,
    /// Which one: Claude's crab by default, the cat, the fox, the capybara,
    /// the mochi or the girl.
    pub mascot_character: MascotCharacter,
    /// The mascot also suggests a break after 90 minutes of work without a
    /// pause. Off by default: a reminder is only welcome when asked for.
    pub mascot_break_reminder: bool,
    /// Settings asks GitHub Releases for a newer version when it opens. The
    /// only network access in Nosih, so it can be turned off.
    pub check_updates: bool,
    /// With `check_updates`: once a day, while the user is away from the
    /// computer, new versions are downloaded and installed without asking.
    pub auto_update: bool,
    pub language: Language,
    /// The look of the dock, the menu bar and their menus and panels.
    /// `Auto` (the default) follows Windows' light or dark mode.
    pub appearance: Appearance,
    /// A theme of its own for the menu bar and its menus and panels (issue
    /// #54). `None`, the default, gives it the dock's `appearance`.
    pub menu_bar_appearance: Option<Appearance>,
    /// A folder shown at the right of the dock whose contents open as a
    /// menu, with a submenu for each folder inside, like a Windows 10 taskbar
    /// toolbar. Only one: the user arranges the folder as they like.
    pub shortcuts_folder: Option<String>,
    // Retired switches, whose saved values are ignored: the attention
    // indicator follows Windows' own "Show badges on taskbar apps"; icons
    // always bounce for attention; window previews always have a close
    // button.
}

/// Pinned apps the user gathered in one dock icon by dropping one on
/// another. Its `id` stands for it in `pinned`, in its place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct DockGroup {
    pub id: String,
    pub apps: Vec<String>,
}

/// Speed of the genie. Three steps, not a slider: a duration in milliseconds
/// would ask the user to guess, and these three cover what anyone asked for.
/// The screen edge the dock sits on: the three macOS offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockPosition {
    Left,
    Right,
    /// Also what an unknown value from a newer version reads as (serde
    /// needs this fallback last).
    #[default]
    #[serde(other)]
    Bottom,
}

/// Which mascot comes out for the moments worth it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MascotCharacter {
    Cat,
    Fox,
    Capybara,
    Mochi,
    Girl,
    /// Claude's crab. Also what an unknown value from a newer version reads
    /// as (serde needs the fallback variant last).
    #[default]
    #[serde(other)]
    Crab,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenieSpeed {
    Faster,
    Normal,
    /// Also what an unknown value from a newer version reads as (serde needs
    /// the fallback variant last).
    #[default]
    #[serde(other)]
    Fast,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockDensity {
    Compact,
    Spacious,
    #[default]
    #[serde(other)]
    Normal,
}

impl DockDensity {
    /// Icon spacing and inner padding, in logical pixels, independent of icon size.
    #[must_use]
    pub const fn spacing(self) -> (f32, f32) {
        match self {
            Self::Compact => (8.0, 6.0),
            Self::Normal => (12.0, 9.0),
            Self::Spacious => (16.0, 12.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockAutoHideSpeed {
    Smooth,
    Fast,
    #[default]
    #[serde(other)]
    Normal,
}

impl DockAutoHideSpeed {
    /// Frequency of the existing critically damped slide spring; dwell is unchanged.
    #[must_use]
    pub const fn omega(self) -> f32 {
        match self {
            Self::Smooth => 16.0,
            Self::Normal => 24.0,
            Self::Fast => 36.0,
        }
    }
}

impl GenieSpeed {
    /// How long one genie flight lasts, minimizing or restoring.
    #[must_use]
    pub fn seconds(self) -> f32 {
        match self {
            Self::Normal => 0.5,
            Self::Fast => 0.35,
            Self::Faster => 0.25,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandStyle {
    /// Compact rounded bubble placed next to the right-side status indicators.
    Bubble,
    /// Centered curved notch hanging from the top of the menu bar.
    #[default]
    #[serde(other)]
    Notch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Es,
    En,
    Fr,
    Zh,
    Hi,
    Ar,
    Bn,
    Pt,
    /// Follow the Windows display language. Also what a language added by a
    /// newer version reads as, so the file stays valid for older versions
    /// (serde requires this fallback to be the last variant).
    #[default]
    #[serde(other)]
    Auto,
}

/// The themes Settings offers, as macOS offers its appearances. Every glass
/// theme falls back to a solid one while Windows has transparency off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
    /// Diffuse glass following Windows' light or dark palette.
    Frosted,
    /// Barely tinted glass with a bright rim. Light content.
    Clear,
    /// Neutral lens-like glass. Older Aurora preferences migrate on read.
    #[serde(rename = "liquidGlass", alias = "aurora")]
    LiquidGlass,
    /// Follow Windows' light or dark mode. Also what a theme added by a newer
    /// version reads as (serde needs this fallback last).
    #[default]
    #[serde(other)]
    Auto,
}

pub const ICON_SIZE_RANGE: (u32, u32) = (32, 96);
pub const MAX_MAGNIFIED_SIZE: u32 = 160;

impl Config {
    /// Either policy needs a reveal strip and leaves maximized windows the full work area.
    pub fn hides_automatically(&self) -> bool {
        self.auto_hide || self.smart_hide
    }

    /// Brings out-of-range values (a hand-edited file) back into range.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.icon_size = self.icon_size.clamp(ICON_SIZE_RANGE.0, ICON_SIZE_RANGE.1);
        self.magnified_size = self
            .magnified_size
            .clamp(self.icon_size, MAX_MAGNIFIED_SIZE);
        self.shortcuts_folder = self
            .shortcuts_folder
            .map(|folder| folder.trim().to_owned())
            .filter(|folder| !folder.is_empty());
        self.menu_bar_clock_target = self
            .menu_bar_clock_target
            .map(|target| target.trim().to_owned())
            .filter(|target| !target.is_empty());
        self
    }

    /// Scale of the icon under the cursor (1.0 = no magnification).
    #[must_use]
    pub fn max_scale(&self) -> f32 {
        if self.magnification {
            self.magnified_size as f32 / self.icon_size as f32
        } else {
            1.0
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            pinned: Vec::new(),
            groups: Vec::new(),
            recent: Vec::new(),
            taskbar_pins: None,
            auto_hide: false,
            smart_hide: true,
            icon_size: 48,
            magnification: true,
            dock_position: DockPosition::Bottom,
            dock_density: DockDensity::Normal,
            dock_auto_hide_speed: DockAutoHideSpeed::Normal,
            show_start: true,
            show_trash: true,
            show_downloads: true,
            pin_explorer: true,
            magnified_size: 77,
            genie: true,
            genie_speed: GenieSpeed::Fast,
            show_minimized: false,
            show_other_workspaces: false,
            jump_lists: true,
            menu_bar: true,
            menu_bar_auto_hide: false,
            menu_bar_smart_hide: false,
            ai_usage: true,
            menu_bar_music: true,
            menu_bar_island_style: IslandStyle::Notch,
            menu_bar_notifications: false,
            menu_bar_system_monitor: false,
            menu_bar_clock_target: None,
            accessory_battery: false,
            show_indicators: true,
            show_recents: true,
            bounce_on_open: true,
            mascot: true,
            mascot_character: MascotCharacter::Crab,
            mascot_break_reminder: false,
            check_updates: true,
            auto_update: true,
            language: Language::Auto,
            appearance: Appearance::Auto,
            menu_bar_appearance: None,
            shortcuts_folder: None,
        }
    }
}

/// Moves the settings folder from before the app was renamed to Nosih.
pub fn migrate_legacy_folder() {
    let roaming = PathBuf::from(env::var_os("APPDATA").unwrap_or_default());
    let (old, new) = (roaming.join("WinDock"), roaming.join("Nosih"));
    if old.is_dir() && !new.exists() {
        let _ = fs::rename(old, new);
    }
}

#[must_use]
pub fn default_path() -> PathBuf {
    PathBuf::from(env::var_os("APPDATA").unwrap_or_default())
        .join("Nosih")
        .join("config.json")
}

/// Reads the configuration. Returns `Ok(None)` if the file does not exist yet.
pub fn load(path: &Path) -> Result<Option<Config>, ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_owned(),
                source,
            });
        }
    };
    serde_json::from_str(&text)
        .map(|config: Config| Some(config.sanitized()))
        .map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
}

/// Replaces the configuration under the same lock used by incremental edits.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let _lock = lock_for_write(path)?;
    write_config(path, config)
}

/// Reads, changes and publishes one version while other writers wait. Readers
/// can still load the last complete version without acquiring the lock.
pub fn update(
    path: &Path,
    edit: impl FnOnce(Option<Config>) -> Result<Config, ConfigError>,
) -> Result<Config, ConfigError> {
    let _lock = lock_for_write(path)?;
    let config = edit(load(path)?)?.sanitized();
    write_config(path, &config)?;
    Ok(config)
}

/// Updates only preferences explicitly changed in Settings. The roster and
/// schema version belong to the dock and may have changed since Settings opened.
pub fn update_preferences(
    path: &Path,
    patch: serde_json::Map<String, serde_json::Value>,
) -> Result<Config, ConfigError> {
    update(path, |saved| {
        let mut fields = serde_json::to_value(saved.unwrap_or_default())?;
        if let Some(fields) = fields.as_object_mut() {
            for (key, value) in patch {
                if fields.contains_key(&key)
                    && !matches!(
                        key.as_str(),
                        "version" | "pinned" | "groups" | "recent" | "taskbarPins"
                    )
                {
                    fields.insert(key, value);
                }
            }
        }
        serde_json::from_value(fields).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    })
}

fn lock_for_write(path: &Path) -> Result<File, ConfigError> {
    let io_error = |source| ConfigError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(io_error)?;
    }
    // Lock a separate file so the atomic rename cannot replace the lock.
    // Closing the handle releases it, including when a process exits.
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("json.lock"))
        .map_err(io_error)?;
    let start = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::WouldBlock) if start.elapsed() < Duration::from_millis(80) => {
                thread::sleep(Duration::from_millis(4));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(io_error(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Ajustes y el dock están guardando la configuración",
                )));
            }
            Err(TryLockError::Error(error)) => return Err(io_error(error)),
        }
    }
}

fn write_config(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let io_error = |source| ConfigError::Io {
        path: path.to_owned(),
        source,
    };
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_string_pretty(config)?).map_err(io_error)?;
    fs::rename(&temporary, path).map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_bluetooth_visibility_does_not_override_system_status()
    -> Result<(), serde_json::Error> {
        for old in [false, true] {
            let config: Config = serde_json::from_value(
                serde_json::json!({"menuBarBluetooth": old, "showStart": false}),
            )?;
            assert!(!config.show_start);
            assert!(
                serde_json::to_value(config)?
                    .get("menuBarBluetooth")
                    .is_none()
            );
        }
        Ok(())
    }

    #[test]
    fn other_workspaces_are_opt_in_and_preserved_when_saving() -> Result<(), serde_json::Error> {
        let legacy: Config = serde_json::from_str("{}")?;
        assert!(!legacy.show_other_workspaces);
        let enabled: Config = serde_json::from_str(r#"{"showOtherWorkspaces":true}"#)?;
        let restored: Config = serde_json::from_str(&serde_json::to_string(&enabled)?)?;
        assert!(restored.show_other_workspaces);
        Ok(())
    }

    #[test]
    fn new_installs_use_intelligent_hiding_and_existing_modes_are_preserved()
    -> Result<(), serde_json::Error> {
        let fresh = Config::default();
        assert!(!fresh.auto_hide && fresh.smart_hide);
        let round_trip: Config = serde_json::from_str(&serde_json::to_string(&fresh)?)?;
        assert_eq!(round_trip, fresh);
        for (json, auto, smart) in [
            (r#"{}"#, false, false),
            (r#"{"autoHide":false}"#, false, false),
            (r#"{"autoHide":true}"#, true, false),
            (r#"{"auto_hide":true}"#, true, false),
            (r#"{"autoHide":false,"smartHide":false}"#, false, false),
            (r#"{"autoHide":false,"smartHide":true}"#, false, true),
            (r#"{"autoHide":true,"smartHide":true}"#, true, true),
        ] {
            let existing: Config = serde_json::from_str(json)?;
            assert_eq!(
                (existing.auto_hide, existing.smart_hide),
                (auto, smart),
                "{json}"
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_visible_mode_and_retired_banner_preferences_are_preserved()
    -> Result<(), serde_json::Error> {
        let old: Config = serde_json::from_value(serde_json::json!({
            "notifications": true, "notificationPosition": "top-right",
            "notificationDuration": 5, "notificationDnd": true,
            "notificationBadges": false, "autoHide": false,
            "notificationBounce": false, "showPreviewCloseButton": false
        }))?;
        assert!(!old.smart_hide && !old.hides_automatically());
        let saved = serde_json::to_value(&old)?;
        assert!(saved.get("notifications").is_none());
        assert!(saved.get("notificationBadges").is_none());
        assert!(saved.get("notificationBounce").is_none());
        assert!(saved.get("showPreviewCloseButton").is_none());
        assert_eq!(saved.get("smartHide"), Some(&serde_json::json!(false)));
        Ok(())
    }

    #[test]
    fn explicitly_enabled_intelligent_hiding_survives_round_trip() -> Result<(), serde_json::Error>
    {
        let enabled: Config = serde_json::from_str(r#"{"smartHide":true,"autoHide":false}"#)?;
        let restored: Config = serde_json::from_str(&serde_json::to_string(&enabled)?)?;
        assert!(restored.smart_hide && restored.hides_automatically());
        Ok(())
    }

    #[test]
    fn older_configs_keep_the_original_dock_defaults() -> Result<(), serde_json::Error> {
        let config: Config = serde_json::from_str(r#"{"iconSize":64}"#)?;
        assert!(
            config.show_start && config.show_trash && config.show_downloads && config.pin_explorer
        );
        assert_eq!(config.dock_density.spacing(), (12.0, 9.0));
        assert_eq!(config.dock_auto_hide_speed.omega(), 24.0);
        Ok(())
    }

    #[test]
    fn dock_preferences_round_trip_and_unknown_presets_fall_back() -> Result<(), serde_json::Error>
    {
        let config: Config = serde_json::from_str(
            r#"{
            "showStart":false,"showTrash":false,"showDownloads":false,"pinExplorer":false,
            "dockDensity":"compact","dockAutoHideSpeed":"fast"
        }"#,
        )?;
        assert!(
            !config.show_start
                && !config.show_trash
                && !config.show_downloads
                && !config.pin_explorer
        );
        assert_eq!(config.dock_density.spacing(), (8.0, 6.0));
        assert_eq!(config.dock_auto_hide_speed.omega(), 36.0);
        assert_eq!(
            serde_json::from_str::<Config>(&serde_json::to_string(&config)?)?,
            config
        );
        let unknown: Config =
            serde_json::from_str(r#"{"dockDensity":"future","dockAutoHideSpeed":"future"}"#)?;
        assert_eq!(unknown.dock_density, DockDensity::Normal);
        assert_eq!(unknown.dock_auto_hide_speed, DockAutoHideSpeed::Normal);
        Ok(())
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("nosih-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder.join("config.json")
    }

    #[test]
    fn load_should_return_none_when_file_is_missing() {
        assert_eq!(load(&scratch("missing")).ok(), Some(None));
    }

    #[test]
    fn save_then_load_should_round_trip() {
        let path = scratch("round-trip");
        let config = Config {
            pinned: vec!["explorer".into()],
            ..Config::default()
        };
        save(&path, &config).ok();
        assert_eq!(load(&path).ok(), Some(Some(config)));
    }

    #[test]
    fn settings_patch_keeps_dock_edits_made_since_opening() -> Result<(), ConfigError> {
        let path = scratch("settings-patch");
        let initial = Config::default();
        save(&path, &initial)?;
        update(&path, |saved| {
            let mut config = saved.unwrap_or_default();
            config.auto_hide = true;
            config.pinned.push("browser".into());
            Ok(config)
        })?;
        let patch = serde_json::Map::from_iter([
            ("iconSize".into(), 64.into()),
            ("pinned".into(), serde_json::Value::Array(Vec::new())),
        ]);
        let saved = update_preferences(&path, patch)?;
        assert!(saved.auto_hide);
        assert_eq!(saved.pinned, ["browser"]);
        assert_eq!(saved.icon_size, 64);
        Ok(())
    }

    #[test]
    fn accessory_monitoring_is_opt_in_and_its_preference_round_trips() -> Result<(), ConfigError> {
        let path = scratch("accessory-monitoring");
        assert!(!Config::default().accessory_battery);
        for enabled in [true, false] {
            let saved = update_preferences(
                &path,
                serde_json::Map::from_iter([("accessoryBattery".into(), enabled.into())]),
            )?;
            assert_eq!(saved.accessory_battery, enabled);
            assert_eq!(
                load(&path)?.map(|config| config.accessory_battery),
                Some(enabled)
            );
        }
        Ok(())
    }

    #[test]
    fn retired_status_overrides_are_ignored_without_changing_other_preferences()
    -> Result<(), ConfigError> {
        let path = scratch("retired-status-overrides");
        save(&path, &Config::default())?;
        for old in [serde_json::Value::Null, false.into(), true.into()] {
            let mut patch = serde_json::Map::from_iter([
                ("menuBarSearch".into(), old.clone()),
                ("menuBarTaskView".into(), old.clone()),
                ("menuBarWidgets".into(), old.clone()),
                ("menuBarBatteryPercent".into(), old),
                ("showStart".into(), false.into()),
                ("accessoryBattery".into(), true.into()),
            ]);
            // Exercise loading an existing file, then saving a Settings patch.
            fs::write(&path, serde_json::to_vec(&patch)?).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
            let loaded = load(&path)?.unwrap_or_default();
            assert!(!loaded.show_start && loaded.accessory_battery);
            patch.insert("iconSize".into(), 64.into());
            let saved = update_preferences(&path, patch)?;
            assert!(!saved.show_start && saved.accessory_battery);
            assert_eq!(saved.icon_size, 64);
            let json = serde_json::to_value(load(&path)?.unwrap_or_default())?;
            for key in [
                "menuBarSearch",
                "menuBarTaskView",
                "menuBarWidgets",
                "menuBarBatteryPercent",
            ] {
                assert!(json.get(key).is_none(), "retired override {key}");
            }
        }
        Ok(())
    }

    #[test]
    fn settings_patch_keeps_the_dock_groups() -> Result<(), ConfigError> {
        let path = scratch("settings-patch-groups");
        let group = DockGroup {
            id: "nosih:group:1".into(),
            apps: vec!["a".into(), "b".into()],
        };
        save(
            &path,
            &Config {
                pinned: vec![group.id.clone()],
                groups: vec![group.clone()],
                ..Config::default()
            },
        )?;
        let patch = serde_json::Map::from_iter([
            ("iconSize".into(), 64.into()),
            ("groups".into(), serde_json::Value::Array(Vec::new())),
        ]);
        let saved = update_preferences(&path, patch)?;
        assert_eq!(saved.groups, [group]);
        assert_eq!(saved.icon_size, 64);
        Ok(())
    }

    #[test]
    fn a_busy_writer_keeps_the_last_complete_configuration() -> Result<(), ConfigError> {
        let path = scratch("write-contention");
        let original = Config::default();
        save(&path, &original)?;
        let _writer = lock_for_write(&path)?;
        let result = update(&path, |_| panic!("contending writer cannot edit"));
        assert!(
            matches!(result, Err(ConfigError::Io { source, .. }) if source.kind() == io::ErrorKind::WouldBlock)
        );
        assert_eq!(load(&path)?, Some(original));
        Ok(())
    }

    #[test]
    fn load_fills_missing_fields_and_preserves_legacy_visibility() {
        let path = scratch("partial");
        save(&path, &Config::default()).ok();
        fs::write(&path, "{}").ok();
        assert_eq!(
            load(&path).ok(),
            Some(Some(Config {
                smart_hide: false,
                ..Config::default()
            }))
        );
    }

    #[test]
    fn load_should_give_older_files_the_new_behavior_defaults() {
        let path = scratch("before-behavior");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"version":1,"iconSize":56}"#).ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert_eq!(config.icon_size, 56);
        assert!(config.show_indicators && config.show_recents && config.bounce_on_open);
        assert!(config.check_updates);
        // Files from before automatic updates get them, like new installs.
        assert!(config.auto_update);
        assert!(config.ai_usage);
        assert!(config.menu_bar_music);
        assert!(!config.accessory_battery);
        assert!(!config.show_minimized);
        assert!(config.jump_lists);
    }

    #[test]
    fn load_should_read_dock_groups() {
        let path = scratch("groups");
        save(&path, &Config::default()).ok();
        fs::write(
            &path,
            r#"{"pinned":["a","nosih:group:1"],"groups":[{"id":"nosih:group:1","apps":["b","c"]}]}"#,
        )
        .ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert_eq!(
            config.groups,
            [DockGroup {
                id: "nosih:group:1".into(),
                apps: vec!["b".into(), "c".into()],
            }]
        );
    }

    #[test]
    fn load_should_read_show_minimized() {
        let path = scratch("show-minimized");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"showMinimized":true}"#).ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert!(config.show_minimized);
    }

    #[test]
    fn load_should_read_turned_off_update_checks() {
        let path = scratch("no-update-checks");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"checkUpdates":false}"#).ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert!(!config.check_updates);
    }

    #[test]
    fn load_should_read_the_behavior_settings() {
        let path = scratch("behavior");
        save(&path, &Config::default()).ok();
        fs::write(
            &path,
            r#"{"activeAppClick":"nothing","showRecents":false,"bounceOnOpen":false}"#,
        )
        .ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert!(!config.show_recents && !config.bounce_on_open);
    }

    #[test]
    fn supported_languages_round_trip_without_changing_other_settings()
    -> Result<(), serde_json::Error> {
        for (code, language) in [
            ("auto", Language::Auto),
            ("en", Language::En),
            ("es", Language::Es),
            ("fr", Language::Fr),
            ("zh", Language::Zh),
            ("hi", Language::Hi),
            ("ar", Language::Ar),
            ("bn", Language::Bn),
            ("pt", Language::Pt),
        ] {
            let value = serde_json::json!({"language": code, "iconSize": 56});
            let config: Config = serde_json::from_value(value)?;
            assert_eq!(config.language, language);
            assert_eq!(config.icon_size, 56);
            assert_eq!(serde_json::to_value(&config)?["language"], code);
        }
        Ok(())
    }

    #[test]
    fn load_should_read_french() {
        let path = scratch("french");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"language":"fr"}"#).ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert_eq!(config.language, Language::Fr);
    }

    #[test]
    fn load_should_read_an_unknown_language_as_auto() {
        let path = scratch("unknown-language");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"language":"de","iconSize":56}"#).ok();
        let config = load(&path).ok().flatten();
        assert_eq!(
            config.map(|config| (config.language, config.icon_size)),
            Some((Language::Auto, 56))
        );
    }

    #[test]
    fn load_should_read_the_shortcuts_folder() {
        let path = scratch("shortcuts-folder");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"shortcutsFolder":"D:\\Racc"}"#).ok();
        let config = load(&path).ok().flatten().unwrap_or_default();
        assert_eq!(config.shortcuts_folder.as_deref(), Some(r"D:\Racc"));
    }

    #[test]
    fn sanitized_should_forget_a_blank_shortcuts_folder() {
        let config = Config {
            shortcuts_folder: Some("  ".into()),
            ..Config::default()
        };
        assert_eq!(config.sanitized().shortcuts_folder, None);
    }

    #[test]
    fn the_menu_bar_stays_visible_unless_another_mode_is_chosen() -> Result<(), serde_json::Error> {
        let fresh = Config::default();
        assert!(!fresh.menu_bar_auto_hide && !fresh.menu_bar_smart_hide);
        let older: Config = serde_json::from_str(r#"{"menuBarAutoHide":true}"#)?;
        assert!(older.menu_bar_auto_hide && !older.menu_bar_smart_hide);
        let chosen: Config = serde_json::from_str(r#"{"menuBarSmartHide":true}"#)?;
        assert!(chosen.menu_bar_smart_hide);
        Ok(())
    }

    #[test]
    fn the_crab_is_the_mascot_unless_the_cat_is_chosen() -> Result<(), serde_json::Error> {
        assert_eq!(Config::default().mascot_character, MascotCharacter::Crab);
        assert!(!Config::default().mascot_break_reminder);
        let older: Config = serde_json::from_str("{}")?;
        assert_eq!(older.mascot_character, MascotCharacter::Crab);
        let cat: Config = serde_json::from_str(r#"{"mascotCharacter":"cat"}"#)?;
        assert_eq!(cat.mascot_character, MascotCharacter::Cat);
        let fox: Config = serde_json::from_str(r#"{"mascotCharacter":"fox"}"#)?;
        assert_eq!(fox.mascot_character, MascotCharacter::Fox);
        let capybara: Config = serde_json::from_str(r#"{"mascotCharacter":"capybara"}"#)?;
        assert_eq!(capybara.mascot_character, MascotCharacter::Capybara);
        let mochi: Config = serde_json::from_str(r#"{"mascotCharacter":"mochi"}"#)?;
        assert_eq!(mochi.mascot_character, MascotCharacter::Mochi);
        let girl: Config = serde_json::from_str(r#"{"mascotCharacter":"girl"}"#)?;
        assert_eq!(girl.mascot_character, MascotCharacter::Girl);
        // A mascot added by a newer version falls back to the crab.
        let newer: Config = serde_json::from_str(r#"{"mascotCharacter":"owl"}"#)?;
        assert_eq!(newer.mascot_character, MascotCharacter::Crab);
        assert_eq!(serde_json::to_value(MascotCharacter::Cat)?, "cat");
        Ok(())
    }

    #[test]
    fn the_system_monitor_is_off_unless_chosen() -> Result<(), serde_json::Error> {
        assert!(!Config::default().menu_bar_system_monitor);
        let older: Config = serde_json::from_str("{}")?;
        assert!(!older.menu_bar_system_monitor);
        let chosen: Config = serde_json::from_str(r#"{"menuBarSystemMonitor":true}"#)?;
        assert!(chosen.menu_bar_system_monitor);
        Ok(())
    }

    #[test]
    fn the_clock_opens_notifications_and_the_bell_is_off_unless_chosen()
    -> Result<(), serde_json::Error> {
        let older: Config = serde_json::from_str(r#"{"iconSize":56}"#)?;
        assert!(!older.menu_bar_notifications);
        assert_eq!(older.menu_bar_clock_target, None);
        let chosen: Config = serde_json::from_str(
            r#"{"menuBarNotifications":true,"menuBarClockTarget":"C:\\Apps\\Calendar.lnk"}"#,
        )?;
        assert!(chosen.menu_bar_notifications);
        assert_eq!(
            chosen.menu_bar_clock_target.as_deref(),
            Some(r"C:\Apps\Calendar.lnk")
        );
        assert_eq!(
            serde_json::from_str::<Config>(&serde_json::to_string(&chosen)?)?,
            chosen
        );
        Ok(())
    }

    #[test]
    fn sanitized_should_forget_a_blank_clock_target() {
        let config = Config {
            menu_bar_clock_target: Some(" \t".into()),
            ..Config::default()
        };
        assert_eq!(config.sanitized().menu_bar_clock_target, None);
        let uri = Config {
            menu_bar_clock_target: Some(" outlookcal: ".into()),
            ..Config::default()
        };
        assert_eq!(
            uri.sanitized().menu_bar_clock_target.as_deref(),
            Some("outlookcal:")
        );
    }

    #[test]
    fn load_should_fail_on_invalid_json() {
        let path = scratch("invalid");
        save(&path, &Config::default()).ok();
        fs::write(&path, "esto no es json").ok();
        assert!(matches!(load(&path), Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn sanitized_should_clamp_icon_size() {
        let config = Config {
            icon_size: 500,
            ..Config::default()
        };
        assert_eq!(config.sanitized().icon_size, ICON_SIZE_RANGE.1);
    }

    #[test]
    fn sanitized_should_never_magnify_below_the_icon_size() {
        let config = Config {
            icon_size: 80,
            magnified_size: 40,
            ..Config::default()
        };
        assert_eq!(config.sanitized().magnified_size, 80);
    }

    #[test]
    fn max_scale_should_be_one_when_magnification_is_off() {
        let config = Config {
            magnification: false,
            ..Config::default()
        };
        assert_eq!(config.max_scale(), 1.0);
    }

    #[test]
    fn genie_should_be_on_by_default() {
        assert!(Config::default().genie);
        assert_eq!(Config::default().genie_speed, GenieSpeed::Fast);
    }

    #[test]
    fn the_dock_stays_at_the_bottom_unless_moved() {
        for json in [r#"{}"#, r#"{"dockPosition":"top"}"#] {
            let config: Config = serde_json::from_str(json).unwrap_or_default();
            assert_eq!(config.dock_position, DockPosition::Bottom);
        }
        let left: Config = serde_json::from_str(r#"{"dockPosition":"left"}"#).unwrap_or_default();
        assert_eq!(left.dock_position, DockPosition::Left);
    }

    #[test]
    fn the_appearance_follows_windows_unless_chosen() {
        // Files from before the setting, and themes from newer versions.
        for json in [r#"{}"#, r#"{"appearance":"sepia"}"#] {
            let config: Config = serde_json::from_str(json).unwrap_or_default();
            assert_eq!(config.appearance, Appearance::Auto);
        }
        let frosted: Config =
            serde_json::from_str(r#"{"appearance":"frosted"}"#).unwrap_or_default();
        assert_eq!(frosted.appearance, Appearance::Frosted);
    }

    #[test]
    fn the_menu_bar_shares_the_dock_theme_unless_given_its_own() -> Result<(), serde_json::Error> {
        // Files from before the setting, and an explicit "same as the dock".
        for json in [
            r#"{"appearance":"aurora"}"#,
            r#"{"menuBarAppearance":null}"#,
        ] {
            let config: Config = serde_json::from_str(json)?;
            assert_eq!(config.menu_bar_appearance, None);
        }
        let own: Config =
            serde_json::from_str(r#"{"appearance":"aurora","menuBarAppearance":"dark"}"#)?;
        assert_eq!(
            (own.appearance, own.menu_bar_appearance),
            (Appearance::LiquidGlass, Some(Appearance::Dark))
        );
        assert_eq!(
            serde_json::from_str::<Config>(&serde_json::to_string(&own)?)?,
            own
        );
        // A theme from a newer version reads as automatic, as for the dock.
        let newer: Config = serde_json::from_str(r#"{"menuBarAppearance":"sepia"}"#)?;
        assert_eq!(newer.menu_bar_appearance, Some(Appearance::Auto));
        Ok(())
    }

    #[test]
    fn aurora_migrates_to_liquid_glass_for_both_surfaces() -> Result<(), serde_json::Error> {
        let old: Config = serde_json::from_str(
            r#"{"appearance":"aurora","menuBarAppearance":"aurora","iconSize":64}"#,
        )?;
        assert_eq!(old.appearance, Appearance::LiquidGlass);
        assert_eq!(old.menu_bar_appearance, Some(Appearance::LiquidGlass));
        assert_eq!(old.icon_size, 64);
        let saved = serde_json::to_value(&old)?;
        assert_eq!(saved["appearance"], "liquidGlass");
        assert_eq!(saved["menuBarAppearance"], "liquidGlass");
        assert_eq!(serde_json::from_value::<Config>(saved)?, old);
        Ok(())
    }

    #[test]
    fn genie_speed_defaults_to_fast_and_preserves_saved_choices() {
        let missing: Config = serde_json::from_str(r#"{}"#).unwrap_or_default();
        assert_eq!(missing.genie_speed, GenieSpeed::Fast);

        let unknown: Config = serde_json::from_str(r#"{"genieSpeed":"warp"}"#).unwrap_or_default();
        assert_eq!(unknown.genie_speed, GenieSpeed::Fast);

        let normal: Config = serde_json::from_str(r#"{"genieSpeed":"normal"}"#).unwrap_or_default();
        assert_eq!(normal.genie_speed, GenieSpeed::Normal);

        let fast: Config = serde_json::from_str(r#"{"genieSpeed":"fast"}"#).unwrap_or_default();
        assert_eq!(fast.genie_speed, GenieSpeed::Fast);

        let faster: Config = serde_json::from_str(r#"{"genieSpeed":"faster"}"#).unwrap_or_default();
        assert_eq!(faster.genie_speed, GenieSpeed::Faster);
        assert!(GenieSpeed::Faster.seconds() < GenieSpeed::Fast.seconds());
        assert!(GenieSpeed::Fast.seconds() < GenieSpeed::Normal.seconds());
    }

    #[test]
    fn load_should_read_files_written_before_camel_case_keys() {
        let path = scratch("legacy-keys");
        save(&path, &Config::default()).ok();
        fs::write(&path, r#"{"version":1,"pinned":["a"],"auto_hide":true}"#).ok();
        let loaded = load(&path).ok().flatten();
        assert_eq!(loaded.map(|config| config.auto_hide), Some(true));
    }
}
