//! Dock coordinator: shared UI state and stable entry points.
//!
//! Domain logic lives in the private modules below; they borrow this same App
//! on its UI thread. Native entry points are re-exported to preserve app::*.
//! Keep the App borrow here, and execute deferred Outside actions only after
//! releasing it. New domain behavior belongs in its module, not in this facade.
//!
//! There are two windows:
//! - the **visual** window draws everything and passes through clicks; it is
//!   larger than the dock to fit the shadow, magnified icons, and labels;
//! - the invisible **input** window covers only the area where the dock receives
//!   mouse input. It expands while the cursor is over the dock (hysteresis).
//!
//! While the dock is running, the Windows taskbar is hidden and the dock is
//! registered as an AppBar, reserving a strip along its configured screen edge.

mod activation;
mod assets;
mod drag;
mod events;
mod feedback;
mod groups;
mod input;
mod lifecycle;
mod menus;
mod preferences;
mod previews;
mod rendering;
mod visibility;

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicIsize, AtomicU8, Ordering},
    },
    thread,
    time::Instant,
};

use anyhow::Result;
use nosih_config::{Config, DockDensity};
use nosih_core::{
    attention::Attention,
    frame::{DockEdge, Frame as DockFrame},
    layout::{ItemKind, Layout, Rect},
    magnify::Magnifier,
    roster::Roster,
    spring::Spring,
};
use nosih_shell::{
    appbar::AppBar,
    end_task::PreparedEndTask,
    jumplist, power,
    recycle_bin::BinWatch,
    tracker::{TrackedWindow, WindowTracker},
};
use windows::{
    UI::Composition::{CompositionSurfaceBrush, Desktop::DesktopWindowTarget},
    Win32::{
        Foundation::{HWND, POINT, RECT},
        System::Ole::{
            DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
        },
        UI::WindowsAndMessaging::{
            HWND_TOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, WM_APP,
        },
    },
    core::PCWSTR,
};

use crate::{
    assets::Visuals,
    edge_watch::EdgeWatch,
    flight::{Flight, Snapshot},
    gfx::{Gfx, LabelBrush},
    group_view::{DragGhost, GroupView},
    hud::Hud,
    mascot,
    menu::{self, MenuItem},
    menubar::MenuBar,
    multi_window_view::MultiWindowView,
    scene::Scene,
    spotlight::Spotlight,
    thumbnails::Thumbnails,
};

use menus::run_outside;

pub use events::{
    drag_enter, drag_leave, drag_over, drop_files, group_view_menu, group_view_moved,
    group_view_press, group_view_press_lost, group_view_release, group_view_tick, hud_timeout,
    menu_bar_accessories_changed, menu_bar_accessories_reset, menu_bar_audio_changed,
    menu_bar_bluetooth_changed, menu_bar_brightness_flush, menu_bar_check_pointer,
    menu_bar_clicked, menu_bar_control_center_changed, menu_bar_dismiss_island,
    menu_bar_finish_hide, menu_bar_media_changed, menu_bar_monitor_tick,
    menu_bar_notification_banner_timeout, menu_bar_notifications_changed, menu_bar_pointer_moved,
    menu_bar_progress_tick, menu_bar_registry_changed, menu_bar_scrolled, menu_bar_tick,
    menu_bar_volume_flush, menu_bar_volume_press, menu_bar_volume_release,
    multi_window_view_dismiss, multi_window_view_middle_click, multi_window_view_press,
    multi_window_view_release, multi_window_view_scroll, multi_window_view_tick, open_spotlight,
    show_hud_brightness, show_hud_volume, spotlight_char, spotlight_click, spotlight_deactivate,
    spotlight_key_down,
};
pub(crate) use input::primary_button_down;
pub use lifecycle::{request_quit, run};

/// These constants are not named in the `windows` crate.
const WM_MOUSELEAVE: u32 = 0x02A3;
const HSHELL_WINDOWACTIVATED: usize = 4;
const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
const HSHELL_FLASH: usize = 0x8006;
const HSHELL_WINDOWDESTROYED: usize = 2;
/// A window's title or icon changed.
const HSHELL_REDRAW: usize = 6;
/// The keyboard language changed.
const HSHELL_LANGUAGE: usize = 8;
/// `wParam` of WM_SETTINGCHANGE when the work area changes. The dock's own
/// reservation causes this, so handling it would create a loop.
const SPI_SETWORKAREA: usize = 0x002F;
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
/// Sent when the charger is plugged in or out, and when energy saver turns
/// on or off.
const PBT_APMPOWERSTATUSCHANGE: usize = 0x000A;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;

const WINDOW_CLASS: PCWSTR = nosih_shell::dock_link::DOCK_WINDOW_CLASS;
pub(super) const SPOTLIGHT_HOTKEY_ID: i32 = 1;
const APPBAR_CALLBACK: u32 = WM_APP + 1;
const TASKBAR_TAKEN: u32 = WM_APP + 2;
const BIN_CHANGED: u32 = WM_APP + 3;
/// Result of checking the recycle bin in the background (`wParam` = 1 if non-empty).
const BIN_STATE: u32 = WM_APP + 4;
/// Posted (not sent) when mouse capture is lost, so it is handled outside the
/// call that released it.
const DRAG_CANCELLED: u32 = WM_APP + 5;
/// Runs a file drop after OLE's `Drop` returns, so the drag source is not
/// blocked while files are copied or an app starts.
const DROP_RUN: u32 = WM_APP + 6;
/// Posted when apps are pinned to or unpinned from the Windows taskbar.
const PINS_CHANGED: u32 = WM_APP + 7;
const ASSETS_READY: u32 = WM_APP + 8;
/// The guard also checks graphics health while the scene is otherwise idle.
pub(crate) const HEALTH_CHECK: u32 = WM_APP + 9;
const TASKBAR_PREFS_CHANGED: u32 = WM_APP + 10;

/// Magnification range, in icons on either side of the cursor.
const RANGE_IN_ICONS: f32 = 3.0;
/// Speed of magnification entry and exit (about 200 ms).
const HOVER_OMEGA: f32 = 28.0;
/// Bounce height as a fraction of the icon size.
const BOUNCE_HEIGHT: f32 = 0.5;
const LAUNCH_BOUNCE_LIMIT: f32 = 10.0;
const ATTENTION_HOPS: f32 = 3.0;
/// Opening an app that already runs without windows (in the notification
/// area): it may show nothing (issue #59), so a short acknowledgement
/// instead of bouncing until the limit.
const RUNNING_LAUNCH_HOPS: f32 = 2.0;

// Dimensions are in logical pixels and are multiplied by the display scale.
/// Gap between the dock and the bottom screen edge, and between the dock and
/// maximized windows.
const FLOAT_MARGIN: f32 = 8.0;
const SHADOW_ROOM: f32 = 44.0;
const LABEL_ROOM: f32 = 60.0;
/// Widest room kept beside a side dock for an app's name, in logical pixels.
const SIDE_LABEL_MAX: f32 = 320.0;
/// Hysteresis distances: the effect starts at the first distance and ends at the second.
const ENTER_HALO: f32 = 5.0;
const LEAVE_HALO: f32 = 15.0;

/// Windows sends settings changes in bursts; coalesce them into one recomposition.
const REFRESH_TIMER: usize = 1;
const REFRESH_DELAY_MS: u32 = 120;
/// Window notifications also arrive in bursts when an app opens.
const SYNC_TIMER: usize = 2;
const SYNC_DELAY_MS: u32 = 40;
/// Minimized windows whose snapshot is kept to fly them back out of the dock.
/// Each costs a quarter of the window's pixels; older ones restore without
/// the genie.
const MAX_SNAPSHOTS: usize = 4;
/// After Explorer restarts or the system resumes, the new taskbar needs time
/// to apply its initial state before it can be taken over again.
const TASKBAR_TIMER: usize = 3;
const TASKBAR_DELAY_MS: u32 = 600;
const TASKBAR_MAX_ATTEMPTS: u8 = 50;
const TRIM_TIMER: usize = 4;
const TRIM_DELAY_MS: u32 = 3000;
const BIN_TIMER: usize = 5;
const BIN_DELAY_MS: u32 = 300;
const HIDE_TIMER: usize = 6;
const HIDE_DELAY_MS: u32 = 300;
/// Explorer writes a pin change in a few steps; read it once they settle.
const PINS_TIMER: usize = 7;
const PINS_DELAY_MS: u32 = 400;
/// Holding a drag over an open app brings its window to the front after this
/// long, so the drop can land in it: the Windows taskbar's drag-hover, and
/// macOS's spring-loaded Dock icons. Passing over icons does not.
const SPRING_TIMER: usize = 8;
const SPRING_DELAY_MS: u32 = 600;
/// While the Alt+Tab switcher is open, checks how long it stays: Windows
/// reports when it comes to the front, but not reliably when it closes.
/// A duration as timer milliseconds; Windows caps timers at about 24 days.
fn millis(duration: std::time::Duration) -> u32 {
    duration.as_millis().min(0x7FFF_FFFF) as u32
}

/// The background update runs once a day: a single timer, aimed at the
/// moment the next look is due (see `schedule_update`).
const UPDATE_TIMER: usize = 12;
const PREVIEW_TIMER: usize = 13;
/// While the crab is out (a few seconds): is its act over, is the pointer on it.
const MASCOT_TIMER: usize = 14;
const MASCOT_POLL_MS: u32 = 50;
const EDGE_TIMER: usize = 15;
/// The charger act at most this often, however many times it is plugged.
const CHARGING_AGAIN: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const PREVIEW_DWELL_MS: u32 = 420;
const UPDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// After the dock starts, the look waits this long, so it never slows signing in.
const UPDATE_AFTER_START: std::time::Duration = std::time::Duration::from_secs(2 * 60);
/// While a full-screen app (a game, a presentation) is in front, try later.
const UPDATE_RETRY: std::time::Duration = std::time::Duration::from_secs(60 * 60);
const SWITCHER_TIMER: usize = 9;
/// Short enough to see every Tab press (a key stays down 50-100 ms).
const SWITCHER_POLL_MS: u32 = 16;
/// How long the switcher may take to show after coming to the front.
const SWITCHER_APPEAR: std::time::Duration = std::time::Duration::from_millis(400);
/// How soon after closing the switcher's window coming back means nothing.
const SWITCHER_ECHO: std::time::Duration = std::time::Duration::from_millis(150);
/// Recheck full-screen state while hidden in case Windows drops the exit event.
const FULLSCREEN_TIMER: usize = 10;
const FULLSCREEN_RECHECK_MS: u32 = 3_000;
/// Shell results arriving together cause one scene rebuild.
const ASSETS_TIMER: usize = 11;
const ASSETS_DELAY_MS: u32 = 16;
/// Non-activating windows cannot rely on capture outside their bounds.
/// Armed only while dragging or holding a draggable icon.
const DRAG_TIMER: usize = 16;
/// The first moment of a run, a little after start, a wake-up or an unlock
/// (see `moments::welcome`): the dock has settled by then.
const WELCOME_TIMER: usize = 17;
const WELCOME_DELAY_MS: u32 = 5_000;
/// Tries again while the mascot is busy or something runs full screen.
const WELCOME_RETRY_MS: u32 = 30_000;
const WELCOME_TRIES: u8 = 10;
/// The break reminder's minute, only while it is on.
const BREAK_TIMER: usize = 18;
const BREAK_CHECK_MS: u32 = 60_000;
const DRAG_POLL_MS: u32 = 16;

/// Screen-edge strip that reveals a hidden dock, in physical pixels.
const REVEAL_STRIP: i32 = 2;
/// An icon released this far above the magnified dock, in icon heights, leaves it.
const REMOVE_DISTANCE: f32 = 0.8;
const REORDER_DURATION: std::time::Duration = std::time::Duration::from_millis(140);
/// A dragged app joins the icon under it (in a group) when released over
/// its middle, past this fraction of its width from either side; over its
/// sides, it moves between icons as usual.
const MERGE_SIDE: f32 = 0.25;

/// Longest hover label of a group, which lists its apps.
const GROUP_LABEL_CHARS: usize = 48;

const START_ID: &str = "nosih:start";
const WINDOW_ID_PREFIX: &str = "nosih:window:";
const WINDOW_TITLE_CHARS: usize = 40;
/// Fraction of a thumbnail slot covered by the app badge (see `scene`).
const BADGE_CORNER: f32 = crate::scene::BADGE_SIZE * crate::scene::BADGE_INSIDE;
const CMD_CLOSE_WINDOW: u32 = 8;
const CMD_SETTINGS: u32 = 9;

/// Context menu commands. Window commands start at `CMD_WINDOW`.
const CMD_OPEN: u32 = 1;
const CMD_TOGGLE_PIN: u32 = 2;
const CMD_REVEAL: u32 = 3;
const CMD_QUIT_APP: u32 = 4;
const CMD_EMPTY_BIN: u32 = 5;
const CMD_QUIT_DOCK: u32 = 6;
const CMD_TOGGLE_HIDING: u32 = 7;
const CMD_RUN_AS_ADMIN: u32 = 11;
const CMD_SHOW_DESKTOP: u32 = 12;
const CMD_TASK_MANAGER: u32 = 13;
const CMD_TASKBAR_SETTINGS: u32 = 14;
const CMD_UNGROUP: u32 = 15;
const CMD_REMOVE_FOLDER: u32 = 16;
const CMD_FORGET_RECENT: u32 = 17;
const CMD_END_TASK: u32 = 18;
const CMD_HIDE_FIXED: u32 = 19;
const CMD_WINDOW: u32 = 100;
/// Jump list entries start here, after any window command.
const CMD_JUMP: u32 = 1000;
const MENU_WINDOWS: usize = 12;
const MENU_TITLE_CHARS: usize = 60;

static SHELL_HOOK: OnceLock<u32> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
/// Broadcast by the settings app after it saves the configuration.
static CONFIG_CHANGED: OnceLock<u32> = OnceLock::new();
/// Dock visual window, used to close it from the Ctrl+C handler running on
/// another thread.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
static DROP_WINDOW: AtomicIsize = AtomicIsize::new(0);
static MINIMIZE_HOOK: AtomicIsize = AtomicIsize::new(0);
static CLOAK_HOOK: AtomicIsize = AtomicIsize::new(0);
static FOREGROUND_HOOK: AtomicIsize = AtomicIsize::new(0);
static LOCATION_HOOK: AtomicIsize = AtomicIsize::new(0);
static MOVESIZE_HOOK: AtomicIsize = AtomicIsize::new(0);
/// The window in front before the Alt+Tab switcher opened.
static LAST_FRONT: AtomicIsize = AtomicIsize::new(0);

/// One drive scan at a time; changes during a scan request one more pass.
struct BinQueryGate(AtomicU8);

impl BinQueryGate {
    const RUNNING: u8 = 1;
    const DIRTY: u8 = 2;

    const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    fn request(&self) -> bool {
        if self.0.fetch_or(Self::DIRTY, Ordering::AcqRel) & Self::RUNNING != 0 {
            return false;
        }
        self.0
            .compare_exchange(
                Self::DIRTY,
                Self::RUNNING | Self::DIRTY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn begin_pass(&self) {
        self.0.fetch_and(!Self::DIRTY, Ordering::AcqRel);
    }

    fn needs_another_pass(&self) -> bool {
        self.0
            .compare_exchange(Self::RUNNING, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    }
}

static BIN_QUERY: BinQueryGate = BinQueryGate::new();

#[derive(Clone, PartialEq)]
struct Entry {
    kind: ItemKind,
    id: Option<String>,
    role: Role,
    running: bool,
    /// A group's apps, in order; empty for anything else. Part of the
    /// scene's key: the group's tile shows them.
    members: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    App,
    /// Pinned apps gathered in one icon; a click unfolds them.
    Group,
    Start,
    /// A minimized window, shown as a live thumbnail.
    Window,
    Folder,
    /// The shortcuts folder: a click opens its contents as a menu.
    Shortcuts,
    Trash,
}

fn window_id(hwnd: isize) -> String {
    format!("{WINDOW_ID_PREFIX}{hwnd}")
}

fn window_of(id: &str) -> Option<isize> {
    id.strip_prefix(WINDOW_ID_PREFIX)?.parse().ok()
}

/// Work deferred until the app state is released. These calls may open a modal
/// loop (a menu, shell error dialog, or recycle bin confirmation) that dispatches
/// messages back to the dock.
pub(crate) enum Outside {
    Open(String),
    OpenStart,
    Activate(Vec<isize>),
    Minimize(Vec<isize>),
    EndTask(PreparedEndTask),
    Reveal(String),
    EmptyBin,
    OpenWith(String, Vec<String>),
    Recycle(Vec<String>),
    Transfer {
        folder: String,
        files: Vec<String>,
        copy: bool,
    },
    Menu(MenuRequest),
    /// The shortcuts folder's contents, as a menu beside its icon.
    FolderMenu {
        folder: String,
        anchor: POINT,
        placement: menu::Placement,
        dark: bool,
    },
    InputMenu {
        gfx: Gfx,
        anchor: POINT,
        dark: bool,
    },
    QuitDock,
    OpenSettings,
    OpenAsAdmin(String),
    /// Unpin from the Windows taskbar too (see `pins::unpin_from_taskbar`).
    UnpinFromTaskbar(String),
    /// An entry of the jump list of the app `program`.
    OpenJump {
        target: jumplist::Target,
        program: String,
    },
    ShowDesktop,
}

pub(crate) struct MenuRequest {
    gfx: Gfx,
    target: MenuTarget,
    items: Vec<MenuItem>,
    anchor: POINT,
    placement: menu::Placement,
    dark: bool,
}

/// Preview positions follow discovery order, independently of activation's
/// z-order. Only open tracked windows are retained; a reopened window is new.
#[derive(Default)]
struct PreviewOrder(Vec<isize>);

impl PreviewOrder {
    fn update(&mut self, windows: &[TrackedWindow]) {
        self.0
            .retain(|hwnd| windows.iter().any(|window| window.hwnd == *hwnd));
        // EnumWindows returns topmost first. Append a newly discovered batch
        // back-to-front so its frontmost (usually newest) window goes last.
        for window in windows.iter().rev() {
            if !self.0.contains(&window.hwnd) {
                self.0.push(window.hwnd);
            }
        }
    }

    fn windows_of(&self, windows: &[TrackedWindow], app: &str) -> Vec<isize> {
        self.0
            .iter()
            .copied()
            .filter(|hwnd| {
                windows
                    .iter()
                    .any(|window| window.hwnd == *hwnd && window.app_id == app)
            })
            .collect()
    }
}

enum MenuTarget {
    Entry {
        id: String,
        role: Role,
        windows: Vec<isize>,
        end_task: Option<PreparedEndTask>,
        /// What each jump list command opens, from `CMD_JUMP` on.
        jump: Vec<jumplist::Target>,
    },
    Dock,
}

struct Asset {
    icon: Option<CompositionSurfaceBrush>,
    label: Option<LabelBrush>,
    /// A group's apps when its tile was drawn; empty for anything else.
    drawn_for: Vec<String>,
}

struct Press {
    id: String,
    at: POINT,
    draggable: bool,
}

struct Dragging {
    id: String,
    point: POINT,
    removing: bool,
    /// The pinned app or group it would join if released now.
    merge: Option<String>,
    /// Dragged out of an open group (see `start_group_drag`).
    from_group: bool,
}

/// What dropping the dragged files at a given place would do.
enum DropAction {
    Nothing,
    OpenWith(String),
    Recycle,
    Transfer { folder: String, copy: bool },
    Pin { id: String, before: Option<String> },
}

impl DropAction {
    fn effect(&self) -> DROPEFFECT {
        match self {
            Self::Nothing => DROPEFFECT_NONE,
            Self::OpenWith(_) => DROPEFFECT_COPY,
            Self::Recycle => DROPEFFECT_MOVE,
            Self::Transfer { copy, .. } => {
                if *copy {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_MOVE
                }
            }
            Self::Pin { .. } => DROPEFFECT_LINK,
        }
    }
}

struct Bounce {
    start: Instant,
    /// Seconds after `start` when bouncing ends. `None` means until the app opens a window.
    until: Option<f32>,
}

/// State that determines the scene; recomposition is unnecessary if it is unchanged.
#[derive(PartialEq)]
struct StageKey {
    screen: RECT,
    /// The screen edge the dock sits on.
    edge: DockEdge,
    /// The menu bar's strip at the top of this screen: a side dock is
    /// centred in the height below it.
    top_inset: i32,
    scale: f32,
    /// The theme, which follows Windows unless Settings picked one. A
    /// backdrop brush made while Windows had transparency off stays black
    /// after it comes back, so its `glass` rebuilds the dock both ways.
    look: crate::appearance::Look,
    bin_full: bool,
    auto_hide: bool,
    smart_hide: bool,
    icon_size: u32,
    density: DockDensity,
    max_scale: f32,
    indicators: bool,
    entries: Vec<Entry>,
}

/// Where the Alt+Tab selection is believed to be.
struct SwitcherFollow {
    /// The windows the switcher lists, in its order.
    order: Vec<isize>,
    index: usize,
    tab_down: bool,
}

struct Stage {
    key: StageKey,
    window_zone: RECT,
    scene: Scene,
    reorder: Option<ReorderMotion>,
    layout: Layout,
    magnifier: Magnifier,
    icon: f32,
    gap: f32,
    scale: f32,
    /// The resting dock on the screen: maps dock coordinates (along, across)
    /// to screen pixels and back, whatever edge it is on.
    frame: DockFrame,
    /// The same, inside the visual window.
    window_frame: DockFrame,
    rest_zone: RECT,
    hover_zone: RECT,
    /// Thin strip at the screen edge that reveals the dock when auto-hide is on.
    edge_zone: RECT,
    /// How far the dock slides toward its screen edge to hide completely.
    slide_distance: f32,
}

impl Stage {
    /// Dock coordinates (along, across) of a screen point. Across is
    /// negative away from the screen edge.
    fn to_dock(&self, point: POINT) -> (f32, f32) {
        self.frame.to_dock(point.x as f32, point.y as f32)
    }

    /// The screen box of a box in dock coordinates.
    fn screen_rect(&self, rect: Rect) -> RECT {
        let rect = self.frame.rect(rect);
        RECT {
            left: rect.x.round() as i32,
            top: rect.y.round() as i32,
            right: (rect.x + rect.width).round() as i32,
            bottom: (rect.y + rect.height).round() as i32,
        }
    }

    /// The screen point `gap` pixels off the box `rect` (dock coordinates)
    /// on its side away from the screen edge, centred: where menus and
    /// popups for an icon hang.
    fn beside(&self, rect: Rect, gap: i32) -> POINT {
        let r = self.frame.rect(rect);
        let (middle_x, middle_y) = (
            (r.x + r.width / 2.0).round() as i32,
            (r.y + r.height / 2.0).round() as i32,
        );
        match self.frame.edge {
            DockEdge::Bottom => POINT {
                x: middle_x,
                y: r.y.round() as i32 - gap,
            },
            DockEdge::Left => POINT {
                x: (r.x + r.width).round() as i32 + gap,
                y: middle_y,
            },
            DockEdge::Right => POINT {
                x: r.x.round() as i32 - gap,
                y: middle_y,
            },
        }
    }

    /// Which way a menu or popup for an icon opens: away from the edge.
    fn menu_placement(&self) -> menu::Placement {
        match self.frame.edge {
            DockEdge::Bottom => menu::Placement::Above,
            DockEdge::Left => menu::Placement::Right,
            DockEdge::Right => menu::Placement::Left,
        }
    }

    /// Where the dock starts along the screen axis it runs on.
    fn along_origin(&self) -> f32 {
        if self.frame.edge.is_vertical() {
            self.frame.origin.1
        } else {
            self.frame.origin.0
        }
    }

    /// Whether `point` is within the span of the reveal strip (the pointer
    /// came out of it toward the dock rather than past its end).
    fn spans_reveal(&self, point: POINT) -> bool {
        let zone = self.edge_zone;
        if self.frame.edge.is_vertical() {
            point.y >= zone.top && point.y < zone.bottom
        } else {
            point.x >= zone.left && point.x < zone.right
        }
    }
}

struct ReorderMotion {
    started: Instant,
    offsets: Vec<f32>,
}

pub(crate) struct App {
    visual: HWND,
    input: HWND,
    gfx: Gfx,
    target: DesktopWindowTarget,
    appbar: Option<AppBar>,
    taskbar_worker: Option<thread::JoinHandle<()>>,
    taskbar_retry: bool,
    taskbar_attempts: u8,
    fullscreen: bool,
    smart_hidden: bool,
    roster: Roster,
    tracker: WindowTracker,
    windows: Vec<TrackedWindow>,
    preview_order: PreviewOrder,
    /// Where each window was last seen on screen while not maximized, for
    /// the genie of snapped windows (see `tracker::rect_before_minimize`).
    last_rects: HashMap<isize, RECT>,
    entries: Vec<Entry>,
    icons: Visuals,
    downloads: Option<String>,
    bin_full: bool,
    _bin_watch: Option<BinWatch>,
    /// Energy saver on or off: glass follows (see `system::transparency_enabled`).
    _saver_watch: power::SaverWatch,
    /// Hears pins and unpins on the Windows taskbar.
    pin_watch: Option<nosih_shell::watch::RegistryWatch>,
    badge_watch: Option<nosih_shell::watch::RegistryWatch>,
    attention: Attention,
    attention_enabled: bool,
    assets: HashMap<String, Asset>,
    /// Scale, theme and icon pixel size the cached assets were drawn for.
    /// Scale, theme, icon size and dock edge the assets were drawn for
    /// (labels point at their icon, so they depend on the edge).
    assets_for: (f32, bool, u32, DockEdge),
    placeholder: Option<CompositionSurfaceBrush>,
    remove_label: Option<LabelBrush>,
    config: Config,
    /// 0 = shown, 1 = hidden (auto-hide).
    slide: Spring,
    press: Option<Press>,
    drag: Option<Dragging>,
    /// Files being dragged over the dock from another app.
    incoming: Vec<String>,
    /// What dropping `incoming` between icons would pin.
    incoming_pin: Option<String>,
    /// Something (files, text, an image) is being dragged over the dock.
    drag_over: bool,
    /// The entry a drag is resting on, which springs open on `SPRING_TIMER`.
    spring: Option<String>,
    /// Whether the group view was unfolded by a spring action during this drag.
    spring_opened_group: bool,
    /// The Alt+Tab switcher is open; an auto-hidden dock shows meanwhile.
    switching: bool,
    switch_started: Instant,
    switch_ended: Option<Instant>,
    /// When the dock started (the update waits a little after it).
    launched: Instant,
    /// When this dock last started a background update.
    update_tried: Option<Instant>,
    /// The selection in the switcher, as far as it can be followed.
    follow: Option<SwitcherFollow>,
    /// The magnifier rests on the icon selected in Alt+Tab, not the pointer.
    pointed: bool,
    pending_drop: Option<Outside>,
    bounces: HashMap<String, Bounce>,
    /// Minimized windows, in the order they were minimized.
    minimized: Vec<isize>,
    thumbnails: Thumbnails,
    /// Genie animations in progress.
    flights: Vec<Flight>,
    /// What the windows minimized with the genie looked like, oldest first.
    snapshots: Vec<(isize, Snapshot)>,
    menubar: Option<MenuBar>,
    spotlight: Option<Spotlight>,
    hud: Option<Hud>,
    edge_watch: Option<EdgeWatch>,
    /// Screen point identifying the last display chosen by a bottom-edge dwell.
    dock_display: Option<POINT>,
    /// Display names already looked up, by app id.
    names: HashMap<String, String>,
    /// Windows whose system transitions are turned off (see `calm_windows`).
    calmed: Vec<isize>,
    /// Windows already minimized when the dock starts get no animation.
    started: bool,
    stage: Option<Stage>,
    hovering: bool,
    hovered: Option<usize>,
    preview_candidate: Option<String>,
    preview: Option<MultiWindowView>,
    /// A group's apps, unfolded above it after a click.
    group_view: Option<GroupView>,
    /// The group whose view a press on the dock closed: releasing on that
    /// group then leaves it closed rather than opening it again.
    group_was_open: Option<String>,
    /// The icon following the pointer while an app is dragged out of a group.
    ghost: Option<DragGhost>,
    menu_open: bool,
    /// The crab, while it is out (see `perform`).
    mascot: Option<mascot::Mascot>,
    /// Charging and energy saver at the last power change.
    power_seen: Option<(Option<bool>, bool)>,
    /// When the charger act last played.
    charging_shown: Option<Instant>,
    /// The mascot's other moments (see `moments`).
    moments: crate::moments::Moments,
    /// Welcome attempts left while the mascot cannot come out.
    welcome_tries: u8,
    /// Horizontal cursor position relative to the resting dock.
    cursor: f32,
    /// 0 = resting, 1 = full magnification.
    amount: Spring,
    last_frame: Instant,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(max - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

fn place(hwnd: HWND, zone: &RECT, visible: bool) -> Result<()> {
    let show = if visible {
        SWP_SHOWWINDOW
    } else {
        SWP_HIDEWINDOW
    };
    // SAFETY: `hwnd` is one of our live windows.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            zone.left,
            zone.top,
            zone.right - zone.left,
            zone.bottom - zone.top,
            SWP_NOACTIVATE | show,
        )?;
    }
    Ok(())
}

/// Runs `action` on the app and logs failures; a transient failure must not
/// terminate the dock.
pub(crate) fn with_app(what: &str, action: impl FnOnce(&mut App) -> Result<()>) {
    with_app_then(what, |app| action(app).map(|()| None));
}

/// Like `with_app`, but `action` can request work to perform after releasing
/// the app state (see [`Outside`]).
pub(crate) fn with_app_then(what: &str, action: impl FnOnce(&mut App) -> Result<Option<Outside>>) {
    let outside = APP.with(|cell| {
        // Modal calls run outside the borrow, so this should not happen. If it
        // does, discard the message rather than terminating the dock.
        let Ok(mut app) = cell.try_borrow_mut() else {
            eprintln!("nosih: {what}: el dock estaba ocupado; se ignora");
            return None;
        };
        match app.as_mut().map(action) {
            Some(Ok(outside)) => outside,
            Some(Err(error)) => {
                crate::recovery::graphics_error(&error);
                nosih_shell::log::line(&format!("{what}: {error:#}"));
                None
            }
            None => None,
        }
    });
    if let Some(outside) = outside {
        run_outside(outside);
    }
}
