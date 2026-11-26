//! Minimal macOS-style menu bar across the top of the screen.
//!
//! Hiding the Windows taskbar also hides its clock and status area; this bar
//! brings them back: the name of the app in front, the battery, a control
//! center button (opens the Windows quick settings: network, volume,
//! brightness) and the date and time (opens the notification center).
//!
//! Cost while idle: one timer that fires once a minute, on the minute, to
//! redraw the clock. Everything else is event-driven.
//!
//! With auto-hide on, the bar reserves no space and stays off screen. Resting
//! the pointer briefly at the top edge reveals it.
//!
//! Status items, right to left as on a Mac: clock, control center, search,
//! network, volume, battery, then Task View and Widgets. Search, Task View and
//! Widgets appear only if the user has them on the Windows taskbar. Network
//! and volume changes arrive through separate notifications; nothing polls.

use anyhow::Result;
use nosih_config::{IslandStyle, Language};
use nosih_shell::{
    accessories::{Kind as AccessoryKind, Snapshot as AccessorySnapshot},
    appbar::{AppBar, Edge},
    control_center as shell_cc, launcher,
    status::{self, AudioWatch, Network, NetworkWatch, Privacy, TaskbarButtons, Volume},
    system::{self, Battery},
    watch::RegistryWatch,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    UI::{
        Color,
        Composition::{
            CompositionBatchTypes, CompositionBrush, CompositionRoundedRectangleGeometry,
            CompositionScopedBatch, CompositionSurfaceBrush, Compositor, ContainerVisual,
            Desktop::DesktopWindowTarget, ShapeVisual, SpriteVisual, VisualCollection,
        },
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SYSTEMTIME, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
        System::{
            LibraryLoader::GetModuleHandleW, SystemInformation::GetLocalTime,
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::Input::KeyboardAndMouse::{
            GetCapture, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
        },
        UI::WindowsAndMessaging::{
            CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, HHOOK,
            HWND_TOPMOST, IDC_ARROW, KillTimer, LoadCursorW, MSLLHOOKSTRUCT,
            PBT_POWERSETTINGCHANGE, PostMessageW, RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SetCursor, SetTimer, SetWindowPos, SetWindowsHookExW,
            UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_CAPTURECHANGED, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST,
            WM_RBUTTONDOWN, WM_SETCURSOR, WM_SETTINGCHANGE, WM_TIMECHANGE, WM_TIMER,
            WM_XBUTTONDOWN, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{BOOL, Interface, h, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    accessories, app,
    appearance::{self, Material},
    bluetooth, control_center, edge_watch,
    gfx::{Gfx, Ink, LabelBrush},
    mascot,
    media::{self, Control as MediaControl, Track},
    scene::Theme,
    strings,
    usage::{
        self, ActiveProviders, Monitor as UsageMonitor, Provider, ProviderUsage, Snapshot, State,
    },
};

// Logical pixels.
const HEIGHT: f32 = 28.0;
const SIDE_MARGIN: f32 = 14.0;
const ITEM_GAP: f32 = 16.0;
/// Extra clickable room around an item.
const HIT_SLOP: f32 = 7.0;

const MEDIA_CHANGED: u32 = WM_APP + 28;
const VOLUME_CAPTURE_LOST: u32 = WM_APP + 31;
const AUDIO_CHANGED: u32 = WM_APP + 32;
const AUDIO_TICK: u32 = WM_APP + 33;
const BLUETOOTH_CHANGED: u32 = WM_APP + 34;
const CONTROL_CENTER_CHANGED: u32 = WM_APP + 35;
const VOLUME_TIMER: usize = 4;
const BRIGHTNESS_TIMER: usize = 8;
const CONTROL_CENTER_WIDTH: f32 = 320.0;
const PROGRESS_TIMER: usize = 5;
/// The system monitor's samples (issue #64): every 2 s while it shows, and
/// a first one soon after it appears.
const MONITOR_TIMER: usize = 6;
const MONITOR_EVERY_MS: u32 = 2_000;
const MONITOR_FIRST_MS: u32 = 500;
/// Its two lines per column, smaller than the bar's text so both fit.
const MONITOR_FONT: f32 = 10.0;
/// How far each line reaches past the middle of the bar, in logical pixels.
const MONITOR_TUCK: f32 = 1.0;
const MONITOR_COLUMN_GAP: f32 = 8.0;
/// The network lost this long is worth a word from the mascot; shorter
/// drops (Wi-Fi roaming, a router restarting) say nothing.
const NETWORK_TIMER: usize = 7;
const OFFLINE_GRACE: Duration = Duration::from_secs(8);
const ACCESSORIES_CHANGED: u32 = WM_APP + 30;

const CLOCK_TIMER: usize = 1;
const POINTER_TIMER: usize = 2;
const POINTER_CHECK_MS: u32 = 100;
const HIDE_ANIMATION_TIMER: usize = 3;
const REVEAL_MS: u32 = 180;
const HIDE_MS: u32 = 130;
// Recovery only; normal completion comes from the compositor, not this timer.
const HIDE_FALLBACK_MS: u32 = HIDE_MS + 250;
const HIDE_ANIMATION_DONE: u32 = WM_APP + 29;
const ISLAND_SINGLE_WIDTH: f32 = 145.0;
const ISLAND_EXPANDED_WIDTH: f32 = 440.0;
const ISLAND_ROW_HEIGHT: f32 = 118.0;
const BUBBLE_VERTICAL_INSET: f32 = 2.5;
const BUBBLE_MUSIC_WIDTH: f32 = 164.0;
const BUBBLE_PROVIDER_WIDTH: f32 = 62.0;
const BUBBLE_USAGE_PADDING: f32 = 16.0;
const PANEL_GAP: f32 = 5.0;
const PANEL_RADIUS: f32 = 18.0;
const PANEL_SHADOW_MARGIN: f32 = 20.0;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// AppBar notifications need a message id; the bar ignores them.
const APPBAR_CALLBACK: u32 = WM_APP + 20;
/// Posted by the network watch; audio uses its own coalesced notification.
const STATUS_CHANGED: u32 = WM_APP + 21;
/// Posted by the bar to itself to update from the message loop.
const TICK: u32 = WM_APP + 22;
const USAGE_CHANGED: u32 = WM_APP + 23;
const DISMISS_ISLAND: u32 = WM_APP + 24;
/// Privacy activity or the user's taskbar preferences changed.
const REGISTRY_CHANGED: u32 = WM_APP + 25;
/// `wParam` of WM_SETTINGCHANGE when the work area changes, which every
/// AppBar reservation (the dock's, this bar's) causes.
const SPI_SETWORKAREA: usize = 0x002F;
/// A `TICK` is already posted.
static TICK_PENDING: AtomicBool = AtomicBool::new(false);
static AUDIO_TICK_PENDING: AtomicBool = AtomicBool::new(false);
static TRIGGERS_LEFT: AtomicI32 = AtomicI32::new(0);
static TRIGGERS_RIGHT: AtomicI32 = AtomicI32::new(0);
static PANEL_LEFT: AtomicI32 = AtomicI32::new(0);
static PANEL_RIGHT: AtomicI32 = AtomicI32::new(0);
static BAR_TOP: AtomicI32 = AtomicI32::new(0);
static BAR_BOTTOM: AtomicI32 = AtomicI32::new(0);
static PANEL_BOTTOM: AtomicI32 = AtomicI32::new(0);
/// The hook exists only while the island is expanded.
static ISLAND_HOOK_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Segoe Fluent Icons code points.
pub(crate) mod glyph {
    pub const ETHERNET: char = '\u{E839}';
    pub const OFFLINE: char = '\u{F384}';
    pub const VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
    pub const MUTE: char = '\u{E74F}';
    pub const SEARCH: char = '\u{E721}';
    pub const TASK_VIEW: char = '\u{E7C4}';
    pub const WIDGETS: char = '\u{F246}';
    /// The taskbar's "show hidden icons" arrow.
    pub const HIDDEN_ICONS: char = '\u{E70E}';
    pub const MICROPHONE: char = '\u{E720}';
    pub const CAMERA: char = '\u{E722}';
    pub const MOUSE: char = '\u{E962}';
    pub const HEADSET: char = '\u{E95B}';
    pub const BLUETOOTH: char = '\u{E702}';
    /// The bell of the Windows 11 notification center ("Ringer").
    pub const NOTIFICATIONS: char = '\u{EA8F}';
    pub const WIFI: char = '\u{E701}';
    pub const FOCUS: char = '\u{E708}';
    pub const DARK_MODE: char = '\u{E793}';
    pub const BRIGHTNESS: char = '\u{E706}';
    pub const SNIP: char = '\u{E8C6}';
    pub const LOCK: char = '\u{E72E}';
    pub const SETTINGS: char = '\u{E713}';
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    AccessoryIndicator,
    QuickSettings,
    Notifications,
    /// The clock: the user's own target, or the notification center.
    Clock,
    /// The battery: power settings.
    Power,
    /// The Wi-Fi, Ethernet or offline icon: the list of networks.
    Network,
    Sound,
    Search,
    TaskView,
    Widgets,
    HiddenIcons,
    InputLanguage,
    Microphone,
    Camera,
    Bluetooth,
    /// The system monitor: Task Manager, which lists what uses each.
    SystemMonitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuBarWidgets {
    pub island_style: IslandStyle,
    /// The notification bell at the right end (`Config::menu_bar_notifications`).
    pub notifications: bool,
    /// CPU, memory and network speed (`Config::menu_bar_system_monitor`).
    pub system_monitor: bool,
}

impl Default for MenuBarWidgets {
    fn default() -> Self {
        Self {
            island_style: IslandStyle::Notch,
            notifications: false,
            system_monitor: false,
        }
    }
}

/// Network, volume and taskbar buttons, read only when Windows reports a change.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Status {
    bluetooth_on: bool,
    network: Network,
    volume: Option<Volume>,
    buttons: TaskbarButtons,
    privacy: Privacy,
    battery_percentage: bool,
}

/// What the bar was last drawn for.
#[derive(Clone, PartialEq)]
struct Look {
    accessories: [Option<AccessoryReading>; 2],
    /// The system monitor's lines (see `MenuBar::monitor_lines`) while on.
    monitor: Option<[String; 4]>,
    language: Language,
    screen: RECT,
    scale: f32,
    /// The theme's material; `light` and `glass` are its other two halves
    /// (see `appearance::Look`).
    material: Material,
    light: bool,
    /// See `system::transparency_enabled`.
    glass: bool,
    app_name: String,
    clock: String,
    /// The input language indicator, when more than one is installed.
    input: Option<String>,
    battery: Option<Battery>,
    status: Status,
    widgets: MenuBarWidgets,
    usage: Snapshot,
    usage_enabled: bool,
    active_providers: ActiveProviders,
    panel: Option<Island>,
    usage_refreshing: bool,
    track: Option<Track>,
    control_center: control_center::State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AccessoryReading {
    kind: AccessoryKind,
    percent: Option<u8>,
}

// Compare only visible readings: unrelated names/devices do not redraw the bar.
fn accessory_readings(snapshot: &AccessorySnapshot) -> [Option<AccessoryReading>; 2] {
    snapshot.compact().map(|device| {
        device.map(|device| AccessoryReading {
            kind: device.kind,
            percent: device.percent,
        })
    })
}

impl Look {
    fn panel_content_height(&self) -> f32 {
        match self.panel {
            Some(Island::Music) => 180.0,
            Some(Island::ControlCenter) => control_center_height(self.track.is_some()),
            Some(Island::Usage) => {
                74.0 + Provider::ALL
                    .into_iter()
                    .filter(|provider| self.active_providers.has(*provider))
                    .map(|provider| usage_row_height(self.usage.get(provider)))
                    .sum::<f32>()
            }
            None => 0.0,
        }
    }

    fn panel_height(&self) -> f32 {
        let natural = self.panel_content_height();
        if self.panel == Some(Island::Usage) {
            let available = (self.screen.bottom - self.screen.top) as f32 / self.scale
                - HEIGHT
                - PANEL_GAP
                - PANEL_SHADOW_MARGIN;
            natural.min(available.max(0.0))
        } else {
            natural
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Island {
    Music,
    Usage,
    ControlCenter,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Span {
    left: f32,
    right: f32,
}

impl Span {
    fn width(self) -> f32 {
        self.right - self.left
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct VolumeTrack {
    center_x: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct HorizontalTrack {
    left: f32,
    right: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum SliderDrag {
    VerticalVolume(VolumeTrack),
    HorizontalVolume(HorizontalTrack),
    HorizontalBrightness(HorizontalTrack),
}

#[derive(Clone, Copy)]
enum ControlCenterAction {
    ToggleWifi,
    ToggleBluetooth,
    ToggleDoNotDisturb,
    ToggleDarkMode,
    ScreenSnip,
    LockScreen,
    OpenPower,
    OpenNetworkList,
    OpenSettings,
}

#[derive(Clone, Copy)]
enum IslandAction {
    Toggle(Island),
    Media(MediaControl),
    Volume(VolumeTrack),
    HorizontalVolume(HorizontalTrack),
    HorizontalBrightness(HorizontalTrack),
    Mute,
    Refresh,
    ScrollUsage(i8),
    ControlCenter(ControlCenterAction),
}

#[derive(Clone, Copy)]
struct IslandHit {
    bounds: (f32, f32, f32, f32),
    action: IslandAction,
}

/// A retained, clipped body. Wheel/arrow input only changes its transform and
/// arrow opacity; no quota queries or redraws.
struct UsageViewport {
    content: ContainerVisual,
    maximum: f32,
    scale: f32,
    bounds: (f32, f32, f32, f32),
    up: SpriteVisual,
    down: SpriteVisual,
}

impl IslandHit {
    fn at(&self, x: f32, y: f32) -> bool {
        let (left, top, right, bottom) = self.bounds;
        x >= left && x < right && y >= top && y < bottom
    }
}

struct ExitAnimation {
    batch: CompositionScopedBatch,
    completed: i64,
}

/// Only these retained visuals move during a volume gesture. Album artwork,
/// track labels and the rest of the menu bar keep their existing visual tree.
struct VolumeControls {
    fill: CompositionRoundedRectangleGeometry,
    value: SpriteVisual,
    geometry: IslandGeometry,
    height: f32,
    position_percent: Option<u8>,
    label_percent: u8,
}

/// Only the progress fill and time labels change while the popup is open.
struct ProgressControls {
    fill: CompositionRoundedRectangleGeometry,
    elapsed: SpriteVisual,
    total: SpriteVisual,
    geometry: IslandGeometry,
    width: f32,
    elapsed_seconds: Option<u64>,
    total_seconds: Option<u64>,
}

impl ProgressControls {
    fn paint(&mut self, gfx: &Gfx, timeline: media::Timeline, now: Instant) -> Result<()> {
        let position = timeline.position_at(now);
        let px = |v| self.geometry.px(v);
        let filled = self.width * (position.as_secs_f64() / timeline.duration.as_secs_f64()) as f32;
        self.fill.SetSize(Vector2::new(filled, px(4.0)))?;
        self.fill
            .SetCornerRadius(Vector2::new(px(2.0).min(filled / 2.0), px(2.0)))?;
        for (visual, cached, seconds, right) in [
            (
                &self.elapsed,
                &mut self.elapsed_seconds,
                position.as_secs(),
                false,
            ),
            (
                &self.total,
                &mut self.total_seconds,
                timeline.duration.as_secs(),
                true,
            ),
        ] {
            if *cached == Some(seconds) {
                continue;
            }
            let label = gfx.text_brush_sized(
                &media_time(seconds),
                self.geometry.scale,
                10.0,
                false,
                self.geometry.ink(),
            )?;
            visual.SetBrush(&label.brush.cast::<CompositionBrush>()?)?;
            visual.SetSize(Vector2::new(label.width, label.height))?;
            if right {
                visual.SetOffset(Vector3::new(
                    px(26.0) + self.width - label.width,
                    px(148.0),
                    0.0,
                ))?;
            }
            *cached = Some(seconds);
        }
        Ok(())
    }
}

impl VolumeControls {
    fn move_to(&mut self, percent: u8) -> Result<()> {
        if self.position_percent == Some(percent) {
            return Ok(());
        }
        let px = |v| self.geometry.px(v);
        let filled = self.height * f32::from(percent) / 100.0;
        self.fill.SetSize(Vector2::new(px(4.0), filled))?;
        self.fill
            .SetOffset(Vector2::new(0.0, self.height - filled))?;
        self.fill
            .SetCornerRadius(Vector2::new(px(2.0), px(2.0).min(filled / 2.0)))?;
        self.position_percent = Some(percent);
        Ok(())
    }

    fn paint(&mut self, gfx: &Gfx, percent: u8) -> Result<()> {
        self.move_to(percent)?;
        if self.label_percent != percent {
            let label = gfx.text_brush_sized(
                &format!("{percent}%"),
                self.geometry.scale,
                10.5,
                false,
                self.geometry.ink(),
            )?;
            self.value
                .SetBrush(&label.brush.cast::<CompositionBrush>()?)?;
            self.value
                .SetSize(Vector2::new(label.width, label.height))?;
            self.value.SetOffset(Vector3::new(
                (self.geometry.width - label.width) / 2.0,
                self.geometry.px(12.0),
                0.0,
            ))?;
            self.label_percent = percent;
        }
        Ok(())
    }
}

/// Retained visuals for horizontal macOS-style Control Center sliders (Display & Sound).
struct HorizontalSliderControls {
    fill: CompositionRoundedRectangleGeometry,
    value: SpriteVisual,
    geometry: IslandGeometry,
    track_width: f32,
    track_height: f32,
    value_right: f32,
    value_y: f32,
    position_percent: Option<u8>,
    label_percent: Option<u8>,
    muted: bool,
}

impl HorizontalSliderControls {
    fn move_to(&mut self, percent: u8) -> Result<()> {
        let clamped = percent.min(100);
        if self.position_percent == Some(clamped) {
            return Ok(());
        }
        let filled = if clamped == 0 {
            0.0
        } else {
            (self.track_width * f32::from(clamped) / 100.0)
                .max(self.track_height)
                .min(self.track_width)
        };
        let radius = (self.track_height / 2.0).min(filled / 2.0);
        self.fill.SetSize(Vector2::new(filled, self.track_height))?;
        self.fill.SetCornerRadius(Vector2::new(radius, radius))?;
        self.position_percent = Some(clamped);
        Ok(())
    }

    fn paint(&mut self, gfx: &Gfx, percent: u8, muted: bool) -> Result<()> {
        let clamped = percent.min(100);
        self.move_to(if muted { 0 } else { clamped })?;
        if self.label_percent != Some(clamped) || self.muted != muted {
            let text = if muted {
                strings::text().cc_muted.to_owned()
            } else {
                format!("{clamped}%")
            };
            let label =
                gfx.text_brush_sized(&text, self.geometry.scale, 11.0, false, self.geometry.ink())?;
            self.value
                .SetBrush(&label.brush.cast::<CompositionBrush>()?)?;
            self.value
                .SetSize(Vector2::new(label.width, label.height))?;
            self.value.SetOffset(Vector3::new(
                (self.value_right - label.width).round(),
                self.value_y.round(),
                0.0,
            ))?;
            self.label_percent = Some(clamped);
            self.muted = muted;
        }
        Ok(())
    }
}

impl Drop for ExitAnimation {
    fn drop(&mut self) {
        let _ = self.batch.RemoveCompleted(self.completed);
    }
}

pub struct MenuBar {
    bluetooth: Option<bluetooth::Monitor>,
    accessories: Option<accessories::Monitor>,
    accessories_enabled: bool,
    accessory_retry_at: Instant,
    /// The system monitor's counters, only while it samples (issue #64).
    monitor: Option<nosih_shell::monitor::Sampler>,
    monitor_reading: nosih_shell::monitor::Reading,
    /// Widest each monitor column has been at this scale: digits changing
    /// width must not shift the rest of the bar every two seconds.
    monitor_widths: (f32, [f32; 2]),
    hwnd: HWND,
    _target: DesktopWindowTarget,
    /// Stable parent: content refreshes must not restart reveal animations.
    surface: ContainerVisual,
    appbar: Option<AppBar>,
    app_name: String,
    look: Option<Look>,
    /// Clickable spans, in window pixels.
    zones: Vec<(f32, f32, Action)>,
    visible: bool,
    /// Whether the bar hides now, until the pointer reaches the top edge:
    /// always with "Automatically hide", and with "Intelligent" while the
    /// window in front reaches under it (`covered`).
    auto_hide: bool,
    /// The user's choice of hiding (`Config::menu_bar_auto_hide`).
    hide_setting: bool,
    /// Intelligent hiding (`Config::menu_bar_smart_hide`, issue #61): like
    /// the dock's, the bar hides only while the window in front reaches
    /// under it. It reserves no strip of the screen then, or each switch
    /// would resize every maximized window.
    smart: bool,
    /// The window in front reaches under the bar (see `set_covered`).
    covered: bool,
    /// With auto-hide on: the pointer is at the top edge, so the bar shows.
    revealed: bool,
    hide_until: Option<Instant>,
    animation_epoch: usize,
    exit_animation: Option<ExitAnimation>,
    /// Where the full bar goes on screen.
    bounds: RECT,
    status: Status,
    widgets: MenuBarWidgets,
    /// What the clock opens (`Config::menu_bar_clock_target`). Not drawn, so
    /// it lives outside `widgets` and changing it redraws nothing.
    clock_target: Option<String>,
    usage: Option<UsageMonitor>,
    /// Where each provider sits in the closed island, as screen x.
    usage_anchors: Vec<(Provider, i32)>,
    /// A moment for the crab, waiting for the dock (see `take_cue`).
    cue: Option<(mascot::Act, i32)>,
    /// Whether the network was up at the last reading (`None` before the
    /// first), since when it is down, and whether that was told.
    online: Option<bool>,
    offline_since: Option<Instant>,
    offline_told: bool,
    media: Option<media::Watch>,
    /// "Show what is playing": off, nothing listens to media sessions.
    media_enabled: bool,
    cover: Option<(
        Option<Arc<nosih_shell::icons::IconImage>>,
        CompositionSurfaceBrush,
    )>,
    control_center: Option<control_center::Controller>,
    slider_drag: Option<SliderDrag>,
    pending_volume: Option<u8>,
    volume_target: Option<u8>,
    pending_brightness: Option<u8>,
    brightness_target: Option<u8>,
    volume_controls: Option<VolumeControls>,
    cc_volume_controls: Option<HorizontalSliderControls>,
    cc_brightness_controls: Option<HorizontalSliderControls>,
    progress_controls: Option<ProgressControls>,
    progress_timer: bool,
    usage_scroll: f32,
    usage_viewport: Option<UsageViewport>,
    suppress_volume_release: bool,
    panel: Option<Island>,
    visual_height: i32,
    triggers_span: Span,
    panel_span: Span,
    island_hits: Vec<IslandHit>,
    outside_click_hook: Option<HHOOK>,
    /// Held for their callbacks; dropping them unsubscribes.
    _network: Option<NetworkWatch>,
    /// Hears when apps start or stop using the microphone or camera.
    privacy_watch: Option<RegistryWatch>,
    /// Windows' taskbar preferences, including the battery percentage.
    settings_watch: Option<RegistryWatch>,
    /// Search uses its own Windows preference key.
    search_watch: Option<RegistryWatch>,
    /// Energy saver on or off arrives as `WM_POWERBROADCAST`; glass follows.
    _saver_watch: nosih_shell::power::SaverWatch,
    audio: Option<AudioWatch>,
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            launcher::note_menu_bar_press();
            app::menu_bar_volume_press((l.0 & 0xFFFF) as i16 as f32, (l.0 >> 16) as i16 as f32);
        }
        WM_LBUTTONUP => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = (l.0 >> 16) as i16;
            if !app::menu_bar_volume_release(Some((f32::from(x), f32::from(y)))) {
                app::menu_bar_clicked(f32::from(x), f32::from(y));
            }
        }
        WM_MOUSEMOVE => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = (l.0 >> 16) as i16;
            app::menu_bar_pointer_moved(f32::from(x), f32::from(y));
        }
        WM_MOUSELEAVE => app::menu_bar_check_pointer(),
        WM_MOUSEWHEEL => {
            let delta = (w.0 >> 16) as u16 as i16;
            let point = POINT {
                x: (l.0 & 0xFFFF) as i16 as i32,
                y: (l.0 >> 16) as i16 as i32,
            };
            if app::menu_bar_scrolled(point, delta) {
                return LRESULT(0);
            }
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        WM_TIMER if w.0 == POINTER_TIMER => app::menu_bar_check_pointer(),
        WM_TIMER if w.0 == VOLUME_TIMER => app::menu_bar_volume_flush(),
        WM_TIMER if w.0 == BRIGHTNESS_TIMER => app::menu_bar_brightness_flush(),
        WM_TIMER if w.0 == PROGRESS_TIMER => app::menu_bar_progress_tick(),
        WM_TIMER if w.0 == MONITOR_TIMER => app::menu_bar_monitor_tick(),
        WM_CAPTURECHANGED => {
            // Avoid reentering the app borrow inside Set/ReleaseCapture.
            unsafe {
                let _ = PostMessageW(Some(hwnd), VOLUME_CAPTURE_LOST, WPARAM(0), LPARAM(0));
            }
        }
        VOLUME_CAPTURE_LOST => {
            // A delayed notification must not cancel a newer drag.
            if unsafe { GetCapture() } != hwnd {
                app::menu_bar_volume_release(None);
            }
        }
        WM_TIMER if w.0 == HIDE_ANIMATION_TIMER => app::menu_bar_finish_hide(None),
        HIDE_ANIMATION_DONE => app::menu_bar_finish_hide(Some(w.0)),
        DISMISS_ISLAND => app::menu_bar_dismiss_island(),
        REGISTRY_CHANGED => app::menu_bar_registry_changed(),
        ACCESSORIES_CHANGED => app::menu_bar_accessories_changed(),
        MEDIA_CHANGED => app::menu_bar_media_changed(),
        BLUETOOTH_CHANGED => app::menu_bar_bluetooth_changed(),
        CONTROL_CENTER_CHANGED => app::menu_bar_control_center_changed(),
        AUDIO_CHANGED => {
            if !AUDIO_TICK_PENDING.swap(true, Ordering::Relaxed) {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), AUDIO_TICK, WPARAM(0), LPARAM(0));
                }
            }
        }
        AUDIO_TICK => {
            AUDIO_TICK_PENDING.store(false, Ordering::Relaxed);
            app::menu_bar_audio_changed();
        }
        WM_SETCURSOR => {
            // SAFETY: Keep the standard arrow throughout the bar and popups.
            if let Ok(arrow) = unsafe { LoadCursorW(None, IDC_ARROW) } {
                unsafe { SetCursor(Some(arrow)) };
                return LRESULT(1);
            }
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        WM_NCHITTEST => {
            let x = (l.0 & 0xFFFF) as i16 as i32;
            let y = (l.0 >> 16) as i16 as i32;
            if y >= BAR_BOTTOM.load(Ordering::Relaxed)
                && (y >= PANEL_BOTTOM.load(Ordering::Relaxed)
                    || x < PANEL_LEFT.load(Ordering::Relaxed)
                    || x >= PANEL_RIGHT.load(Ordering::Relaxed))
            {
                return LRESULT(-1);
            }
            // SAFETY: Default hit testing for the bar and the island.
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        // The minute changed, the charger was plugged or unplugged, the
        // network or volume changed, or a Windows setting (taskbar buttons).
        // Nothing the bar shows depends on the work area.
        WM_SETTINGCHANGE if w.0 == SPI_SETWORKAREA => {}
        // Some of these are sent while the dock is busy reserving space, and
        // updating then was dropped. From the message loop the dock is never
        // busy, and a burst of them makes a single update.
        // WM_TIMECHANGE: the clock or time zone was changed.
        WM_TIMER | WM_POWERBROADCAST | WM_TIMECHANGE | STATUS_CHANGED | USAGE_CHANGED
        | WM_SETTINGCHANGE => {
            if msg == WM_POWERBROADCAST && matches!(w.0, 7 | 18) {
                app::menu_bar_accessories_reset();
            }
            if msg == WM_POWERBROADCAST && w.0 == PBT_POWERSETTINGCHANGE as usize {
                // SAFETY: Windows owns the setting payload for this message.
                unsafe {
                    nosih_shell::power::power_setting_changed(l);
                }
            }
            if !TICK_PENDING.swap(true, Ordering::Relaxed) {
                // SAFETY: Posting to our own live window.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), TICK, WPARAM(0), LPARAM(0));
                }
            }
        }
        TICK => {
            TICK_PENDING.store(false, Ordering::Relaxed);
            app::menu_bar_tick();
        }
        // SAFETY: Standard forwarding of unhandled messages.
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

unsafe extern "system" fn outside_click_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            w.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: Windows provides an MSLLHOOKSTRUCT for mouse hook actions.
        let point = unsafe { (*(l.0 as *const MSLLHOOKSTRUCT)).pt };
        let in_triggers = point.y >= BAR_TOP.load(Ordering::Relaxed)
            && point.y < BAR_BOTTOM.load(Ordering::Relaxed)
            && point.x >= TRIGGERS_LEFT.load(Ordering::Relaxed)
            && point.x < TRIGGERS_RIGHT.load(Ordering::Relaxed);
        let in_panel = point.y >= BAR_BOTTOM.load(Ordering::Relaxed)
            && point.y < PANEL_BOTTOM.load(Ordering::Relaxed)
            && point.x >= PANEL_LEFT.load(Ordering::Relaxed)
            && point.x < PANEL_RIGHT.load(Ordering::Relaxed);
        if !in_triggers && !in_panel {
            let window = ISLAND_HOOK_WINDOW.load(Ordering::Relaxed);
            if window != 0 {
                // SAFETY: The window remains live until the hook is removed in Drop.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        DISMISS_ISLAND,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }
    // SAFETY: Never consume the click; the target application must receive it.
    unsafe { CallNextHookEx(None, code, w, l) }
}

pub(crate) fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

pub(crate) fn add_visual(
    compositor: &Compositor,
    layers: &VisualCollection,
    brush: &CompositionBrush,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Result<SpriteVisual> {
    let visual = compositor.CreateSpriteVisual()?;
    visual.SetSize(Vector2 {
        X: width,
        Y: height,
    })?;
    visual.SetOffset(Vector3 {
        X: x.round(),
        Y: y.round(),
        Z: 0.0,
    })?;
    visual.SetBrush(brush)?;
    layers.InsertAtTop(&visual)?;
    Ok(visual)
}

#[expect(
    clippy::too_many_arguments,
    reason = "A rounded shape needs its rectangle, radius and two colors"
)]
pub(crate) fn add_rounded_box(
    compositor: &Compositor,
    layers: &VisualCollection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    fill: Color,
    border: Option<Color>,
) -> Result<()> {
    retained_rounded_box(
        compositor, layers, x, y, width, height, radius, fill, border,
    )
    .map(|_| ())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Same rectangle/color inputs as add_rounded_box; retain geometry for small UI updates"
)]
fn retained_rounded_box(
    compositor: &Compositor,
    layers: &VisualCollection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    fill: Color,
    border: Option<Color>,
) -> Result<(ShapeVisual, CompositionRoundedRectangleGeometry)> {
    let geometry = compositor.CreateRoundedRectangleGeometry()?;
    let inset = if border.is_some() { 0.5 } else { 0.0 };
    geometry.SetOffset(Vector2 { X: inset, Y: inset })?;
    geometry.SetSize(Vector2 {
        X: (width - inset * 2.0).max(0.0),
        Y: (height - inset * 2.0).max(0.0),
    })?;
    geometry.SetCornerRadius(Vector2 {
        X: radius.min(width / 2.0),
        Y: radius.min(height / 2.0),
    })?;
    let shape = compositor.CreateSpriteShapeWithGeometry(&geometry)?;
    shape.SetFillBrush(&compositor.CreateColorBrushWithColor(fill)?)?;
    if let Some(border) = border {
        shape.SetStrokeBrush(&compositor.CreateColorBrushWithColor(border)?)?;
        shape.SetStrokeThickness(1.0)?;
    }
    let visual = compositor.CreateShapeVisual()?;
    visual.SetSize(Vector2 {
        X: width,
        Y: height,
    })?;
    visual.SetOffset(Vector3 {
        X: x.round(),
        Y: y.round(),
        Z: 0.0,
    })?;
    visual.Shapes()?.Append(&shape)?;
    layers.InsertAtTop(&visual)?;
    Ok((visual, geometry))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Typography and placement are chosen together for panel labels"
)]
fn add_panel_text(
    gfx: &Gfx,
    layers: &VisualCollection,
    value: &str,
    x: f32,
    y: f32,
    scale: f32,
    font_size: f32,
    bold: bool,
    ink: Ink,
    opacity: f32,
) -> Result<LabelBrush> {
    let label = gfx.text_brush_sized(value, scale, font_size, bold, ink)?;
    add_visual(
        &gfx.compositor,
        layers,
        &label.brush.cast()?,
        x,
        y,
        label.width,
        label.height,
    )?
    .SetOpacity(opacity)?;
    Ok(label)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Right-aligned panel labels choose typography and placement together"
)]
fn add_panel_text_right(
    gfx: &Gfx,
    layers: &VisualCollection,
    value: &str,
    right: f32,
    y: f32,
    scale: f32,
    font_size: f32,
    bold: bool,
    ink: Ink,
    opacity: f32,
) -> Result<LabelBrush> {
    let label = gfx.text_brush_sized(value, scale, font_size, bold, ink)?;
    add_visual(
        &gfx.compositor,
        layers,
        &label.brush.cast()?,
        right - label.width,
        y,
        label.width,
        label.height,
    )?
    .SetOpacity(opacity)?;
    Ok(label)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Each provider row draws its own colors, data and measured rectangle"
)]
fn draw_usage_row(
    gfx: &Gfx,
    layers: &VisualCollection,
    provider: Provider,
    usage: &ProviderUsage,
    accent: Color,
    scale: f32,
    light: bool,
    island_width: f32,
    y: f32,
) -> Result<()> {
    let c = &gfx.compositor;
    let px = |logical: f32| (logical * scale).round();
    let ink = if light { Ink::Dark } else { Ink::Light };
    let text = strings::text();
    let left = px(23.0);
    let width = island_width - px(46.0);
    let right = left + width;
    add_rounded_box(
        c,
        layers,
        left + px(12.0),
        y + px(11.0),
        px(32.0),
        px(32.0),
        px(10.0),
        rgba(accent.R, accent.G, accent.B, if light { 31 } else { 41 }),
        None,
    )?;
    if let Some(image) = usage::logo(provider) {
        let icon_size = px(20.0) as u32;
        let image_brush = gfx.icon_brush(image, icon_size)?;
        let brush: CompositionBrush =
            if !matches!(provider, Provider::Claude | Provider::Gemini) && light {
                let tinted = c.CreateMaskBrush()?;
                tinted.SetMask(&image_brush)?;
                tinted.SetSource(&c.CreateColorBrushWithColor(rgba(27, 34, 45, 255))?)?;
                tinted.cast()?
            } else {
                image_brush.cast()?
            };
        add_visual(
            c,
            layers,
            &brush,
            left + px(18.0),
            y + px(17.0),
            icon_size as f32,
            icon_size as f32,
        )?;
    }
    add_panel_text(
        gfx,
        layers,
        provider.name(),
        left + px(53.0),
        y + px(10.0),
        scale,
        14.0,
        true,
        ink,
        1.0,
    )?;
    if let Some(main) = usage.main_window() {
        add_panel_text(
            gfx,
            layers,
            &main.title(),
            left + px(53.0),
            y + px(30.0),
            scale,
            11.5,
            false,
            ink,
            0.72,
        )?;
        add_panel_text_right(
            gfx,
            layers,
            &format!("{}%", main.used_percent),
            right - px(14.0),
            y + px(6.0),
            scale,
            20.0,
            true,
            ink,
            1.0,
        )?;
        add_panel_text_right(
            gfx,
            layers,
            text.usage_used,
            right - px(15.0),
            y + px(31.0),
            scale,
            11.0,
            false,
            ink,
            0.7,
        )?;
        let track_left = left + px(14.0);
        let track_width = width - px(28.0);
        let track_y = y + px(53.0);
        add_rounded_box(
            c,
            layers,
            track_left,
            track_y,
            track_width,
            px(6.0),
            px(3.0),
            if light {
                rgba(54, 73, 95, 43)
            } else {
                rgba(224, 235, 248, 48)
            },
            None,
        )?;
        let fill = track_width * f32::from(main.used_percent) / 100.0;
        if fill > 0.0 {
            add_rounded_box(
                c,
                layers,
                track_left,
                track_y,
                fill,
                px(6.0),
                px(3.0),
                accent,
                None,
            )?;
        }
        if let Some(reset) = main.reset_text() {
            add_panel_text_right(
                gfx,
                layers,
                &format!("{} {reset}", text.usage_resets),
                right - px(14.0),
                y + px(64.0),
                scale,
                11.0,
                false,
                ink,
                0.78,
            )?;
        }
        if usage.state == State::Unavailable
            || usage.age_minutes().is_some_and(|minutes| minutes > 5)
        {
            add_panel_text(
                gfx,
                layers,
                text.usage_stale,
                left + px(14.0),
                y + px(64.0),
                scale,
                11.0,
                false,
                ink,
                0.7,
            )?;
        }
        for (index, secondary) in usage.windows.iter().skip(1).enumerate() {
            add_visual(
                c,
                layers,
                &c.CreateColorBrushWithColor(if light {
                    rgba(50, 68, 88, 24)
                } else {
                    rgba(228, 236, 247, 29)
                })?
                .cast()?,
                left + px(14.0),
                y + px(83.0 + index as f32 * 26.0),
                width - px(28.0),
                px(1.0),
            )?;
            add_panel_text(
                gfx,
                layers,
                &format!("{} · {}%", secondary.title(), secondary.used_percent),
                left + px(14.0),
                y + px(87.0 + index as f32 * 26.0),
                scale,
                11.5,
                true,
                ink,
                0.94,
            )?;
            if let Some(reset) = secondary.reset_text() {
                add_panel_text_right(
                    gfx,
                    layers,
                    &format!("{} {reset}", text.usage_resets),
                    right - px(14.0),
                    y + px(87.0 + index as f32 * 26.0),
                    scale,
                    11.5,
                    false,
                    ink,
                    0.76,
                )?;
            }
        }
    } else {
        let message = match usage.state {
            State::Loading => text.usage_loading,
            State::SignIn if provider == Provider::Claude => text.usage_claude_sign_in,
            State::SignIn => text.usage_sign_in,
            State::Ready | State::Unavailable => text.usage_unavailable,
        };
        add_panel_text(
            gfx,
            layers,
            message,
            left + px(53.0),
            y + px(43.0),
            scale,
            12.0,
            false,
            ink,
            0.72,
        )?;
    }
    Ok(())
}

fn usage_row_height(usage: &ProviderUsage) -> f32 {
    ISLAND_ROW_HEIGHT + usage.windows.len().saturating_sub(2) as f32 * 26.0
}

impl MenuBar {
    pub fn create(
        gfx: &Gfx,
        usage_enabled: bool,
        media_enabled: bool,
        auto_hide: bool,
        widgets: MenuBarWidgets,
    ) -> Result<Self> {
        let class = w!("NosihMenuBar");
        // SAFETY: Standard class registration and window creation on the UI
        // thread; the window is destroyed in `Drop`.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            // Registering an existing class fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(bar_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Nosih"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            // Without this, the system backdrop brush appears black.
            let enable = BOOL::from(true);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const enable).cast(),
                size_of::<BOOL>() as u32,
            );
            hwnd
        };
        let target = unsafe {
            // SAFETY: `hwnd` is the live window created above.
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        // Our compositor owns the transition; do not combine it with DWM's
        // native show/hide effect, which can present a cached opaque frame.
        system::set_window_transitions(hwnd.0 as isize, false);
        let surface = gfx.compositor.CreateContainerVisual()?;
        target.SetRoot(&surface)?;
        let mut bar = Self {
            accessories: None,
            accessories_enabled: false,
            accessory_retry_at: Instant::now() + Duration::from_secs(60),
            monitor: None,
            monitor_reading: nosih_shell::monitor::Reading::default(),
            monitor_widths: (0.0, [0.0; 2]),
            hwnd,
            _target: target,
            surface,
            appbar: AppBar::register(hwnd, APPBAR_CALLBACK),
            app_name: String::new(),
            look: None,
            zones: Vec::new(),
            visible: true,
            auto_hide,
            hide_setting: auto_hide,
            smart: false,
            covered: false,
            revealed: false,
            hide_until: None,
            animation_epoch: 0,
            exit_animation: None,
            bounds: RECT::default(),
            status: Status {
                bluetooth_on: false,
                network: Network::Offline,
                volume: None,
                buttons: TaskbarButtons::default(),
                privacy: Privacy::default(),
                battery_percentage: false,
            },
            widgets,
            clock_target: None,
            usage: usage_enabled.then(|| UsageMonitor::new(hwnd.0 as isize, USAGE_CHANGED)),
            usage_anchors: Vec::new(),
            cue: None,
            online: None,
            offline_since: None,
            offline_told: false,
            media: media_enabled
                .then(|| media::Watch::start(hwnd, MEDIA_CHANGED))
                .flatten(),
            media_enabled,
            cover: None,
            control_center: control_center::Controller::start(hwnd, CONTROL_CENTER_CHANGED),
            slider_drag: None,
            pending_volume: None,
            volume_target: None,
            pending_brightness: None,
            brightness_target: None,
            volume_controls: None,
            cc_volume_controls: None,
            cc_brightness_controls: None,
            progress_controls: None,
            progress_timer: false,
            usage_scroll: 0.0,
            usage_viewport: None,
            suppress_volume_release: false,
            panel: None,
            visual_height: 0,
            triggers_span: Span::default(),
            panel_span: Span::default(),
            island_hits: Vec::new(),
            outside_click_hook: None,
            bluetooth: bluetooth::Monitor::new(hwnd.0 as isize, BLUETOOTH_CHANGED).ok(),
            _network: NetworkWatch::new(hwnd.0 as isize, STATUS_CHANGED),
            privacy_watch: status::watch_privacy(hwnd.0 as isize, REGISTRY_CHANGED),
            settings_watch: status::watch_taskbar_settings(hwnd.0 as isize, REGISTRY_CHANGED),
            search_watch: status::watch_search_settings(hwnd.0 as isize, REGISTRY_CHANGED),
            _saver_watch: nosih_shell::power::SaverWatch::new(hwnd.0 as isize),
            audio: AudioWatch::new(hwnd.0 as isize, AUDIO_CHANGED),
        };
        AUDIO_TICK_PENDING.store(false, Ordering::Relaxed);
        bar.update_status();
        bar.refresh(gfx)?;
        bar.schedule_next_minute();
        Ok(bar)
    }

    pub fn set_app_name(&mut self, gfx: &Gfx, name: &str) -> Result<()> {
        if self.app_name == name {
            return Ok(());
        }
        name.clone_into(&mut self.app_name);
        self.refresh(gfx)
    }

    /// Hidden while an app is full screen.
    /// How tall the bar is at `scale`: the strip it keeps at the top of the
    /// primary display unless it hides itself. A side dock starts below it.
    pub fn strip_height(scale: f32) -> i32 {
        (HEIGHT * scale).round() as i32
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<()> {
        if !visible {
            self.release_volume(None)?;
        }
        if self.visible == visible {
            return Ok(());
        }
        self.visible = visible;
        if !visible && self.auto_hide {
            self.revealed = false;
        }
        self.place()
    }

    pub fn set_auto_hide(&mut self, auto_hide: bool) -> Result<()> {
        if self.hide_setting == auto_hide {
            return Ok(());
        }
        self.hide_setting = auto_hide;
        self.apply_hiding()
    }

    /// Intelligent hiding on or off (see `smart`).
    pub fn set_smart_hide(&mut self, smart: bool) -> Result<()> {
        if self.smart == smart {
            return Ok(());
        }
        self.smart = smart;
        self.apply_hiding()
    }

    /// A change of setting applies at once, without animation.
    fn apply_hiding(&mut self) -> Result<()> {
        self.auto_hide = self.hide_setting || (self.smart && self.covered);
        self.revealed = false;
        self.hide_until = None;
        // The reserved strip may change even when hiding does not.
        self.place()
    }

    /// Whether the bar keeps a strip of the screen for itself, so windows
    /// are maximized below it: only while it neither auto-hides nor hides
    /// intelligently.
    fn reserves(&self) -> bool {
        !self.hide_setting && !self.smart
    }

    /// Where the bar is on screen, shown or not.
    pub fn bounds(&self) -> RECT {
        self.bounds
    }

    /// Intelligent hiding: whether the window in front reaches under the
    /// bar. It slides away as when the pointer leaves an auto-hiding bar,
    /// unless the pointer is on it, and slides back once nothing covers it;
    /// meanwhile the top edge reveals it as usual.
    pub fn set_covered(&mut self, gfx: &Gfx, covered: bool) -> Result<()> {
        if self.covered == covered {
            return Ok(());
        }
        self.covered = covered;
        if !self.smart || self.hide_setting {
            return Ok(());
        }
        if covered {
            // It is on screen now: hide it the way a leaving pointer does.
            self.auto_hide = true;
            self.revealed = true;
            let mut point = POINT::default();
            // SAFETY: Read-only cursor query.
            let on_bar = unsafe { GetCursorPos(&mut point) }.is_ok()
                && point.x >= self.bounds.left
                && point.x < self.bounds.right
                && point.y >= self.bounds.top
                && point.y < self.bounds.bottom;
            if on_bar || self.panel.is_some() || self.slider_drag.is_some() {
                // In use: it goes once the pointer leaves, as usual.
                return self.place();
            }
            return self.hover(gfx, false);
        }
        let was_hidden = self.auto_hide && !self.revealed;
        self.auto_hide = false;
        self.revealed = false;
        self.hide_until = None;
        self.animation_epoch = self.animation_epoch.wrapping_add(1);
        self.exit_animation = None;
        self.place()?;
        if was_hidden && self.visible && system::animations_enabled() {
            // `place` showed it in its final state; slide it in from there.
            self.surface
                .SetOffset(Vector3::new(0.0, -self.reveal_lift(), 0.0))?;
            self.surface.SetOpacity(0.0)?;
            if let Err(error) = self.animate_visibility(gfx, true) {
                nosih_shell::log::line(&format!("animación de barra superior: {error:#}"));
                self.reset_surface()?;
            }
        }
        // Content is not refreshed while hidden.
        self.refresh(gfx)
    }

    pub fn set_usage_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.usage.is_some() == enabled {
            return Ok(());
        }
        self.usage = enabled.then(|| UsageMonitor::new(self.hwnd.0 as isize, USAGE_CHANGED));
        self.refresh(gfx)
    }

    /// Shows or hides what is playing. Off, the media watch stops too, so
    /// nothing follows playback changes.
    pub fn set_media_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.media_enabled == enabled {
            return Ok(());
        }
        self.media_enabled = enabled;
        self.media = enabled
            .then(|| media::Watch::start(self.hwnd, MEDIA_CHANGED))
            .flatten();
        self.refresh(gfx)
    }

    pub fn set_accessories_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.accessories_enabled == enabled {
            return Ok(());
        }
        self.accessories_enabled = enabled;
        if enabled {
            self.restart_accessories(gfx)
        } else {
            self.accessories = None;
            self.refresh(gfx)
        }
    }

    pub fn accessories_changed(&mut self, gfx: &Gfx) -> Result<()> {
        if self.accessories_enabled && self.visible && (!self.auto_hide || self.revealed) {
            self.refresh(gfx)?;
        }
        Ok(())
    }

    pub fn restart_accessories(&mut self, gfx: &Gfx) -> Result<()> {
        // Resume/audio notifications and the minute tick must never start a
        // disabled reader, including after a previously enabled one failed.
        if !self.accessories_enabled {
            return Ok(());
        }
        self.accessory_retry_at = Instant::now() + Duration::from_secs(60);
        self.accessories = None;
        self.accessories = accessories::Monitor::new(
            self.hwnd.0 as isize,
            ACCESSORIES_CHANGED,
            self.visible && (!self.auto_hide || self.revealed),
        )
        .inspect_err(|error| eprintln!("nosih: accesorios: {error:#}"))
        .ok();
        self.refresh(gfx)
    }

    pub fn set_widgets(&mut self, gfx: &Gfx, widgets: MenuBarWidgets) -> Result<()> {
        if self.widgets == widgets {
            return Ok(());
        }
        self.widgets = widgets;
        self.refresh(gfx)
    }

    /// What clicking the clock opens; `None` for the notification center.
    pub fn set_clock_target(&mut self, target: Option<&str>) {
        if self.clock_target.as_deref() != target {
            self.clock_target = target.map(str::to_owned);
        }
    }

    /// The pointer reached the top edge or left the bar.
    pub fn hover(&mut self, gfx: &Gfx, inside: bool) -> Result<()> {
        if self.slider_drag.is_some() && !inside {
            return Ok(());
        }
        if !inside && self.panel.take().is_some() {
            self.refresh(gfx)?;
        }
        if !self.auto_hide || self.revealed == inside {
            return Ok(());
        }
        let was_hiding = self.hide_until.is_some();
        let animated = self.visible && system::animations_enabled();
        self.animation_epoch = self.animation_epoch.wrapping_add(1);
        self.exit_animation = None;
        self.revealed = inside;
        self.hide_until = (!inside && animated)
            .then(|| Instant::now() + Duration::from_millis(u64::from(HIDE_FALLBACK_MS)));
        if !animated {
            self.reset_surface()?;
        } else if inside && !was_hiding {
            self.surface
                .SetOffset(Vector3::new(0.0, -self.reveal_lift(), 0.0))?;
            self.surface.SetOpacity(0.0)?;
        }
        self.place()?;
        if animated && let Err(error) = self.animate_visibility(gfx, inside) {
            nosih_shell::log::line(&format!("animación de barra superior: {error:#}"));
            self.hide_until = None;
            self.reset_surface()?;
            self.place()?;
        }
        if inside {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: `track` is a valid structure with its size initialized.
            unsafe { TrackMouseEvent(&mut track)? };
        }
        self.refresh(gfx)
    }

    fn reveal_lift(&self) -> f32 {
        (self.bounds.bottom - self.bounds.top) as f32 * 0.3
    }

    fn animate_visibility(&mut self, gfx: &Gfx, shown: bool) -> Result<()> {
        let c = &gfx.compositor;
        let ease = if shown {
            c.CreateCubicBezierEasingFunction(Vector2::new(0.2, 0.8), Vector2::new(0.2, 1.0))?
        } else {
            c.CreateCubicBezierEasingFunction(Vector2::new(0.4, 0.0), Vector2::new(1.0, 1.0))?
        };
        let duration = TimeSpan {
            Duration: i64::from(if shown { REVEAL_MS } else { HIDE_MS }) * 10_000,
        };
        let slide = c.CreateScalarKeyFrameAnimation()?;
        slide.SetDuration(duration)?;
        slide.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
        slide.InsertKeyFrameWithEasingFunction(
            1.0,
            if shown { 0.0 } else { -self.reveal_lift() },
            &ease,
        )?;
        let fade = c.CreateScalarKeyFrameAnimation()?;
        fade.SetDuration(duration)?;
        fade.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
        fade.InsertKeyFrameWithEasingFunction(1.0, if shown { 1.0 } else { 0.0 }, &ease)?;
        let batch = (!shown)
            .then(|| c.CreateScopedBatch(CompositionBatchTypes::Animation))
            .transpose()?;
        let started = (|| -> Result<()> {
            self.surface.StartAnimation(h!("Offset.Y"), &slide)?;
            self.surface.StartAnimation(h!("Opacity"), &fade)?;
            Ok(())
        })();
        if let Some(batch) = batch {
            let window = self.hwnd.0 as isize;
            let epoch = self.animation_epoch;
            let completed = batch.Completed(&TypedEventHandler::new(move |_, _| {
                // SAFETY: Only scalar state is posted back to the UI thread.
                // A cancelled transition's already queued reply is ignored.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        HIDE_ANIMATION_DONE,
                        WPARAM(epoch),
                        LPARAM(0),
                    );
                }
                Ok(())
            }));
            // Register before ending the scope. End it even on failure so
            // unrelated later animations cannot become part of this exit.
            let ended = batch.End();
            started?;
            ended?;
            let completed = completed?;
            self.exit_animation = Some(ExitAnimation { batch, completed });
        } else {
            started?;
        }
        Ok(())
    }

    fn park_surface(&self) -> Result<()> {
        self.surface.StopAnimation(h!("Offset.Y"))?;
        self.surface.StopAnimation(h!("Opacity"))?;
        self.surface
            .SetOffset(Vector3::new(0.0, -self.reveal_lift(), 0.0))?;
        self.surface.SetOpacity(0.0)?;
        Ok(())
    }

    fn reset_surface(&self) -> Result<()> {
        self.surface.StopAnimation(h!("Offset.Y"))?;
        self.surface.StopAnimation(h!("Opacity"))?;
        self.surface.SetOffset(Vector3::new(0.0, 0.0, 0.0))?;
        self.surface.SetOpacity(1.0)?;
        Ok(())
    }

    pub fn finish_hide(&mut self, gfx: &Gfx, completed_epoch: Option<usize>) -> Result<()> {
        let mut point = POINT::default();
        // SAFETY: Read-only cursor query before completing an exit transition.
        unsafe { GetCursorPos(&mut point)? };
        self.finish_hide_at(gfx, Instant::now(), point, completed_epoch)
    }

    fn finish_hide_at(
        &mut self,
        gfx: &Gfx,
        now: Instant,
        point: POINT,
        completed_epoch: Option<usize>,
    ) -> Result<()> {
        // A queued timer from a cancelled transition cannot hide a reopened bar.
        if completed_epoch.is_some_and(|epoch| epoch != self.animation_epoch) {
            return Ok(());
        }
        let Some(deadline) = self.hide_until else {
            return Ok(());
        };
        self.pointer_left_at(gfx, point)?;
        if self.hide_until.is_none() {
            return Ok(());
        }
        if completed_epoch.is_some() || now >= deadline {
            self.hide_until = None;
            self.exit_animation = None;
        }
        self.place()
    }

    /// A leave notification can already be queued when the hidden window is
    /// shown. Check the pointer rather than hiding a newly revealed bar on it.
    pub fn check_pointer(&mut self, gfx: &Gfx) -> Result<()> {
        if self.panel.is_some() {
            // SAFETY: Read-only Escape state; checked only while a popup is open.
            if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x1B) } < 0 {
                return self.dismiss_island(gfx);
            }
        }
        let mut point = POINT::default();
        // SAFETY: Read-only cursor query in the same DPI context as `bounds`.
        unsafe { GetCursorPos(&mut point)? };
        self.pointer_left_at(gfx, point)
    }

    fn pointer_left_at(&mut self, gfx: &Gfx, point: POINT) -> Result<()> {
        let x = (point.x - self.bounds.left) as f32;
        if let Some(drag) = self.slider_drag {
            let y = (point.y - self.bounds.top) as f32;
            self.queue_slider(drag, x, y);
            // A nonactivating background window may not receive mouse-up
            // outside its visible bounds. Reuse the existing popup pointer
            // check to finish the gesture without ever stealing app focus.
            let pressed = unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
                let primary = if GetSystemMetrics(SM_SWAPBUTTON) == 0 {
                    0x01
                } else {
                    0x02
                };
                windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(primary) < 0
            };
            if pressed {
                return self.paint_volume_preview(gfx);
            }
            self.release_volume(Some((x, y)))?;
        }
        let within_bar = point.x >= self.bounds.left
            && point.x < self.bounds.right
            && point.y >= self.bounds.top
            && point.y < self.bounds.bottom;
        // Include the gap between the island and its popup so crossing it
        // does not close the popup before the pointer reaches its controls.
        let within_panel = self.panel.is_some()
            && x >= self.panel_span.left
            && x < self.panel_span.right
            && point.y >= self.bounds.bottom
            && point.y < PANEL_BOTTOM.load(Ordering::Relaxed);
        if (within_bar || within_panel) && self.hide_until.is_some() {
            self.hover(gfx, true)?;
        } else if !within_bar && !within_panel {
            self.hover(gfx, false)?;
        }
        Ok(())
    }

    /// Leaving the island sideways still stays inside the full-width bar window.
    pub fn pointer_moved(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        if let Some(drag) = self.slider_drag {
            self.queue_slider(drag, x, y);
            return self.paint_volume_preview(gfx);
        }
        let span = if y < (self.bounds.bottom - self.bounds.top) as f32 {
            self.triggers_span
        } else {
            self.panel_span
        };
        // Control Center behaves like macOS: it stays open until outside click,
        // Escape, or clicking its menu bar toggle again.
        if self.panel.is_some()
            && self.panel != Some(Island::ControlCenter)
            && (x < span.left || x >= span.right)
        {
            self.dismiss_island(gfx)?;
        }
        self.hover(gfx, true)
    }

    pub fn dismiss_island(&mut self, gfx: &Gfx) -> Result<()> {
        self.release_volume(None)?;
        if self.panel.take().is_some() {
            self.refresh(gfx)?;
        }
        Ok(())
    }

    /// The strip of the screen the bar covers for good, if any: none while it
    /// auto-hides or is hidden for a full-screen app.
    pub fn reserved_strip(&self) -> Option<RECT> {
        (self.visible && self.reserves()).then_some(self.bounds)
    }

    /// Positions the window and reserves (or frees) its strip of the screen.
    fn place(&mut self) -> Result<()> {
        self.sync_progress_watch();
        let bounds = self.bounds;
        let bar_height = bounds.bottom - bounds.top;
        if bar_height <= 0 {
            return Ok(());
        }
        let hidden = self.auto_hide && !self.revealed;
        if !self.visible || !self.auto_hide {
            self.hide_until = None;
            self.exit_animation = None;
        }
        let on_screen = self.visible && (!hidden || self.hide_until.is_some());
        if let Some(monitor) = &self.accessories {
            monitor.set_visible(on_screen && !hidden);
        }
        self.sync_monitor(self.visible && !hidden);
        if !on_screen {
            // Keep the hidden tree transparent. Restoring opacity here races
            // HWND hiding and flashes its last opaque frame on the next show.
            self.park_surface()?;
        } else if !self.auto_hide {
            self.reset_surface()?;
        }
        // An auto-hiding or intelligent bar leaves the whole screen to the
        // windows.
        let strip = if self.reserves() { bar_height } else { 0 };
        if let Some(appbar) = &mut self.appbar {
            appbar.reserve(Edge::Top, bounds, strip);
        }
        // Hidden, the bar keeps no window on screen, not even a thin strip:
        // a strip along the top edge took the clicks meant for the close
        // button of maximized windows and for browser tabs. The edge watcher
        // watches the pointer instead.
        let show = if on_screen {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        // SAFETY: Our own live window.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                self.visual_height,
                SWP_NOACTIVATE | show,
            )?;
            if on_screen && (self.auto_hide || self.panel.is_some()) {
                // TrackMouseEvent may report a leave immediately after showing
                // a window. Check real geometry while revealed, including if
                // no further WM_MOUSEMOVE reaches this window.
                SetTimer(Some(self.hwnd), POINTER_TIMER, POINTER_CHECK_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), POINTER_TIMER);
            }
            let _ = KillTimer(Some(self.hwnd), HIDE_ANIMATION_TIMER);
            if let Some(deadline) = self.hide_until {
                let wait = deadline.saturating_duration_since(Instant::now());
                SetTimer(
                    Some(self.hwnd),
                    HIDE_ANIMATION_TIMER,
                    (wait.as_millis() as u32).max(1),
                    None,
                );
            }
        }
        edge_watch::set_menu_bar((self.visible && hidden).then_some(bounds));
        self.sync_outside_click_hook()
    }

    /// The edge watcher has verified the pointer's dwell on this bar.
    pub fn edge_rested(&mut self, gfx: &Gfx, point: POINT) -> Result<()> {
        if self.visible && self.auto_hide && !self.revealed {
            if nosih_shell::tracker::caption_button_at(point, self.bounds) {
                edge_watch::retry_on_move(edge_watch::Target::MenuBar);
            } else {
                self.hover(gfx, true)?;
            }
        }
        Ok(())
    }

    fn sync_outside_click_hook(&mut self) -> Result<()> {
        let needed =
            self.panel_span.width() > 0.0 && self.visible && (!self.auto_hide || self.revealed);
        if needed && self.outside_click_hook.is_none() {
            // SAFETY: This process owns the hook procedure and installs the hook
            // only while the panel is visible.
            let module = unsafe { GetModuleHandleW(None)? };
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(outside_click_proc),
                    Some(module.into()),
                    0,
                )?
            };
            ISLAND_HOOK_WINDOW.store(self.hwnd.0 as isize, Ordering::Relaxed);
            self.outside_click_hook = Some(hook);
        } else if !needed && let Some(hook) = self.outside_click_hook {
            // SAFETY: This hook was installed by this MenuBar.
            unsafe { UnhookWindowsHookEx(hook)? };
            self.outside_click_hook = None;
            ISLAND_HOOK_WINDOW.store(0, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Privacy activity or the user's taskbar preferences changed.
    pub fn registry_changed(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(watch) = &mut self.privacy_watch {
            watch.arm();
        }
        if let Some(watch) = &mut self.settings_watch {
            watch.arm();
        }
        if let Some(watch) = &mut self.search_watch {
            watch.arm();
        }
        // Only reread registry-backed indicators. This event must not trigger
        // network/audio queries, provider scans or accessory discovery.
        self.status.buttons = status::taskbar_buttons();
        self.status.battery_percentage = status::battery_percentage_enabled();
        self.status.privacy = status::privacy();
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.status = self.status;
        self.apply_look(gfx, look)
    }

    pub fn tick(&mut self, gfx: &Gfx) -> Result<()> {
        self.update_status();
        self.watch_network();
        let audio_changed = self
            .audio
            .as_ref()
            .is_some_and(AudioWatch::take_default_changed);
        let retry_due = Instant::now() >= self.accessory_retry_at
            && self.accessories.as_ref().is_none_or(|monitor| {
                monitor.snapshot().state == nosih_shell::accessories::State::Unavailable
            });
        if self.accessories_enabled && (audio_changed || retry_due) {
            self.restart_accessories(gfx)?;
        }
        self.schedule_next_minute();
        self.refresh(gfx)
    }

    /// Ordinary audio notifications do not requery network, registry, providers
    /// or clock. A default-device change still restarts enabled accessories.
    pub fn audio_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let volume = self.audio.as_mut().and_then(AudioWatch::volume);
        if self
            .audio
            .as_ref()
            .is_some_and(AudioWatch::take_default_changed)
            && self.accessories_enabled
        {
            self.status.volume = volume;
            self.restart_accessories(gfx)?;
        }
        self.apply_volume_reading(gfx, volume)
    }

    fn apply_volume_reading(&mut self, gfx: &Gfx, volume: Option<Volume>) -> Result<()> {
        self.status.volume = volume;
        if let Some(look) = &self.look
            && same_volume_indicator(look.status.volume, volume)
        {
            if let Some(volume) = volume {
                let target = self.volume_target.unwrap_or(volume.percent);
                if let Some(controls) = &mut self.volume_controls {
                    controls.paint(gfx, target)?;
                }
                if let Some(controls) = &mut self.cc_volume_controls {
                    controls.paint(gfx, target, volume.muted)?;
                }
            }
            if let Some(look) = &mut self.look {
                look.status.volume = volume;
            }
            return Ok(());
        }
        // Availability, mute and speaker-glyph changes affect layout, but do
        // not justify a new provider/process or system-status scan either.
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.status.volume = volume;
        self.apply_look(gfx, look)
    }

    pub fn control_center_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let cc_state = self
            .control_center
            .as_ref()
            .map(control_center::Controller::snapshot)
            .unwrap_or_default();
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        let appearance = appearance::menu_bar();
        if look.control_center == cc_state
            && look.material == appearance.material
            && look.light == appearance.light
            && look.glass == appearance.glass
        {
            return Ok(());
        }
        look.material = appearance.material;
        look.light = appearance.light;
        look.glass = appearance.glass;
        look.control_center = cc_state;
        self.apply_look(gfx, look)
    }

    pub fn bluetooth_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let on = self
            .bluetooth
            .as_ref()
            .is_some_and(bluetooth::Monitor::is_on);
        self.status.bluetooth_on = on;
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        if look.status.bluetooth_on == on {
            return Ok(());
        }
        look.status.bluetooth_on = on;
        self.apply_look(gfx, look)
    }

    fn update_status(&mut self) {
        self.status = Status {
            bluetooth_on: self
                .bluetooth
                .as_ref()
                .is_some_and(bluetooth::Monitor::is_on),
            network: status::network(),
            volume: self.audio.as_mut().and_then(AudioWatch::volume),
            buttons: status::taskbar_buttons(),
            privacy: status::privacy(),
            battery_percentage: status::battery_percentage_enabled(),
        };
    }

    /// One wake-up per minute, aligned to the minute, instead of polling.
    fn schedule_next_minute(&self) {
        // SAFETY: Read-only query, then a timer on our own window (setting an
        // existing timer id replaces it).
        unsafe {
            let now: SYSTEMTIME = GetLocalTime();
            let elapsed = u32::from(now.wSecond) * 1000 + u32::from(now.wMilliseconds);
            SetTimer(
                Some(self.hwnd),
                CLOCK_TIMER,
                60_000 - elapsed.min(59_900) + 50,
                None,
            );
        }
    }

    pub fn click(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        if let Some(hit) = self.island_hits.iter().find(|hit| hit.at(x, y)) {
            match hit.action {
                IslandAction::Toggle(island) => {
                    self.panel = toggle_panel(self.panel, island);
                    if self.panel.is_some() {
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: self.hwnd,
                            dwHoverTime: 0,
                        };
                        // SAFETY: `track` describes our live window.
                        unsafe { TrackMouseEvent(&mut track)? };
                    }
                }
                IslandAction::Refresh => {
                    if let Some(usage) = &mut self.usage {
                        usage.refresh(true);
                    }
                }
                IslandAction::Media(control) => {
                    if let Some(media) = &self.media
                        && let Some(track) = self.look.as_ref().and_then(|look| look.track.as_ref())
                    {
                        media.send(track, control);
                    }
                    return Ok(());
                }
                IslandAction::Volume(_)
                | IslandAction::HorizontalVolume(_)
                | IslandAction::HorizontalBrightness(_) => return Ok(()), // Handled on press/drag.
                IslandAction::Mute => {
                    if let Some(audio) = &mut self.audio {
                        audio.toggle_mute()?;
                    }
                    return self.audio_changed(gfx);
                }
                IslandAction::ScrollUsage(direction) => {
                    self.scroll_usage(f32::from(direction) * 72.0)?;
                    return Ok(());
                }
                IslandAction::ControlCenter(action) => {
                    match action {
                        ControlCenterAction::ToggleWifi => {
                            if let Some(cc) = &self.control_center {
                                cc.toggle_wifi();
                            }
                        }
                        ControlCenterAction::ToggleBluetooth => {
                            if let Some(cc) = &self.control_center {
                                cc.toggle_bluetooth();
                            }
                        }
                        ControlCenterAction::ToggleDoNotDisturb => {
                            if let Some(cc) = &self.control_center {
                                cc.toggle_do_not_disturb();
                            }
                        }
                        ControlCenterAction::ToggleDarkMode => {
                            if let Some(cc) = &self.control_center {
                                cc.toggle_dark_mode();
                            }
                        }
                        ControlCenterAction::ScreenSnip => {
                            self.dismiss_island(gfx)?;
                            shell_cc::open_screen_snip();
                        }
                        ControlCenterAction::LockScreen => {
                            self.dismiss_island(gfx)?;
                            shell_cc::lock_workstation();
                        }
                        ControlCenterAction::OpenPower => {
                            self.dismiss_island(gfx)?;
                            launcher::open_power_settings();
                        }
                        ControlCenterAction::OpenNetworkList => {
                            self.dismiss_island(gfx)?;
                            launcher::open_network_list();
                        }
                        ControlCenterAction::OpenSettings => {
                            self.dismiss_island(gfx)?;
                            shell_cc::open_nosih_settings();
                        }
                    }
                    return Ok(());
                }
            }
            return self.refresh(gfx);
        }
        if y >= (self.bounds.bottom - self.bounds.top) as f32 {
            return Ok(());
        }
        let was_control_center = self.panel == Some(Island::ControlCenter);
        self.dismiss_island(gfx)?;
        let hit = self
            .zones
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right);
        // Where a flyout opened from this item should hang: under its centre.
        let anchor = hit.map_or((0, 0), |(left, right, _)| {
            (
                self.bounds.left + ((left + right) / 2.0) as i32,
                self.bounds.bottom,
            )
        });
        match hit.map(|(_, _, action)| *action) {
            Some(Action::AccessoryIndicator) => {}
            Some(Action::QuickSettings | Action::Sound) => {
                if !was_control_center {
                    self.panel = Some(Island::ControlCenter);
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: `track` describes our live window.
                    unsafe { TrackMouseEvent(&mut track)? };
                    if let Some(cc) = &self.control_center {
                        cc.refresh();
                    }
                    return self.refresh(gfx);
                }
            }
            Some(Action::Notifications) => launcher::open_notifications(),
            Some(Action::Clock) => match &self.clock_target {
                // A target that no longer opens (an uninstalled app) falls
                // back to what the clock always opened.
                Some(target) if launcher::open(target).is_ok() => {}
                _ => launcher::open_notifications(),
            },
            Some(Action::Power) => launcher::open_power_settings(),
            Some(Action::Network) => launcher::open_network_list(),
            // Handled alongside Action::QuickSettings above.
            Some(Action::Search) => launcher::open_search(),
            Some(Action::TaskView) => launcher::open_task_view(),
            Some(Action::Widgets) => launcher::open_widgets(),
            Some(Action::Bluetooth) => launcher::open_bluetooth(),
            Some(Action::SystemMonitor) => {
                let _ = launcher::open("taskmgr.exe");
            }
            Some(Action::HiddenIcons) => launcher::toggle_hidden_icons(anchor),
            Some(Action::InputLanguage) => {} // Opened outside the app borrow.
            // Windows' own pages list which apps use each device.
            Some(Action::Microphone) => {
                let _ = launcher::open("ms-settings:privacy-microphone");
            }
            Some(Action::Camera) => {
                let _ = launcher::open("ms-settings:privacy-webcam");
            }
            None => {}
        }
        Ok(())
    }

    pub fn input_switcher_anchor(&self, x: f32, y: f32) -> Option<POINT> {
        if y < 0.0 || y >= (self.bounds.bottom - self.bounds.top) as f32 {
            return None;
        }
        self.zones
            .iter()
            .find(|(left, right, action)| {
                x >= *left && x < *right && matches!(action, Action::InputLanguage)
            })
            .map(|(left, right, _)| POINT {
                x: self.bounds.left + ((left + right) * 0.5) as i32,
                y: self.bounds.bottom + 6,
            })
    }

    /// Renew the host sample without rebuilding an open island or its controls.
    pub fn refresh_backdrops(&self, gfx: &Gfx) -> Result<()> {
        gfx.refresh_backdrops(&self.surface)?;
        Ok(())
    }

    /// Redraws the bar if anything it shows has changed.
    pub fn refresh(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(monitor) = system::primary_monitor() else {
            return Ok(());
        };
        let active_providers = if self.usage.is_some() {
            usage::active_providers()
        } else {
            ActiveProviders::default()
        };
        let track = self.media.as_ref().and_then(media::Watch::snapshot);
        self.panel = available_panel(self.panel, track.is_some(), active_providers.any());
        if !active_providers.any() {
            if let Some(usage) = &mut self.usage {
                usage.mark_inactive();
            }
        } else if self.visible
            && (!self.auto_hide || self.revealed)
            && let Some(usage) = &mut self.usage
        {
            usage.refresh(false);
        }
        let appearance = appearance::menu_bar();
        let look = Look {
            accessories: self.accessories.as_ref().map_or([None, None], |monitor| {
                accessory_readings(&monitor.snapshot())
            }),
            monitor: self.monitor_lines(),
            language: strings::language(),
            screen: monitor.screen,
            scale: monitor.scale,
            material: appearance.material,
            light: appearance.light,
            glass: appearance.glass,
            app_name: self.app_name.clone(),
            clock: system::clock_text(),
            input: system::input_language(),
            battery: system::battery(),
            status: self.status,
            widgets: self.widgets,
            usage: self
                .usage
                .as_ref()
                .map_or_else(Snapshot::default, UsageMonitor::snapshot),
            usage_enabled: self.usage.is_some() && active_providers.any(),
            active_providers,
            panel: self.panel,
            usage_refreshing: self.usage.as_ref().is_some_and(UsageMonitor::is_refreshing),
            track,
            control_center: {
                let mut state = self
                    .control_center
                    .as_ref()
                    .map(control_center::Controller::snapshot)
                    .unwrap_or_default();
                state.dark_mode = shell_cc::is_dark_mode();
                state.do_not_disturb = shell_cc::do_not_disturb();
                state
            },
        };
        self.apply_look(gfx, look)
    }

    /// Samples only while the monitor is on and the bar shows; a hidden or
    /// auto-hidden bar costs nothing (issue #64).
    fn sync_monitor(&mut self, shown: bool) {
        let wanted = self.widgets.system_monitor && shown;
        if wanted == self.monitor.is_some() {
            return;
        }
        if wanted {
            let mut sampler = nosih_shell::monitor::Sampler::default();
            // Memory reads at once; CPU and network keep their last values
            // until the first sample, half a second later.
            self.monitor_reading.memory = sampler.sample().memory;
            self.monitor = Some(sampler);
            // SAFETY: A timer on our own window.
            unsafe {
                SetTimer(Some(self.hwnd), MONITOR_TIMER, MONITOR_FIRST_MS, None);
            }
        } else {
            self.monitor = None;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), MONITOR_TIMER);
            }
        }
    }

    /// A new sample of the system monitor; redraws if what it shows changed.
    pub fn monitor_tick(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(sampler) = &mut self.monitor else {
            return Ok(());
        };
        self.monitor_reading = sampler.sample();
        // SAFETY: A timer on our own window; after the first sample, the
        // regular interval.
        unsafe {
            SetTimer(Some(self.hwnd), MONITOR_TIMER, MONITOR_EVERY_MS, None);
        }
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.monitor = self.monitor_lines();
        self.apply_look(gfx, look)
    }

    /// CPU and memory, then upload and download, as the bar shows them.
    fn monitor_lines(&self) -> Option<[String; 4]> {
        if !self.widgets.system_monitor {
            return None;
        }
        let reading = self.monitor_reading;
        let comma = matches!(
            strings::language(),
            Language::Es | Language::Fr | Language::Pt
        );
        let share = |label: &str, value: Option<u8>| {
            value.map_or_else(|| format!("{label} —"), |value| format!("{label} {value}%"))
        };
        let speed = |arrow: char, value: Option<u64>| {
            value.map_or_else(
                || format!("{arrow} —"),
                |value| {
                    format!(
                        "{arrow} {}",
                        nosih_shell::monitor::format_rate(value, comma)
                    )
                },
            )
        };
        Some([
            share("CPU", reading.cpu),
            share("RAM", reading.memory),
            speed('↑', reading.up),
            speed('↓', reading.down),
        ])
    }

    /// Track changes only need to update the contents of the island.
    pub fn media_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        let track = self.media.as_ref().and_then(media::Watch::snapshot);
        let timeline_only = look
            .track
            .as_ref()
            .zip(track.as_ref())
            .is_some_and(|(old, new)| {
                old.same_content(new) && old.timeline.is_some() == new.timeline.is_some()
            });
        look.track = track;
        if timeline_only {
            self.look = Some(look);
            return self.progress_tick(gfx);
        }
        self.panel = available_panel(self.panel, look.track.is_some(), look.usage_enabled);
        look.panel = self.panel;
        self.apply_look(gfx, look)
    }

    fn apply_look(&mut self, gfx: &Gfx, mut look: Look) -> Result<()> {
        look.active_providers = look.usage.visible_providers(look.active_providers);
        look.usage_enabled &= look.active_providers.any();
        self.panel = available_panel(look.panel, look.track.is_some(), look.usage_enabled);
        look.panel = self.panel;
        if look.panel != Some(Island::Usage) {
            self.usage_scroll = 0.0;
        }
        let moved = self
            .look
            .as_ref()
            .is_some_and(|old| old.screen != look.screen || old.scale != look.scale);
        if !matches!(look.panel, Some(Island::Music | Island::ControlCenter)) || moved {
            self.release_volume(None)?;
        }
        if self.look.as_ref() == Some(&look) {
            return Ok(());
        }
        let moment = self
            .look
            .as_ref()
            .and_then(|old| usage_moment(&old.usage, &look.usage, &look.active_providers));
        self.build(gfx, &look)?;
        // A crowded bar can have no room even for a compact trigger. Do not
        // keep a ghost popup that reappears by itself when space returns.
        if look.panel.is_some() && self.panel_span.width() <= 0.0 {
            self.panel = None;
            look.panel = None;
            self.build(gfx, &look)?;
        }
        self.look = Some(look);
        self.sync_progress_watch();
        if let Some((provider, act)) = moment
            && let Some(&(_, x)) = self.usage_anchors.iter().find(|(p, _)| *p == provider)
        {
            self.cue = Some((act, x));
        }
        Ok(())
    }

    /// A moment worth the crab, found by the last update: the act and the
    /// screen x of the provider in the island. Taken once.
    /// The moment waiting for the mascot, with the point under the bar
    /// where it happens, or `None` when the bar does not show now.
    pub fn take_cue(&mut self) -> Option<(mascot::Act, Option<POINT>)> {
        let (act, x) = self.cue.take()?;
        Some((
            act,
            self.shown().then_some(POINT {
                x,
                y: self.bounds.bottom,
            }),
        ))
    }

    /// Whether the bar is on screen now.
    fn shown(&self) -> bool {
        self.visible && (!self.auto_hide || self.revealed)
    }

    /// The point under a status icon, while the bar shows: its centre and
    /// the bar's bottom edge, in screen pixels.
    fn anchor(&self, item: Action) -> Option<POINT> {
        if !self.shown() {
            return None;
        }
        let (left, right, _) = self.zones.iter().find(|(_, _, action)| *action == item)?;
        Some(POINT {
            x: self.bounds.left + ((left + right) / 2.0) as i32,
            y: self.bounds.bottom,
        })
    }

    /// Under the battery icon, for the battery's moments.
    pub fn battery_anchor(&self) -> Option<POINT> {
        self.anchor(Action::Power)
    }

    /// The network lost for a few seconds, or back after that: a moment
    /// under the network icon (see `take_cue`). Brief drops say nothing;
    /// the first reading is only the starting point.
    fn watch_network(&mut self) {
        let online = !matches!(self.status.network, Network::Offline);
        let before = self.online.replace(online);
        if online {
            if self.offline_told {
                self.network_cue(mascot::Act::Online);
            }
            self.offline_since = None;
            self.offline_told = false;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), NETWORK_TIMER);
            }
            return;
        }
        if before == Some(true) {
            self.offline_since = Some(Instant::now());
            // Fires a tick, which reads the network again.
            // SAFETY: A timer on our own window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    NETWORK_TIMER,
                    OFFLINE_GRACE.as_millis() as u32,
                    None,
                );
            }
        } else if !self.offline_told
            && self
                .offline_since
                .is_some_and(|since| since.elapsed() + Duration::from_millis(250) >= OFFLINE_GRACE)
        {
            self.offline_told = true;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), NETWORK_TIMER);
            }
            self.network_cue(mascot::Act::Offline);
        }
    }

    fn network_cue(&mut self, act: mascot::Act) {
        let x = self
            .zones
            .iter()
            .find(|(_, _, action)| *action == Action::Network)
            .map_or(
                self.bounds.left + (self.bounds.right - self.bounds.left) / 2,
                |(left, right, _)| self.bounds.left + ((left + right) / 2.0) as i32,
            );
        self.cue = Some((act, x));
    }

    fn build(&mut self, gfx: &Gfx, look: &Look) -> Result<()> {
        let c = &gfx.compositor;
        let scale = look.scale;
        let px = |logical: f32| (logical * scale).round();
        let screen = look.screen;
        let (width, height) = ((screen.right - screen.left) as f32, px(HEIGHT));
        let panel_bottom = height + px(PANEL_GAP) + px(look.panel_height());
        let visual_height = if look.panel.is_some() {
            panel_bottom + px(PANEL_SHADOW_MARGIN)
        } else {
            height
        };
        let ink = if look.light { Ink::Dark } else { Ink::Light };

        let root = c.CreateContainerVisual()?;
        root.SetSize(Vector2 {
            X: width,
            Y: visual_height,
        })?;
        let layers = root.Children()?;
        // Without transparency the bar is solid, as macOS draws it with
        // "Reduce transparency".
        let fills = Theme::for_bar(appearance::Look {
            material: look.material,
            light: look.light,
            glass: look.glass,
        })
        .fills(gfx)?;
        for brush in &fills {
            let fill = c.CreateSpriteVisual()?;
            fill.SetSize(Vector2 {
                X: width,
                Y: height,
            })?;
            fill.SetBrush(brush)?;
            layers.InsertAtTop(&fill)?;
        }

        let place = |layers: &windows::UI::Composition::VisualCollection,
                     item: &LabelBrush,
                     left: f32|
         -> Result<SpriteVisual> {
            let visual = c.CreateSpriteVisual()?;
            visual.SetSize(Vector2 {
                X: item.width,
                Y: item.height,
            })?;
            visual.SetOffset(Vector3 {
                X: left.round(),
                Y: ((height - item.height) / 2.0).round(),
                Z: 0.0,
            })?;
            visual.SetBrush(&item.brush)?;
            layers.InsertAtTop(&visual)?;
            Ok(visual)
        };

        self.zones.clear();
        let mut left_end = px(SIDE_MARGIN);
        if !look.app_name.is_empty() {
            let name = gfx.text_brush_fitted(
                &look.app_name,
                scale,
                13.0,
                true,
                ink,
                px(220.0).min(width * 0.22),
            )?;
            place(&layers, &name, px(SIDE_MARGIN))?;
            left_end += name.width;
        }

        // Right side, from the edge inward.
        let slop = px(HIT_SLOP);
        let mut right = width - px(SIDE_MARGIN);
        let status = look.status;
        let symbol = |layers: &[(char, f32)]| gfx.glyph_brush(layers, scale, ink);
        let mut items: Vec<(LabelBrush, Action)> = Vec::new();
        // The notification bell, when chosen, takes the very end of the bar.
        if look.widgets.notifications {
            items.push((
                symbol(&[(glyph::NOTIFICATIONS, 1.0)])?,
                Action::Notifications,
            ));
        }
        items.push((
            gfx.text_brush(&look.clock, scale, false, ink)?,
            Action::Clock,
        ));
        items.push((gfx.control_center_brush(scale, ink)?, Action::QuickSettings));
        // Privacy indicators, as the Windows tray shows them: something is
        // using the microphone or camera right now.
        let privacy = status.privacy;
        for (on, glyph, action) in [
            (privacy.microphone, glyph::MICROPHONE, Action::Microphone),
            (privacy.camera, glyph::CAMERA, Action::Camera),
        ] {
            if on {
                items.push((symbol(&[(glyph, 1.0)])?, action));
            }
        }
        if status.buttons.search {
            items.push((symbol(&[(glyph::SEARCH, 1.0)])?, Action::Search));
        }
        if status.bluetooth_on {
            items.push((symbol(&[(glyph::BLUETOOTH, 1.0)])?, Action::Bluetooth));
        }
        // The control center already opens all of quick settings: the network
        // icon goes straight to the list of networks (issue #55).
        match status.network {
            Network::Wifi { bars, has_internet } => {
                items.push((
                    gfx.wifi_brush(bars, has_internet, scale, ink)?,
                    Action::Network,
                ));
            }
            Network::Ethernet => {
                items.push((symbol(&[(glyph::ETHERNET, 1.0)])?, Action::Network));
            }
            Network::Offline => {
                items.push((symbol(&[(glyph::OFFLINE, 1.0)])?, Action::Network));
            }
        }
        if let Some(volume) = status.volume {
            let speaker = if volume.muted {
                glyph::MUTE
            } else {
                glyph::VOLUME[usize::from(volume.level.min(3))]
            };
            items.push((symbol(&[(speaker, 1.0)])?, Action::Sound));
        }
        let mut battery_label = None;
        if let Some(battery) = look.battery {
            if status.battery_percentage {
                battery_label = Some((
                    items.len(),
                    gfx.text_brush(&format!("{}%", battery.percent), scale, false, ink)?,
                ));
            }
            items.push((
                gfx.battery_brush(battery.percent, battery.charging, battery.saver, scale, ink)?,
                Action::Power,
            ));
        }
        let mut accessory_labels = Vec::new();
        for device in look.accessories.into_iter().flatten().rev() {
            let value = device
                .percent
                .map_or_else(|| "—".to_owned(), |percent| format!("{percent}%"));
            accessory_labels.push((items.len(), gfx.text_brush(&value, scale, false, ink)?));
            let icon = match device.kind {
                AccessoryKind::Mouse => glyph::MOUSE,
                AccessoryKind::Headset => glyph::HEADSET,
            };
            items.push((symbol(&[(icon, 1.0)])?, Action::AccessoryIndicator));
        }
        if let Some(language) = &look.input {
            items.push((
                gfx.text_brush(language, scale, false, ink)?,
                Action::InputLanguage,
            ));
        }
        // The notification area (antivirus, OneDrive, apps in the background)
        // lives only in the hidden taskbar; this arrow opens its flyout.
        items.push((symbol(&[(glyph::HIDDEN_ICONS, 1.0)])?, Action::HiddenIcons));
        if status.buttons.task_view {
            items.push((symbol(&[(glyph::TASK_VIEW, 1.0)])?, Action::TaskView));
        }
        if status.buttons.widgets {
            items.push((symbol(&[(glyph::WIDGETS, 1.0)])?, Action::Widgets));
        }
        for (index, (item, action)) in items.iter().enumerate() {
            let zone_right = right;
            if let Some((_, label)) = battery_label
                .iter()
                .chain(accessory_labels.iter())
                .find(|(item_index, _)| *item_index == index)
            {
                right -= label.width;
                place(&layers, label, right)?;
                right -= px(6.0);
            }
            let left = right - item.width;
            place(&layers, item, left)?;
            // The battery icon and its percentage form one control.
            // Accessory readings are passive status indicators, not controls.
            if *action != Action::AccessoryIndicator {
                self.zones.push((left - slop, zone_right + slop, *action));
            }
            right = left - px(ITEM_GAP);
        }

        // The system monitor (issue #64), left of the status icons: two
        // small stacked columns, CPU over memory, then upload over download.
        if let Some(lines) = &look.monitor {
            let brushes = lines
                .iter()
                .map(|line| gfx.text_brush_sized(line, scale, MONITOR_FONT, false, ink))
                .collect::<Result<Vec<_>>>()?;
            if self.monitor_widths.0 != scale {
                self.monitor_widths = (scale, [0.0; 2]);
            }
            for (column, pair) in brushes.chunks(2).enumerate() {
                let widest = pair.iter().map(|brush| brush.width).fold(0.0, f32::max);
                self.monitor_widths.1[column] = self.monitor_widths.1[column].max(widest);
            }
            let widths = self.monitor_widths.1;
            let zone_right = right;
            let mut left = right;
            for column in [1, 0] {
                left -= widths[column];
                for (row, brush) in brushes[column * 2..column * 2 + 2].iter().enumerate() {
                    let top = if row == 0 {
                        height / 2.0 - brush.height + px(MONITOR_TUCK)
                    } else {
                        height / 2.0 - px(MONITOR_TUCK)
                    };
                    let visual = c.CreateSpriteVisual()?;
                    visual.SetSize(Vector2 {
                        X: brush.width,
                        Y: brush.height,
                    })?;
                    visual.SetOffset(Vector3 {
                        X: left.round(),
                        Y: top.round(),
                        Z: 0.0,
                    })?;
                    visual.SetBrush(&brush.brush)?;
                    layers.InsertAtTop(&visual)?;
                }
                if column == 1 {
                    left -= px(MONITOR_COLUMN_GAP);
                }
            }
            self.zones
                .push((left - slop, zone_right + slop, Action::SystemMonitor));
            right = left - px(ITEM_GAP);
        }

        self.island_hits.clear();
        self.volume_controls = None;
        self.cc_volume_controls = None;
        self.cc_brightness_controls = None;
        self.progress_controls = None;
        self.usage_viewport = None;
        self.triggers_span = Span::default();
        self.panel_span = Span::default();
        self.draw_island(gfx, look, &layers, width, height, left_end, right)?;
        if look.panel == Some(Island::ControlCenter) {
            self.draw_control_center(gfx, look, &layers, width, height)?;
            self.triggers_span = Span {
                left: 0.0,
                right: width,
            };
        }
        if let Some(drag) = self.slider_drag {
            let still_present = self.island_hits.iter().any(|hit| match (drag, hit.action) {
                (SliderDrag::VerticalVolume(a), IslandAction::Volume(b)) => a == b,
                (SliderDrag::HorizontalVolume(a), IslandAction::HorizontalVolume(b)) => a == b,
                (SliderDrag::HorizontalBrightness(a), IslandAction::HorizontalBrightness(b)) => {
                    a == b
                }
                _ => false,
            });
            if !still_present {
                self.release_volume(None)?;
            }
        }
        self.surface.SetSize(Vector2 {
            X: width,
            Y: visual_height,
        })?;
        let contents = self.surface.Children()?;
        contents.RemoveAll()?;
        contents.InsertAtTop(&root)?;
        self.visual_height = if self.panel_span.width() > 0.0 {
            visual_height
        } else {
            height
        } as i32;
        self.bounds = RECT {
            bottom: screen.top + height as i32,
            ..screen
        };
        TRIGGERS_LEFT.store(
            screen.left + self.triggers_span.left as i32,
            Ordering::Relaxed,
        );
        TRIGGERS_RIGHT.store(
            screen.left + self.triggers_span.right as i32,
            Ordering::Relaxed,
        );
        PANEL_LEFT.store(screen.left + self.panel_span.left as i32, Ordering::Relaxed);
        PANEL_RIGHT.store(
            screen.left + self.panel_span.right as i32,
            Ordering::Relaxed,
        );
        BAR_TOP.store(screen.top, Ordering::Relaxed);
        BAR_BOTTOM.store(screen.top + height as i32, Ordering::Relaxed);
        let clickable_height = if self.panel_span.width() > 0.0 {
            panel_bottom
        } else {
            height
        };
        PANEL_BOTTOM.store(screen.top + clickable_height as i32, Ordering::Relaxed);
        self.place()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The island is laid out between the measured bar items"
    )]
    fn draw_island(
        &mut self,
        gfx: &Gfx,
        look: &Look,
        layers: &VisualCollection,
        width: f32,
        bar_height: f32,
        left_end: f32,
        right_start: f32,
    ) -> Result<()> {
        let px = |logical: f32| (logical * look.scale).round();
        let style = look.widgets.island_style;
        let cover = if let Some(track) = &look.track {
            let cached =
                self.cover
                    .as_ref()
                    .is_some_and(|(source, _)| match (source, &track.artwork) {
                        (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                        (None, None) => true,
                        _ => false,
                    });
            if !cached {
                self.cover = media::artwork(track)
                    .map(|image| {
                        let surface = gfx.image_surface(image)?;
                        Ok::<_, anyhow::Error>((
                            track.artwork.clone(),
                            gfx.compositor.CreateSurfaceBrushWithSurface(&surface)?,
                        ))
                    })
                    .transpose()?;
            }
            self.cover.as_ref().map(|(_, brush)| brush.clone())
        } else {
            self.cover = None;
            None
        };
        let prepared_music = if style == IslandStyle::Bubble
            && let Some(track) = &look.track
        {
            Some(prepare_bubble_music(gfx, look, track)?)
        } else {
            None
        };
        let triggers = match style {
            IslandStyle::Notch => trigger_layout(
                Span {
                    left: left_end + px(8.0),
                    right: right_start - px(8.0),
                },
                width / 2.0,
                look.scale,
                look.track.is_some(),
                look.active_providers.count(),
            ),
            IslandStyle::Bubble => bubble_trigger_layout(
                Span {
                    left: left_end + px(8.0),
                    right: right_start,
                },
                look.scale,
                prepared_music.as_ref().map(|music| music.width),
                look.active_providers.count(),
            ),
        };
        let [(_, music), (_, usage)] = triggers;
        let (Some(first), Some(last)) = (music.or(usage), usage.or(music)) else {
            return Ok(());
        };
        self.triggers_span = Span {
            left: first.left,
            right: last.right,
        };
        let geometry = IslandGeometry::new(self.triggers_span, 0.0, look);
        let (compact, compact_height, divider_inset) = match style {
            IslandStyle::Notch => (
                island_surface(gfx, look, layers, geometry, bar_height)?,
                bar_height,
                px(7.0),
            ),
            IslandStyle::Bubble => {
                let (surface, pill_height) =
                    bubble_surface(gfx, look, layers, geometry, bar_height)?;
                (surface, pill_height, px(5.0))
            }
        };
        let divider = music
            .zip(usage)
            .map(|(music, usage)| ((music.right + usage.left) / 2.0).round());
        let c = &gfx.compositor;
        if let Some(x) = divider {
            add_visual(
                c,
                &compact,
                &c.CreateColorBrushWithColor(geometry.border())?.cast()?,
                x - self.triggers_span.left,
                divider_inset,
                px(1.0).max(1.0),
                (compact_height - 2.0 * divider_inset).max(1.0),
            )?;
        }
        let providers = island_providers(look);
        for (kind, span) in triggers {
            let Some(span) = span else {
                continue;
            };
            let geometry = IslandGeometry::new(span, 0.0, look);
            let segment = c.CreateContainerVisual()?;
            segment.SetSize(Vector2::new(geometry.width, compact_height))?;
            segment.SetOffset(Vector3::new(span.left - self.triggers_span.left, 0.0, 0.0))?;
            let content = segment.Children()?;
            compact.InsertAtTop(&segment)?;
            match kind {
                Island::Music => {
                    if let Some(track) = &look.track {
                        match style {
                            IslandStyle::Notch => draw_compact_player(
                                gfx,
                                &content,
                                geometry,
                                geometry.width,
                                compact_height,
                                track,
                                cover.as_ref(),
                            )?,
                            IslandStyle::Bubble => {
                                if let Some(prepared) = &prepared_music {
                                    draw_bubble_player(
                                        gfx,
                                        &content,
                                        geometry,
                                        compact_height,
                                        track,
                                        prepared,
                                        cover.as_ref(),
                                    )?;
                                }
                            }
                        }
                    }
                }
                Island::Usage => {
                    let anchors = match style {
                        IslandStyle::Notch => {
                            draw_compact_usage(gfx, &content, geometry, compact_height, &providers)?
                        }
                        IslandStyle::Bubble => {
                            draw_bubble_usage(gfx, &content, geometry, compact_height, &providers)?
                        }
                    };
                    self.usage_anchors = anchors
                        .into_iter()
                        .map(|(provider, x)| (provider, look.screen.left + (span.left + x) as i32))
                        .collect();
                }
                Island::ControlCenter => {}
            }
            let (left, right) = match kind {
                Island::Music => (span.left, divider.unwrap_or(span.right)),
                Island::Usage => (divider.unwrap_or(span.left), span.right),
                Island::ControlCenter => (span.left, span.right),
            };
            self.island_hits.push(IslandHit {
                bounds: (left, 0.0, right, bar_height),
                action: IslandAction::Toggle(kind),
            });
            if look.panel != Some(kind) {
                continue;
            }
            let desired = px(match kind {
                Island::Music => 360.0,
                Island::Usage => ISLAND_EXPANDED_WIDTH,
                Island::ControlCenter => CONTROL_CENTER_WIDTH,
            });
            self.panel_span = match style {
                IslandStyle::Notch => panel_span(span, width, desired),
                IslandStyle::Bubble => bubble_panel_span(self.triggers_span, width, desired),
            };
            let volume_span = if kind == Island::Music && look.status.volume.is_some() {
                let (music, volume) = volume_popover_layout(self.panel_span, width, look.scale);
                self.panel_span = music;
                Some(volume)
            } else {
                None
            };
            let geometry = IslandGeometry::new(self.panel_span, bar_height + px(PANEL_GAP), look);
            let height = px(look.panel_height());
            let theme = Theme::for_popup(look.light, look.glass);
            let content = popup_surface(gfx, theme, layers, geometry, height, px(PANEL_RADIUS))?;
            match kind {
                Island::Music => {
                    if let Some(track) = &look.track {
                        self.draw_player(gfx, &content, geometry, track, cover.as_ref())?;
                    }
                    if let Some(span) = volume_span {
                        let volume_geometry = IslandGeometry::new(span, geometry.top, look);
                        let volume_content = popup_surface(
                            gfx,
                            theme,
                            layers,
                            volume_geometry,
                            height,
                            span.width() / 2.0,
                        )?;
                        self.draw_volume_capsule(gfx, &volume_content, volume_geometry)?;
                        // Keep the small bridge between surfaces interactive,
                        // so moving towards the volume does not dismiss it.
                        self.panel_span.left = self.panel_span.left.min(span.left);
                        self.panel_span.right = self.panel_span.right.max(span.right);
                    }
                }
                Island::Usage => {
                    self.draw_usage_panel(gfx, &content, geometry, height, look, &providers)?
                }
                Island::ControlCenter => {}
            }
        }
        Ok(())
    }

    /// Renders Nosih's native macOS-style Control Center directly beneath the
    /// right side of the menu bar using DirectComposition glass cards.
    fn draw_control_center(
        &mut self,
        gfx: &Gfx,
        look: &Look,
        layers: &VisualCollection,
        screen_width: f32,
        bar_height: f32,
    ) -> Result<()> {
        let px = |logical: f32| (logical * look.scale).round();
        let panel_w = px(CONTROL_CENTER_WIDTH).min((screen_width - px(SIDE_MARGIN) * 2.0).max(0.0));
        let panel_left = (screen_width - px(SIDE_MARGIN) - panel_w).max(0.0).round();
        self.panel_span = Span {
            left: panel_left,
            right: panel_left + panel_w,
        };
        let geometry = IslandGeometry::new(self.panel_span, bar_height + px(PANEL_GAP), look);
        let height = px(look.panel_height());
        let theme = Theme::for_popup(look.light, look.glass);
        let content = popup_surface(gfx, theme, layers, geometry, height, px(PANEL_RADIUS))?;
        let c = &gfx.compositor;
        let ink = geometry.ink();
        let t = strings::text();
        let cc = &look.control_center;

        let card_fill = if look.light {
            rgba(255, 255, 255, 138)
        } else {
            rgba(255, 255, 255, 22)
        };
        let card_border = if look.light {
            rgba(0, 0, 0, 20)
        } else {
            rgba(255, 255, 255, 26)
        };
        let inactive_circle = if look.light {
            rgba(0, 0, 0, 18)
        } else {
            rgba(255, 255, 255, 28)
        };
        let accent_blue = rgba(10, 132, 255, 255);
        let accent_focus = rgba(94, 92, 230, 255);
        let active_tile_fill = if look.light {
            rgba(10, 132, 255, 38)
        } else {
            rgba(10, 132, 255, 72)
        };

        let pad = px(12.0);
        let inner_w = geometry.width - pad * 2.0;
        let gap = px(10.0);
        let col_w = ((inner_w - gap) / 2.0).floor();
        let right_x = pad + col_w + gap;
        let right_w = inner_w - col_w - gap;
        let card_r = px(12.0);

        // --- Row 1 Left: Connectivity card (Wi-Fi + Bluetooth) ---
        let row1_y = pad;
        let row1_h = px(114.0);
        add_rounded_box(
            c,
            &content,
            pad,
            row1_y,
            col_w,
            row1_h,
            card_r,
            card_fill,
            Some(card_border),
        )?;

        let wifi_on = cc.wifi_on || matches!(look.status.network, Network::Wifi { .. });
        let wifi_sub = if let Some(name) = &cc.wifi_name {
            name.as_str()
        } else if matches!(look.status.network, Network::Ethernet) {
            t.cc_ethernet
        } else if wifi_on {
            t.cc_on
        } else {
            t.cc_off
        };
        let bt_on = look.status.bluetooth_on || cc.bluetooth_on;
        let bt_sub = if bt_on { t.cc_on } else { t.cc_off };

        let row_h = row1_h / 2.0;
        let circle = px(30.0);
        for (idx, (active, glyph_ch, title, subtitle, action)) in [
            (
                wifi_on,
                glyph::WIFI,
                t.cc_wifi,
                wifi_sub,
                ControlCenterAction::ToggleWifi,
            ),
            (
                bt_on,
                glyph::BLUETOOTH,
                t.cc_bluetooth,
                bt_sub,
                ControlCenterAction::ToggleBluetooth,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let ry = row1_y + row_h * idx as f32;
            let cx = pad + px(11.0);
            let cy = ry + (row_h - circle) / 2.0;
            add_rounded_box(
                c,
                &content,
                cx,
                cy,
                circle,
                circle,
                circle / 2.0,
                if active { accent_blue } else { inactive_circle },
                None,
            )?;
            let icon = gfx.glyph_brush(
                &[(glyph_ch, 1.0)],
                geometry.scale,
                if active { Ink::Light } else { ink },
            )?;
            add_visual(
                c,
                &content,
                &icon.brush.cast()?,
                cx + (circle - icon.width) / 2.0,
                cy + (circle - icon.height) / 2.0,
                icon.width,
                icon.height,
            )?;
            let text_x = cx + circle + px(9.0);
            let text_w = (col_w - (text_x - pad) - px(8.0)).max(px(24.0));
            let title_brush =
                gfx.text_brush_fitted(title, geometry.scale, 12.0, true, ink, text_w)?;
            add_visual(
                c,
                &content,
                &title_brush.brush.cast()?,
                text_x,
                ry + px(11.0),
                title_brush.width,
                title_brush.height,
            )?;
            let sub_brush =
                gfx.text_brush_fitted(subtitle, geometry.scale, 10.5, false, ink, text_w)?;
            add_visual(
                c,
                &content,
                &sub_brush.brush.cast()?,
                text_x,
                ry + px(28.0),
                sub_brush.width,
                sub_brush.height,
            )?
            .SetOpacity(0.68)?;
            self.island_hits.push(geometry.hit(
                pad,
                ry,
                col_w,
                row_h,
                IslandAction::ControlCenter(action),
            ));
        }
        // Divider between Wi-Fi and Bluetooth rows.
        add_visual(
            c,
            &content,
            &c.CreateColorBrushWithColor(card_border)?.cast()?,
            pad + px(12.0),
            row1_y + row_h,
            col_w - px(24.0),
            px(1.0).max(1.0),
        )?;

        // --- Row 1 Right Top: Focus / Do Not Disturb ---
        let focus_h = px(52.0);
        add_rounded_box(
            c,
            &content,
            right_x,
            row1_y,
            right_w,
            focus_h,
            card_r,
            card_fill,
            Some(card_border),
        )?;
        let focus_cx = right_x + px(11.0);
        let focus_cy = row1_y + (focus_h - circle) / 2.0;
        add_rounded_box(
            c,
            &content,
            focus_cx,
            focus_cy,
            circle,
            circle,
            circle / 2.0,
            if cc.do_not_disturb {
                accent_focus
            } else {
                inactive_circle
            },
            None,
        )?;
        let focus_icon = gfx.glyph_brush(
            &[(glyph::FOCUS, 1.0)],
            geometry.scale,
            if cc.do_not_disturb { Ink::Light } else { ink },
        )?;
        add_visual(
            c,
            &content,
            &focus_icon.brush.cast()?,
            focus_cx + (circle - focus_icon.width) / 2.0,
            focus_cy + (circle - focus_icon.height) / 2.0,
            focus_icon.width,
            focus_icon.height,
        )?;
        let focus_tx = focus_cx + circle + px(9.0);
        let focus_tw = (right_w - (focus_tx - right_x) - px(8.0)).max(px(24.0));
        let focus_title =
            gfx.text_brush_fitted(t.cc_focus, geometry.scale, 12.0, true, ink, focus_tw)?;
        add_visual(
            c,
            &content,
            &focus_title.brush.cast()?,
            focus_tx,
            row1_y + px(9.0),
            focus_title.width,
            focus_title.height,
        )?;
        let focus_sub = gfx.text_brush_fitted(
            if cc.do_not_disturb { t.cc_on } else { t.cc_off },
            geometry.scale,
            10.5,
            false,
            ink,
            focus_tw,
        )?;
        add_visual(
            c,
            &content,
            &focus_sub.brush.cast()?,
            focus_tx,
            row1_y + px(26.0),
            focus_sub.width,
            focus_sub.height,
        )?
        .SetOpacity(0.68)?;
        self.island_hits.push(geometry.hit(
            right_x,
            row1_y,
            right_w,
            focus_h,
            IslandAction::ControlCenter(ControlCenterAction::ToggleDoNotDisturb),
        ));

        // --- Row 1 Right Bottom: Dark Mode + Screen Snip tiles ---
        let tile_y = row1_y + focus_h + px(10.0);
        let tile_h = row1_h - focus_h - px(10.0);
        let tile_gap = px(8.0);
        let tile_w = ((right_w - tile_gap) / 2.0).floor();
        for (idx, (active, glyph_ch, label_text, action)) in [
            (
                cc.dark_mode,
                glyph::DARK_MODE,
                if cc.dark_mode {
                    t.cc_dark_mode
                } else {
                    t.cc_light_mode
                },
                ControlCenterAction::ToggleDarkMode,
            ),
            (
                false,
                glyph::SNIP,
                t.cc_snip,
                ControlCenterAction::ScreenSnip,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tx = right_x + (tile_w + tile_gap) * idx as f32;
            let tw = if idx == 1 {
                right_w - tile_w - tile_gap
            } else {
                tile_w
            };
            add_rounded_box(
                c,
                &content,
                tx,
                tile_y,
                tw,
                tile_h,
                card_r,
                if active { active_tile_fill } else { card_fill },
                Some(card_border),
            )?;
            let icon = gfx.glyph_brush(&[(glyph_ch, 1.0)], geometry.scale, ink)?;
            add_visual(
                c,
                &content,
                &icon.brush.cast()?,
                tx + (tw - icon.width) / 2.0,
                tile_y + px(8.0),
                icon.width,
                icon.height,
            )?;
            let label = gfx.text_brush_fitted(
                label_text,
                geometry.scale,
                10.0,
                false,
                ink,
                tw - px(8.0),
            )?;
            add_visual(
                c,
                &content,
                &label.brush.cast()?,
                tx + (tw - label.width) / 2.0,
                tile_y + px(30.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.84)?;
            self.island_hits.push(geometry.hit(
                tx,
                tile_y,
                tw,
                tile_h,
                IslandAction::ControlCenter(action),
            ));
        }

        // --- Row 2: Display Brightness Slider Card ---
        let disp_y = row1_y + row1_h + gap;
        let slider_card_h = px(58.0);
        add_rounded_box(
            c,
            &content,
            pad,
            disp_y,
            inner_w,
            slider_card_h,
            card_r,
            card_fill,
            Some(card_border),
        )?;
        add_panel_text(
            gfx,
            &content,
            t.cc_display,
            pad + px(12.0),
            disp_y + px(8.0),
            geometry.scale,
            11.5,
            true,
            ink,
            0.9,
        )?;
        let brightness_val = self.brightness_target.or(cc.brightness);
        let disp_label_text = brightness_val
            .map_or_else(|| "—".to_owned(), |percent| format!("{percent}%"));
        let disp_val_brush =
            gfx.text_brush_sized(&disp_label_text, geometry.scale, 11.0, false, ink)?;
        let disp_val_visual = add_visual(
            c,
            &content,
            &disp_val_brush.brush.cast()?,
            pad + inner_w - px(12.0) - disp_val_brush.width,
            disp_y + px(8.0),
            disp_val_brush.width,
            disp_val_brush.height,
        )?;
        disp_val_visual.SetOpacity(0.72)?;

        let slider_h = px(22.0);
        let slider_r = slider_h / 2.0;
        let track_bg = if look.light {
            rgba(0, 0, 0, 26)
        } else {
            rgba(255, 255, 255, 34)
        };
        let track_fill = if look.light {
            rgba(255, 255, 255, 242)
        } else {
            rgba(244, 247, 252, 238)
        };
        let disp_track_x = pad + px(12.0);
        let disp_track_y = disp_y + px(28.0);
        let disp_track_w = inner_w - px(24.0);
        add_rounded_box(
            c,
            &content,
            disp_track_x,
            disp_track_y,
            disp_track_w,
            slider_h,
            slider_r,
            track_bg,
            Some(card_border),
        )?;
        let (_, disp_fill) = retained_rounded_box(
            c,
            &content,
            disp_track_x,
            disp_track_y,
            disp_track_w,
            slider_h,
            slider_r,
            track_fill,
            None,
        )?;
        let mut disp_controls = HorizontalSliderControls {
            fill: disp_fill,
            value: disp_val_visual,
            geometry,
            track_width: disp_track_w,
            track_height: slider_h,
            value_right: pad + inner_w - px(12.0),
            value_y: disp_y + px(8.0),
            position_percent: None,
            label_percent: brightness_val,
            muted: false,
        };
        disp_controls.move_to(brightness_val.unwrap_or(0))?;
        self.cc_brightness_controls = Some(disp_controls);
        let sun_ink = if brightness_val.unwrap_or(0) > 0 {
            Ink::Dark
        } else {
            ink
        };
        let sun = gfx.glyph_brush(&[(glyph::BRIGHTNESS, 0.86)], geometry.scale, sun_ink)?;
        add_visual(
            c,
            &content,
            &sun.brush.cast()?,
            disp_track_x + px(6.0),
            disp_track_y + (slider_h - sun.height) / 2.0,
            sun.width,
            sun.height,
        )?
        .SetOpacity(if brightness_val.is_some() { 1.0 } else { 0.45 })?;
        if brightness_val.is_some() {
            self.island_hits.push(geometry.hit(
                disp_track_x,
                disp_track_y - px(4.0),
                disp_track_w,
                slider_h + px(8.0),
                IslandAction::HorizontalBrightness(HorizontalTrack {
                    left: geometry.left + disp_track_x,
                    right: geometry.left + disp_track_x + disp_track_w,
                }),
            ));
        }

        // --- Row 3: Sound Volume Slider Card ---
        let sound_y = disp_y + slider_card_h + gap;
        add_rounded_box(
            c,
            &content,
            pad,
            sound_y,
            inner_w,
            slider_card_h,
            card_r,
            card_fill,
            Some(card_border),
        )?;
        add_panel_text(
            gfx,
            &content,
            t.cc_sound,
            pad + px(12.0),
            sound_y + px(8.0),
            geometry.scale,
            11.5,
            true,
            ink,
            0.9,
        )?;
        let vol_percent = self
            .volume_target
            .or_else(|| look.status.volume.map(|v| v.percent));
        let vol_muted = look.status.volume.is_some_and(|v| v.muted);
        let sound_label_text = match (vol_percent, vol_muted) {
            (_, true) => t.cc_muted.to_owned(),
            (Some(percent), false) => format!("{percent}%"),
            (None, false) => "—".to_owned(),
        };
        let sound_val_brush =
            gfx.text_brush_sized(&sound_label_text, geometry.scale, 11.0, false, ink)?;
        let sound_val_visual = add_visual(
            c,
            &content,
            &sound_val_brush.brush.cast()?,
            pad + inner_w - px(12.0) - sound_val_brush.width,
            sound_y + px(8.0),
            sound_val_brush.width,
            sound_val_brush.height,
        )?;
        sound_val_visual.SetOpacity(0.72)?;

        let mute_btn_x = pad + px(10.0);
        let mute_btn_y = sound_y + px(28.0);
        let mute_btn_w = px(26.0);
        let spk_glyph = match look.status.volume {
            Some(v) if v.muted => glyph::MUTE,
            Some(v) => glyph::VOLUME[usize::from(v.level.min(3))],
            None => glyph::MUTE,
        };
        let spk = gfx.glyph_brush(&[(spk_glyph, 0.95)], geometry.scale, ink)?;
        add_visual(
            c,
            &content,
            &spk.brush.cast()?,
            mute_btn_x + (mute_btn_w - spk.width) / 2.0,
            mute_btn_y + (slider_h - spk.height) / 2.0,
            spk.width,
            spk.height,
        )?;
        self.island_hits.push(geometry.hit(
            mute_btn_x,
            mute_btn_y - px(4.0),
            mute_btn_w,
            slider_h + px(8.0),
            IslandAction::Mute,
        ));

        let vol_track_x = mute_btn_x + mute_btn_w + px(6.0);
        let vol_track_w = (pad + inner_w - px(12.0) - vol_track_x).max(px(40.0));
        add_rounded_box(
            c,
            &content,
            vol_track_x,
            mute_btn_y,
            vol_track_w,
            slider_h,
            slider_r,
            track_bg,
            Some(card_border),
        )?;
        let (_, vol_fill) = retained_rounded_box(
            c,
            &content,
            vol_track_x,
            mute_btn_y,
            vol_track_w,
            slider_h,
            slider_r,
            track_fill,
            None,
        )?;
        let mut vol_controls = HorizontalSliderControls {
            fill: vol_fill,
            value: sound_val_visual,
            geometry,
            track_width: vol_track_w,
            track_height: slider_h,
            value_right: pad + inner_w - px(12.0),
            value_y: sound_y + px(8.0),
            position_percent: None,
            label_percent: vol_percent,
            muted: vol_muted,
        };
        vol_controls.move_to(if vol_muted {
            0
        } else {
            vol_percent.unwrap_or(0)
        })?;
        self.cc_volume_controls = Some(vol_controls);
        if look.status.volume.is_some() {
            self.island_hits.push(geometry.hit(
                vol_track_x,
                mute_btn_y - px(4.0),
                vol_track_w,
                slider_h + px(8.0),
                IslandAction::HorizontalVolume(HorizontalTrack {
                    left: geometry.left + vol_track_x,
                    right: geometry.left + vol_track_x + vol_track_w,
                }),
            ));
        }

        // --- Row 4 (optional): Now Playing mini-card ---
        let mut next_y = sound_y + slider_card_h + gap;
        if let Some(track) = &look.track {
            let media_h = px(58.0);
            add_rounded_box(
                c,
                &content,
                pad,
                next_y,
                inner_w,
                media_h,
                card_r,
                card_fill,
                Some(card_border),
            )?;
            let art_size = px(36.0);
            if let Some((_, cover_brush)) = &self.cover {
                add_visual(
                    c,
                    &content,
                    &cover_brush.cast()?,
                    pad + px(11.0),
                    next_y + (media_h - art_size) / 2.0,
                    art_size,
                    art_size,
                )?;
            }
            let text_x = pad + px(56.0);
            let btn_size = px(28.0);
            let btns_left = pad + inner_w - px(10.0) - btn_size * 3.0 - px(4.0);
            let title_w = (btns_left - text_x - px(6.0)).max(px(40.0));
            let title_b = gfx.text_brush_fitted(
                track_title(track),
                geometry.scale,
                12.0,
                true,
                ink,
                title_w,
            )?;
            add_visual(
                c,
                &content,
                &title_b.brush.cast()?,
                text_x,
                next_y + px(12.0),
                title_b.width,
                title_b.height,
            )?;
            if !track.artist.trim().is_empty() {
                let artist_b = gfx.text_brush_fitted(
                    &track.artist,
                    geometry.scale,
                    10.5,
                    false,
                    ink,
                    title_w,
                )?;
                add_visual(
                    c,
                    &content,
                    &artist_b.brush.cast()?,
                    text_x,
                    next_y + px(29.0),
                    artist_b.width,
                    artist_b.height,
                )?
                .SetOpacity(0.68)?;
            }
            let play_glyph = if track.playing { '\u{E769}' } else { '\u{E768}' };
            for (idx, (control, g_ch, enabled)) in [
                (MediaControl::Previous, '\u{E892}', track.previous),
                (MediaControl::PlayPause, play_glyph, track.toggle),
                (MediaControl::Next, '\u{E893}', track.next),
            ]
            .into_iter()
            .enumerate()
            {
                let bx = btns_left + (btn_size + px(2.0)) * idx as f32;
                let by = next_y + (media_h - btn_size) / 2.0;
                let sym = gfx.glyph_brush(&[(g_ch, 0.9)], geometry.scale, ink)?;
                add_visual(
                    c,
                    &content,
                    &sym.brush.cast()?,
                    bx + (btn_size - sym.width) / 2.0,
                    by + (btn_size - sym.height) / 2.0,
                    sym.width,
                    sym.height,
                )?
                .SetOpacity(if enabled { 0.92 } else { 0.30 })?;
                if enabled {
                    self.island_hits.push(geometry.hit(
                        bx,
                        by,
                        btn_size,
                        btn_size,
                        IslandAction::Media(control),
                    ));
                }
            }
            next_y += media_h + gap;
        }

        // --- Row 5: Footer Quick Utility Strip ---
        let foot_h = px(36.0);
        let foot_gap = px(8.0);
        let foot_w = ((inner_w - foot_gap * 2.0) / 3.0).floor();
        let left_label = match look.battery {
            Some(battery) => format!("{}%", battery.percent),
            None => t.cc_ethernet.to_owned(),
        };
        let left_action = if look.battery.is_some() {
            ControlCenterAction::OpenPower
        } else {
            ControlCenterAction::OpenNetworkList
        };
        let left_glyph = if look.battery.is_some() {
            '\u{E83F}'
        } else {
            glyph::ETHERNET
        };
        for (idx, (g_ch, label_text, action)) in [
            (left_glyph, left_label.as_str(), left_action),
            (glyph::LOCK, t.cc_lock, ControlCenterAction::LockScreen),
            (
                glyph::SETTINGS,
                t.cc_settings,
                ControlCenterAction::OpenSettings,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let fx = pad + (foot_w + foot_gap) * idx as f32;
            let fw = if idx == 2 {
                inner_w - (foot_w + foot_gap) * 2.0
            } else {
                foot_w
            };
            add_rounded_box(
                c,
                &content,
                fx,
                next_y,
                fw,
                foot_h,
                px(10.0),
                card_fill,
                Some(card_border),
            )?;
            let sym = gfx.glyph_brush(&[(g_ch, 0.85)], geometry.scale, ink)?;
            let lbl = gfx.text_brush_fitted(
                label_text,
                geometry.scale,
                11.0,
                false,
                ink,
                (fw - sym.width - px(18.0)).max(px(20.0)),
            )?;
            let total_w = sym.width + px(5.0) + lbl.width;
            let start_x = fx + (fw - total_w) / 2.0;
            add_visual(
                c,
                &content,
                &sym.brush.cast()?,
                start_x,
                next_y + (foot_h - sym.height) / 2.0,
                sym.width,
                sym.height,
            )?;
            add_visual(
                c,
                &content,
                &lbl.brush.cast()?,
                start_x + sym.width + px(5.0),
                next_y + (foot_h - lbl.height) / 2.0,
                lbl.width,
                lbl.height,
            )?
            .SetOpacity(0.88)?;
            self.island_hits.push(geometry.hit(
                fx,
                next_y,
                fw,
                foot_h,
                IslandAction::ControlCenter(action),
            ));
        }

        Ok(())
    }

    fn draw_usage_panel(
        &mut self,
        gfx: &Gfx,
        content: &VisualCollection,
        geometry: IslandGeometry,
        island_height: f32,
        look: &Look,
        providers: &[IslandProvider<'_>],
    ) -> Result<()> {
        let c = &gfx.compositor;
        let scale = geometry.scale;
        let px = |v| geometry.px(v);
        let island_width = geometry.width;
        let ink = geometry.ink();
        let title = strings::text();
        add_panel_text(
            gfx,
            content,
            title.usage_title,
            px(28.0),
            px(15.0),
            scale,
            13.0,
            true,
            ink,
            0.94,
        )?;
        let button_text = if look.usage_refreshing {
            title.usage_refreshing
        } else {
            title.usage_refresh
        };
        let button_label = gfx.text_brush_sized(button_text, scale, 11.5, true, ink)?;
        let button_width = button_label.width + px(20.0);
        let button_height = px(27.0);
        let button_left = island_width - px(23.0) - button_width;
        let button_top = px(8.0);
        add_rounded_box(
            c,
            content,
            button_left,
            button_top,
            button_width,
            button_height,
            px(13.5),
            geometry.control_fill(),
            Some(geometry.border()),
        )?;
        add_visual(
            c,
            content,
            &button_label.brush.cast()?,
            button_left + px(10.0),
            button_top + (button_height - button_label.height) / 2.0,
            button_label.width,
            button_label.height,
        )?;
        if !look.usage_refreshing {
            self.island_hits.push(geometry.hit(
                button_left,
                button_top,
                button_width,
                button_height,
                IslandAction::Refresh,
            ));
        }
        let body_top = px(42.0);
        let body_height = (island_height - body_top - px(26.0)).max(0.0);
        let content_height = px((look.panel_content_height() - 68.0).max(0.0));
        let maximum = ((content_height - body_height) / scale).max(0.0);
        self.usage_scroll = self.usage_scroll.clamp(0.0, maximum);
        let viewport = c.CreateContainerVisual()?;
        viewport.SetOffset(Vector3::new(0.0, body_top, 0.0))?;
        viewport.SetSize(Vector2::new(island_width, body_height))?;
        viewport.SetClip(&c.CreateInsetClip()?)?;
        let body = c.CreateContainerVisual()?;
        body.SetSize(Vector2::new(island_width, content_height))?;
        body.SetOffset(Vector3::new(0.0, -px(self.usage_scroll), 0.0))?;
        viewport.Children()?.InsertAtTop(&body)?;
        content.InsertAtTop(&viewport)?;
        let rows = body.Children()?;
        let mut row_top = 6.0;
        for (provider, usage, accent) in providers {
            let y = px(row_top);
            row_top += usage_row_height(usage);
            let inset = px(23.0);
            add_visual(
                c,
                &rows,
                &c.CreateColorBrushWithColor(geometry.border())?.cast()?,
                inset,
                y - px(6.0),
                island_width - 2.0 * inset,
                1.0,
            )?;
            draw_usage_row(
                gfx,
                &rows,
                *provider,
                usage,
                *accent,
                scale,
                look.light,
                island_width,
                y,
            )?;
        }
        let bounds = (
            geometry.left,
            geometry.top + body_top,
            geometry.left + island_width,
            geometry.top + body_top + body_height,
        );
        if maximum > 0.0 {
            let [up, down] = [
                ('\u{E70E}', body_top + px(2.0), -1),
                ('\u{E70D}', body_top + body_height - px(22.0), 1),
            ]
            .map(|(symbol, top, direction)| -> Result<SpriteVisual> {
                let label = gfx.glyph_brush(&[(symbol, 1.0)], scale * 0.7, ink)?;
                let visual = add_visual(
                    c,
                    content,
                    &label.brush.cast()?,
                    island_width - px(22.0) + (px(18.0) - label.width) / 2.0,
                    top + (px(20.0) - label.height) / 2.0,
                    label.width,
                    label.height,
                )?;
                self.island_hits.push(geometry.hit(
                    island_width - px(22.0),
                    top,
                    px(18.0),
                    px(20.0),
                    IslandAction::ScrollUsage(direction),
                ));
                Ok(visual)
            });
            self.usage_viewport = Some(UsageViewport {
                content: body,
                maximum,
                scale,
                bounds,
                up: up?,
                down: down?,
            });
            self.position_usage_body()?;
        }
        let newest_age = providers
            .iter()
            .filter_map(|provider| provider.1.age_minutes())
            .min();
        if let Some(minutes) = newest_age {
            let status = format!("{} {minutes} min", title.usage_updated);
            let label = gfx.text_brush_sized(&status, scale, 11.0, false, ink)?;
            add_visual(
                c,
                content,
                &label.brush.cast()?,
                (island_width - label.width) / 2.0,
                island_height - px(21.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.53)?;
        }
        Ok(())
    }

    fn draw_player(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
        track: &Track,
        cover: Option<&CompositionSurfaceBrush>,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let ink = geometry.ink();
        add_panel_text(
            gfx,
            layers,
            strings::text().media_title,
            px(26.0),
            px(15.0),
            geometry.scale,
            12.5,
            true,
            ink,
            0.88,
        )?;
        let chevron = gfx.glyph_brush(&[('', 0.72)], geometry.scale, ink)?;
        let x = geometry.width - px(22.0) - chevron.width;
        add_visual(
            c,
            layers,
            &chevron.brush.cast()?,
            x,
            (px(46.0) - chevron.height) / 2.0,
            chevron.width,
            chevron.height,
        )?;
        self.island_hits.push(geometry.hit(
            x - px(6.0),
            px(7.0),
            chevron.width + px(12.0),
            px(32.0),
            IslandAction::Toggle(Island::Music),
        ));
        let tile = px(34.0);
        if let Some(cover) = cover {
            add_visual(c, layers, &cover.cast()?, px(25.0), px(46.0), tile, tile)?;
        }
        let available = geometry.width - px(91.0);
        let title = gfx.text_brush_fitted(
            track_title(track),
            geometry.scale,
            13.0,
            true,
            ink,
            available,
        )?;
        let title_y = if track.artist.trim().is_empty() {
            px(63.0) - title.height / 2.0
        } else {
            px(47.0)
        };
        add_visual(
            c,
            layers,
            &title.brush.cast()?,
            px(69.0),
            title_y,
            title.width,
            title.height,
        )?;
        if !track.artist.trim().is_empty() {
            let artist =
                gfx.text_brush_fitted(&track.artist, geometry.scale, 11.0, false, ink, available)?;
            add_visual(
                c,
                layers,
                &artist.brush.cast()?,
                px(69.0),
                px(65.0),
                artist.width,
                artist.height,
            )?
            .SetOpacity(0.69)?;
        }
        let center = geometry.width / 2.0;
        for (control, glyph, enabled, offset) in [
            (MediaControl::Previous, '\u{E892}', track.previous, -44.0),
            (
                MediaControl::PlayPause,
                if track.playing {
                    '\u{E769}'
                } else {
                    '\u{E768}'
                },
                track.toggle,
                0.0,
            ),
            (MediaControl::Next, '\u{E893}', track.next, 44.0),
        ] {
            let size = px(34.0);
            let x = center + px(offset) - size / 2.0;
            let y = px(89.0);
            let main = matches!(control, MediaControl::PlayPause);
            if main {
                add_rounded_box(
                    c,
                    layers,
                    x,
                    y,
                    size,
                    size,
                    size / 2.0,
                    geometry.control_fill(),
                    Some(geometry.border()),
                )?;
            }
            let symbol = gfx.glyph_brush(&[(glyph, 1.0)], geometry.scale, ink)?;
            add_visual(
                c,
                layers,
                &symbol.brush.cast()?,
                x + (size - symbol.width) / 2.0,
                y + (size - symbol.height) / 2.0,
                symbol.width,
                symbol.height,
            )?
            .SetOpacity(if enabled { 1.0 } else { 0.30 })?;
            if enabled {
                self.island_hits
                    .push(geometry.hit(x, y, size, size, IslandAction::Media(control)));
            }
        }
        self.draw_progress(gfx, layers, geometry, track)?;
        Ok(())
    }

    fn draw_volume_capsule(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let ink = geometry.ink();
        if let Some(volume) = self.status.volume {
            let symbol = if volume.muted {
                glyph::MUTE
            } else {
                glyph::VOLUME[volume.level.min(3) as usize]
            };
            let label = gfx.glyph_brush(&[(symbol, 1.0)], geometry.scale, ink)?;
            add_visual(
                c,
                layers,
                &label.brush.cast()?,
                (geometry.width - label.width) / 2.0,
                px(155.0) - label.height / 2.0,
                label.width,
                label.height,
            )?;
            self.island_hits.push(geometry.hit(
                (geometry.width - px(28.0)) / 2.0,
                px(141.0),
                px(28.0),
                px(28.0),
                IslandAction::Mute,
            ));
            let left = (geometry.width - px(4.0)) / 2.0;
            let top = px(34.0);
            let height = px(96.0);
            let percent = self.volume_target.unwrap_or(volume.percent);
            add_rounded_box(
                c,
                layers,
                left,
                top,
                px(4.0),
                height,
                px(2.0),
                geometry.control_fill(),
                None,
            )?;
            let (_, fill) = retained_rounded_box(
                c,
                layers,
                left,
                top,
                px(4.0),
                height,
                px(2.0),
                if geometry.light {
                    rgba(40, 48, 58, 220)
                } else {
                    rgba(240, 244, 250, 235)
                },
                None,
            )?;
            let label =
                gfx.text_brush_sized(&format!("{percent}%"), geometry.scale, 10.5, false, ink)?;
            let value = add_visual(
                c,
                layers,
                &label.brush.cast()?,
                (geometry.width - label.width) / 2.0,
                px(12.0),
                label.width,
                label.height,
            )?;
            value.SetOpacity(0.8)?;
            let mut controls = VolumeControls {
                fill,
                value,
                geometry,
                height,
                position_percent: None,
                label_percent: percent,
            };
            controls.move_to(percent)?;
            self.volume_controls = Some(controls);
            self.island_hits.push(geometry.hit(
                (geometry.width - px(28.0)) / 2.0,
                top - px(8.0),
                px(28.0),
                height + px(16.0),
                IslandAction::Volume(VolumeTrack {
                    center_x: geometry.left + geometry.width / 2.0,
                    top: geometry.top + top,
                    bottom: geometry.top + top + height,
                }),
            ));
        }
        Ok(())
    }

    fn draw_progress(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
        track: &Track,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let width = (geometry.width - px(52.0)).max(px(1.0));
        let Some(timeline) = track.timeline else {
            let text = if self.status.volume.is_some() {
                strings::text().media_no_progress
            } else {
                strings::text().media_no_output
            };
            let label =
                gfx.text_brush_fitted(text, geometry.scale, 11.0, false, geometry.ink(), width)?;
            add_visual(
                c,
                layers,
                &label.brush.cast()?,
                px(26.0),
                px(135.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.55)?;
            return Ok(());
        };
        add_rounded_box(
            c,
            layers,
            px(26.0),
            px(139.0),
            width,
            px(4.0),
            px(2.0),
            geometry.control_fill(),
            None,
        )?;
        let (_, fill) = retained_rounded_box(
            c,
            layers,
            px(26.0),
            px(139.0),
            width,
            px(4.0),
            px(2.0),
            if geometry.light {
                rgba(40, 48, 58, 220)
            } else {
                rgba(240, 244, 250, 235)
            },
            None,
        )?;
        let label = gfx.text_brush_sized("0:00", geometry.scale, 10.0, false, geometry.ink())?;
        let elapsed = add_visual(
            c,
            layers,
            &label.brush.cast()?,
            px(26.0),
            px(148.0),
            label.width,
            label.height,
        )?;
        let total = add_visual(
            c,
            layers,
            &label.brush.cast()?,
            px(26.0),
            px(148.0),
            label.width,
            label.height,
        )?;
        elapsed.SetOpacity(0.6)?;
        total.SetOpacity(0.6)?;
        let mut controls = ProgressControls {
            fill,
            elapsed,
            total,
            geometry,
            width,
            elapsed_seconds: None,
            total_seconds: None,
        };
        controls.paint(gfx, timeline, Instant::now())?;
        self.progress_controls = Some(controls);
        Ok(())
    }

    fn sync_progress_watch(&mut self) {
        let shown =
            self.panel == Some(Island::Music) && self.visible && (!self.auto_hide || self.revealed);
        if let Some(media) = &self.media {
            media.show_progress(shown);
        }
        let ticking = shown
            && self.progress_controls.is_some()
            && self
                .look
                .as_ref()
                .and_then(|look| look.track.as_ref())
                .and_then(|track| track.timeline)
                .is_some_and(|timeline| timeline.advancing(Instant::now()));
        if ticking != self.progress_timer {
            // This timer only advances cached position/time labels while the
            // player popup is visible. No API calls, artwork reads or polling.
            unsafe {
                self.progress_timer = if ticking {
                    SetTimer(Some(self.hwnd), PROGRESS_TIMER, 1000, None) != 0
                } else {
                    let _ = KillTimer(Some(self.hwnd), PROGRESS_TIMER);
                    false
                };
            }
        }
    }

    pub fn progress_tick(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(timeline) = self
            .look
            .as_ref()
            .and_then(|look| look.track.as_ref())
            .and_then(|track| track.timeline)
            && let Some(controls) = &mut self.progress_controls
        {
            controls.paint(gfx, timeline, Instant::now())?;
        }
        self.sync_progress_watch();
        Ok(())
    }

    pub fn press_volume(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        self.suppress_volume_release = false;
        let Some(drag) = self.island_hits.iter().find_map(|hit| {
            if !hit.at(x, y) {
                return None;
            }
            match hit.action {
                IslandAction::Volume(span) => Some(SliderDrag::VerticalVolume(span)),
                IslandAction::HorizontalVolume(span) => Some(SliderDrag::HorizontalVolume(span)),
                IslandAction::HorizontalBrightness(span) => {
                    Some(SliderDrag::HorizontalBrightness(span))
                }
                _ => None,
            }
        }) else {
            return Ok(());
        };
        self.slider_drag = Some(drag);
        // SAFETY: Capture only for a live slider drag, never to activate the bar.
        unsafe {
            SetCapture(self.hwnd);
        }
        self.queue_slider(drag, x, y);
        self.paint_volume_preview(gfx)
    }

    pub fn scroll_usage_at(&mut self, point: POINT, delta: i16) -> Result<bool> {
        if !self.visible || (self.auto_hide && !self.revealed) || delta == 0 {
            return Ok(false);
        }
        let Some(viewport) = &self.usage_viewport else {
            return Ok(false);
        };
        let (x, y) = (
            (point.x - self.bounds.left) as f32,
            (point.y - self.bounds.top) as f32,
        );
        let (left, top, right, bottom) = viewport.bounds;
        if x < left || x >= right || y < top || y >= bottom {
            return Ok(false);
        }
        self.scroll_usage(-f32::from(delta) / 120.0 * 48.0)
    }

    fn scroll_usage(&mut self, delta: f32) -> Result<bool> {
        let Some(viewport) = &self.usage_viewport else {
            return Ok(false);
        };
        let next = (self.usage_scroll + delta).clamp(0.0, viewport.maximum);
        if next != self.usage_scroll {
            self.usage_scroll = next;
            self.position_usage_body()?;
        }
        Ok(true)
    }

    fn position_usage_body(&mut self) -> Result<()> {
        let Some(viewport) = &self.usage_viewport else {
            return Ok(());
        };
        let offset = (self.usage_scroll * viewport.scale).round();
        viewport
            .content
            .SetOffset(Vector3::new(0.0, -offset, 0.0))?;
        viewport
            .up
            .SetOpacity(if self.usage_scroll > 0.0 { 0.8 } else { 0.22 })?;
        viewport
            .down
            .SetOpacity(if self.usage_scroll < viewport.maximum {
                0.8
            } else {
                0.22
            })?;
        Ok(())
    }

    fn paint_volume_preview(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(percent) = self.volume_target {
            if let Some(controls) = &mut self.volume_controls {
                controls.paint(gfx, percent)?;
            }
            if let Some(controls) = &mut self.cc_volume_controls {
                controls.paint(gfx, percent, false)?;
            }
        }
        if let Some(percent) = self.brightness_target
            && let Some(controls) = &mut self.cc_brightness_controls
        {
            controls.paint(gfx, percent, false)?;
        }
        Ok(())
    }

    fn queue_slider(&mut self, drag: SliderDrag, x: f32, y: f32) {
        match drag {
            SliderDrag::VerticalVolume(span) => {
                self.queue_volume_percent(volume_percent(span, y));
            }
            SliderDrag::HorizontalVolume(span) => {
                self.queue_volume_percent(horizontal_percent(span, x));
            }
            SliderDrag::HorizontalBrightness(span) => {
                self.queue_brightness_percent(horizontal_percent(span, x));
            }
        }
    }

    fn queue_volume_percent(&mut self, percent: u8) {
        self.volume_target = Some(percent);
        if let Some(controls) = &mut self.volume_controls {
            let _ = controls.move_to(percent);
        }
        if let Some(controls) = &mut self.cc_volume_controls {
            let _ = controls.move_to(percent);
        }
        if self.pending_volume == Some(percent)
            || (self.pending_volume.is_none()
                && self
                    .status
                    .volume
                    .is_some_and(|v| v.percent == percent && !v.muted))
        {
            return;
        }
        let arm = self.pending_volume.is_none();
        self.pending_volume = Some(percent);
        if arm {
            // At most 30 endpoint writes/second during dragging; no idle timer.
            unsafe {
                SetTimer(Some(self.hwnd), VOLUME_TIMER, 33, None);
            }
        }
    }

    fn queue_brightness_percent(&mut self, percent: u8) {
        self.brightness_target = Some(percent);
        if let Some(controls) = &mut self.cc_brightness_controls {
            let _ = controls.move_to(percent);
        }
        if self.pending_brightness == Some(percent) {
            return;
        }
        let arm = self.pending_brightness.is_none();
        self.pending_brightness = Some(percent);
        if arm {
            unsafe {
                SetTimer(Some(self.hwnd), BRIGHTNESS_TIMER, 50, None);
            }
        }
    }

    pub fn flush_volume(&mut self) -> Result<()> {
        // SAFETY: Our own short-lived drag timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), VOLUME_TIMER);
        }
        if let Some(percent) = self.pending_volume.take()
            && let Some(audio) = &mut self.audio
        {
            audio.set_percent(percent)?;
        }
        Ok(())
    }

    pub fn flush_brightness(&mut self) -> Result<()> {
        // SAFETY: Our own short-lived drag timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), BRIGHTNESS_TIMER);
        }
        if let Some(percent) = self.pending_brightness.take()
            && let Some(cc) = &self.control_center
        {
            cc.set_brightness(percent);
        }
        Ok(())
    }

    pub fn release_volume(&mut self, point: Option<(f32, f32)>) -> Result<bool> {
        let Some(drag) = self.slider_drag.take() else {
            return Ok(point.is_some() && std::mem::take(&mut self.suppress_volume_release));
        };
        self.suppress_volume_release = true;
        if let Some((x, y)) = point {
            self.queue_slider(drag, x, y);
        }
        // Clear state first: ReleaseCapture posts a capture-lost notification.
        unsafe {
            if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
        if let Err(error) = self.flush_volume() {
            nosih_shell::log::line(&format!("ajustar volumen: {error:#}"));
        }
        if let Err(error) = self.flush_brightness() {
            nosih_shell::log::line(&format!("ajustar brillo: {error:#}"));
        }
        self.volume_target = None;
        self.brightness_target = None;
        Ok(true)
    }
}

fn volume_percent(span: VolumeTrack, y: f32) -> u8 {
    let height = span.bottom - span.top;
    if height <= 0.0 || !y.is_finite() {
        return 0;
    }
    (((span.bottom - y) / height).clamp(0.0, 1.0) * 100.0).round() as u8
}

fn horizontal_percent(span: HorizontalTrack, x: f32) -> u8 {
    let width = span.right - span.left;
    if width <= 0.0 || !x.is_finite() {
        return 0;
    }
    (((x - span.left) / width).clamp(0.0, 1.0) * 100.0).round() as u8
}

fn control_center_height(has_media: bool) -> f32 {
    if has_media { 388.0 } else { 320.0 }
}

/// Prefer the player's right edge, then its left. On narrow displays reserve
/// space for the capsule instead of covering the progress or playback buttons.
fn volume_popover_layout(panel: Span, width: f32, scale: f32) -> (Span, Span) {
    let px = |value: f32| (value * scale).round();
    let margin = px(12.0).min(width / 10.0);
    let available = (width - margin * 2.0).max(0.0);
    let capsule = px(44.0).min(available / 3.0);
    let gap = px(8.0).min(available / 10.0);
    let player_width = panel.width().min((available - capsule - gap).max(0.0));
    let mut left = panel
        .left
        .clamp(margin, (width - margin - player_width).max(margin));
    let volume_left = if left + player_width + gap + capsule <= width - margin {
        left + player_width + gap
    } else if left - gap - capsule >= margin {
        left - gap - capsule
    } else {
        left = (width - margin - player_width - gap - capsule).max(margin);
        left + player_width + gap
    };
    (
        Span {
            left,
            right: left + player_width,
        },
        Span {
            left: volume_left,
            right: volume_left + capsule,
        },
    )
}

fn media_time(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn same_volume_indicator(old: Option<Volume>, new: Option<Volume>) -> bool {
    old.map(|v| (v.level, v.muted)) == new.map(|v| (v.level, v.muted))
}

#[derive(Clone, Copy)]
struct IslandGeometry {
    left: f32,
    top: f32,
    width: f32,
    scale: f32,
    light: bool,
}

impl IslandGeometry {
    fn new(span: Span, top: f32, look: &Look) -> Self {
        Self {
            left: span.left,
            top,
            width: span.width(),
            scale: look.scale,
            light: look.light,
        }
    }

    fn hit(self, x: f32, y: f32, width: f32, height: f32, action: IslandAction) -> IslandHit {
        let left = self.left + x;
        let top = self.top + y;
        IslandHit {
            bounds: (left, top, left + width, top + height),
            action,
        }
    }

    fn px(self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }
    fn ink(self) -> Ink {
        if self.light { Ink::Dark } else { Ink::Light }
    }
    fn control_fill(self) -> Color {
        if self.light {
            rgba(0, 0, 0, 18)
        } else {
            rgba(255, 255, 255, 27)
        }
    }
    fn border(self) -> Color {
        if self.light {
            rgba(35, 57, 83, 31)
        } else {
            rgba(223, 237, 255, 37)
        }
    }
}

fn toggle_panel(open: Option<Island>, clicked: Island) -> Option<Island> {
    (open != Some(clicked)).then_some(clicked)
}

fn available_panel(open: Option<Island>, music: bool, usage: bool) -> Option<Island> {
    open.filter(|island| match island {
        Island::Music => music,
        Island::Usage => usage,
        Island::ControlCenter => true,
    })
}

fn trigger_layout(
    bounds: Span,
    center: f32,
    scale: f32,
    music: bool,
    providers: usize,
) -> [(Island, Option<Span>); 2] {
    let px = |v: f32| (v * scale).round();
    let mut result = [(Island::Music, None), (Island::Usage, None)];
    let gap = if music && providers > 0 { px(8.0) } else { 0.0 };
    let music_width = if music { px(214.0) } else { 0.0 };
    let usage_width = match providers {
        0 => 0.0,
        1 => px(ISLAND_SINGLE_WIDTH),
        count => px(134.0 * count as f32),
    };
    let total = music_width + usage_width;
    let available = (bounds.width() - gap).min(total);
    if total == 0.0 || available < px(if gap > 0.0 { 85.0 } else { 42.0 }) {
        return result;
    }
    let music_width = if gap > 0.0 {
        (available * music_width / total)
            .round()
            .clamp(px(42.0), available - px(42.0))
    } else if music {
        available
    } else {
        0.0
    };
    let left = (center - (available + gap) / 2.0)
        .clamp(
            bounds.left,
            (bounds.right - available - gap).max(bounds.left),
        )
        .round();
    if music {
        result[0].1 = Some(Span {
            left,
            right: left + music_width,
        });
    }
    if providers > 0 {
        result[1].1 = Some(Span {
            left: left + music_width + gap,
            right: left + available + gap,
        });
    }
    result
}

fn bubble_trigger_layout(
    bounds: Span,
    scale: f32,
    music_width: Option<f32>,
    providers: usize,
) -> [(Island, Option<Span>); 2] {
    let px = |v: f32| (v * scale).round();
    let mut result = [(Island::Music, None), (Island::Usage, None)];
    let music = music_width.is_some();
    let gap = if music && providers > 0 { px(8.0) } else { 0.0 };
    let music_width = music_width
        .map(|width| width.clamp(px(36.0), px(BUBBLE_MUSIC_WIDTH)))
        .unwrap_or(0.0);
    let usage_width = if providers > 0 {
        px(BUBBLE_USAGE_PADDING + BUBBLE_PROVIDER_WIDTH * providers as f32)
    } else {
        0.0
    };
    let total = music_width + usage_width;
    let available = (bounds.width() - gap).min(total);
    if total == 0.0 || available < px(if gap > 0.0 { 72.0 } else { 36.0 }) {
        return result;
    }
    let music_width = if gap > 0.0 {
        (available * music_width / total)
            .round()
            .clamp(px(36.0), available - px(36.0))
    } else if music {
        available
    } else {
        0.0
    };
    let left = (bounds.right - available - gap).max(bounds.left).round();
    if music {
        result[0].1 = Some(Span {
            left,
            right: left + music_width,
        });
    }
    if providers > 0 {
        result[1].1 = Some(Span {
            left: left + music_width + gap,
            right: left + available + gap,
        });
    }
    result
}

fn panel_span(trigger: Span, screen_width: f32, desired: f32) -> Span {
    let width = desired.max(trigger.width()).min(screen_width);
    let left = ((trigger.left + trigger.right - width) / 2.0)
        .clamp(0.0, screen_width - width)
        .round();
    Span {
        left,
        right: left + width,
    }
}

fn bubble_panel_span(anchor: Span, screen_width: f32, desired: f32) -> Span {
    let width = desired.max(anchor.width()).min(screen_width);
    let left = (anchor.right - width)
        .clamp(0.0, screen_width - width)
        .round();
    Span {
        left,
        right: left + width,
    }
}

type IslandProvider<'a> = (Provider, &'a ProviderUsage, Color);

fn island_providers(look: &Look) -> Vec<IslandProvider<'_>> {
    Provider::ALL
        .into_iter()
        .filter(|provider| look.active_providers.has(*provider))
        .map(|provider| {
            let accent = match provider {
                Provider::Claude => rgba(217, 119, 87, 255),
                Provider::Codex => rgba(135, 190, 245, 255),
                Provider::OpenCode => rgba(132, 196, 166, 255),
                Provider::Gemini => rgba(111, 146, 255, 255),
                Provider::Zai => rgba(79, 110, 247, 255),
            };
            (provider, look.usage.get(provider), accent)
        })
        .collect()
}

fn popup_surface(
    gfx: &Gfx,
    theme: Theme,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    height: f32,
    radius: f32,
) -> Result<VisualCollection> {
    let c = &gfx.compositor;
    let size = Vector2::new(geometry.width, height);
    let panel = theme.panel(gfx, size, radius, geometry.scale)?;
    let popup = c.CreateContainerVisual()?;
    popup.SetSize(size)?;
    popup.SetOffset(Vector3::new(geometry.left.round(), geometry.top, 0.0))?;
    let children = popup.Children()?;
    let mask = c.CreateNineGridBrush()?;
    mask.SetSource(&gfx.rounded_mask(radius)?)?;
    mask.SetInsets(radius.ceil() + 1.0)?;
    children.InsertAtTop(&island_shadow(c, &mask.cast()?, size, geometry)?)?;
    children.InsertAtTop(&panel)?;
    layers.InsertAtTop(&popup)?;
    Ok(children)
}

fn island_shadow(
    c: &Compositor,
    mask: &CompositionBrush,
    size: Vector2,
    geometry: IslandGeometry,
) -> Result<SpriteVisual> {
    let shadow = c.CreateDropShadow()?;
    shadow.SetMask(mask)?;
    shadow.SetBlurRadius(geometry.px(12.0))?;
    shadow.SetOffset(Vector3::new(0.0, geometry.px(5.0), 0.0))?;
    shadow.SetColor(rgba(0, 0, 0, 92))?;
    let host = c.CreateSpriteVisual()?;
    host.SetSize(size)?;
    host.SetShadow(&shadow)?;
    Ok(host)
}

fn island_surface(
    gfx: &Gfx,
    look: &Look,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    island_height: f32,
) -> Result<VisualCollection> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let island_left = geometry.left;
    let island_width = geometry.width;
    let scale = geometry.scale;
    let size = Vector2::new(island_width, island_height);
    let island = c.CreateContainerVisual()?;
    island.SetSize(size)?;
    island.SetOffset(Vector3::new(island_left.round(), 0.0, 0.0))?;
    let content = island.Children()?;
    let silhouette = gfx.island_mask(island_width, island_height, scale)?;
    content.InsertAtTop(&island_shadow(c, &silhouette.cast()?, size, geometry)?)?;

    let tint = match (look.light, look.glass) {
        (true, true) => rgba(247, 249, 252, 243),
        (false, true) => rgba(7, 9, 13, 242),
        (true, false) => rgba(247, 249, 252, 255),
        (false, false) => rgba(10, 12, 17, 255),
    };
    if look.glass {
        // The backdrop cannot be used as a mask-brush source. Inset its
        // rounded clip so it stays entirely inside the curved silhouette.
        let inset = (12.0 * scale)
            .min(island_width / 5.0)
            .min(island_height / 2.0);
        let glass_width = island_width - inset * 2.0;
        let glass = c.CreateContainerVisual()?;
        glass.SetSize(Vector2 {
            X: glass_width,
            Y: island_height,
        })?;
        glass.SetOffset(Vector3 {
            X: inset,
            Y: 0.0,
            Z: 0.0,
        })?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        let lift = px(22.0);
        outline.SetSize(Vector2 {
            X: glass_width,
            Y: island_height + lift,
        })?;
        outline.SetOffset(Vector2 { X: 0.0, Y: -lift })?;
        outline.SetCornerRadius(Vector2 { X: lift, Y: lift })?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;
        add_visual(
            c,
            &glass.Children()?,
            &c.CreateHostBackdropBrush()?.cast()?,
            0.0,
            0.0,
            glass_width,
            island_height,
        )?;
        content.InsertAtTop(&glass)?;
    }
    let masked = c.CreateMaskBrush()?;
    masked.SetSource(&c.CreateColorBrushWithColor(tint)?)?;
    masked.SetMask(&silhouette)?;
    add_visual(
        c,
        &content,
        &masked.cast()?,
        0.0,
        0.0,
        island_width,
        island_height,
    )?;

    layers.InsertAtTop(&island)?;
    Ok(content)
}

fn bubble_surface(
    gfx: &Gfx,
    look: &Look,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    bar_height: f32,
) -> Result<(VisualCollection, f32)> {
    let c = &gfx.compositor;
    let inset_y = geometry.px(BUBBLE_VERTICAL_INSET);
    let pill_height = (bar_height - 2.0 * inset_y).max(1.0);
    let radius = pill_height / 2.0;
    let size = Vector2::new(geometry.width, pill_height);
    let bubble = c.CreateContainerVisual()?;
    bubble.SetSize(size)?;
    bubble.SetOffset(Vector3::new(geometry.left.round(), inset_y, 0.0))?;
    let content = bubble.Children()?;

    let tint = match (look.light, look.glass) {
        (true, true) => rgba(247, 249, 252, 243),
        (false, true) => rgba(7, 9, 13, 242),
        (true, false) => rgba(247, 249, 252, 255),
        (false, false) => rgba(10, 12, 17, 255),
    };
    if look.glass {
        let glass = c.CreateContainerVisual()?;
        glass.SetSize(size)?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        outline.SetSize(size)?;
        outline.SetCornerRadius(Vector2 {
            X: radius,
            Y: radius,
        })?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;
        add_visual(
            c,
            &glass.Children()?,
            &c.CreateHostBackdropBrush()?.cast()?,
            0.0,
            0.0,
            geometry.width,
            pill_height,
        )?;
        content.InsertAtTop(&glass)?;
    }
    add_rounded_box(
        c,
        &content,
        0.0,
        0.0,
        geometry.width,
        pill_height,
        radius,
        tint,
        None,
    )?;

    layers.InsertAtTop(&bubble)?;
    Ok((content, pill_height))
}

/// Draws the providers in the closed island and returns where each one's
/// figure sits (its centre, from the segment's left edge), for the crab.
/// The moment an AI plan's figure crosses a line worth showing, comparing
/// the previous update with this one: a limit reset to 0 % (after being in
/// use), past 90 %, or at 100 %. Nothing on the first figures after starting,
/// which have nothing to compare with.
fn usage_moment(
    old: &usage::Snapshot,
    new: &usage::Snapshot,
    active: &ActiveProviders,
) -> Option<(Provider, mascot::Act)> {
    Provider::ALL
        .into_iter()
        .filter(|p| active.has(*p))
        .find_map(|provider| {
            let previous = old.get(provider);
            let current = new.get(provider);
            if previous.state != State::Ready || current.state != State::Ready {
                return None;
            }
            let before_window = previous.main_window()?;
            let now_window = current.main_window()?;
            if before_window.minutes != now_window.minutes
                || before_window.label != now_window.label
            {
                return None;
            }
            if let (Some(before), Some(now)) = (previous.fetched_at, current.fetched_at)
                && now <= before
            {
                return None;
            }
            let before = before_window.used_percent;
            let now = now_window.used_percent;
            let act = if now >= 100 && before < 100 {
                mascot::Act::Exhausted
            } else if now >= 90 && before < 90 {
                mascot::Act::Nervous
            } else if now <= 1 && before >= 20 {
                mascot::Act::Fresh
            } else {
                return None;
            };
            Some((provider, act))
        })
}

fn draw_compact_usage(
    gfx: &Gfx,
    content: &VisualCollection,
    geometry: IslandGeometry,
    bar_height: f32,
    providers: &[IslandProvider<'_>],
) -> Result<Vec<(Provider, f32)>> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let scale = geometry.scale;
    let inset = px(13.0);
    let half = (geometry.width - 2.0 * inset) / providers.len().max(1) as f32;
    if half < px(20.0) {
        let icon = gfx.glyph_brush(&[('\u{E9D9}', 1.0)], scale, ink)?;
        add_visual(
            c,
            content,
            &icon.brush.cast()?,
            (geometry.width - icon.width) / 2.0,
            (bar_height - icon.height) / 2.0,
            icon.width,
            icon.height,
        )?;
        let centre = geometry.width / 2.0;
        return Ok(providers
            .iter()
            .map(|(provider, ..)| (*provider, centre))
            .collect());
    }
    let mut anchors = Vec::with_capacity(providers.len());
    for (index, (provider, usage, accent)) in providers.iter().enumerate() {
        anchors.push((*provider, inset + (index as f32 + 0.5) * half));
        // When space is tight the full list is still available in the panel.
        if half < px(20.0) {
            break;
        }
        let icon_size = px(18.0) as u32;
        let start = inset
            + index as f32 * half
            + if half < px(84.0) {
                (half - icon_size as f32) / 2.0
            } else {
                px(21.0)
            };
        if let Some(image) = usage::logo(*provider) {
            let image_brush = gfx.icon_brush(image, icon_size)?;
            let brush: CompositionBrush =
                if !matches!(provider, Provider::Claude | Provider::Gemini) && geometry.light {
                    let tinted = c.CreateMaskBrush()?;
                    tinted.SetMask(&image_brush)?;
                    tinted.SetSource(&c.CreateColorBrushWithColor(rgba(27, 34, 45, 255))?)?;
                    tinted.cast()?
                } else {
                    image_brush.cast()?
                };
            add_visual(
                c,
                content,
                &brush,
                start,
                (bar_height - icon_size as f32) / 2.0,
                icon_size as f32,
                icon_size as f32,
            )?;
        }
        let percent = usage.compact_text();
        if half > px(106.0) {
            let font = if usage.main_window().is_some() {
                14.0
            } else {
                12.0
            };
            let label = gfx.text_brush_sized(&percent, scale, font, true, ink)?;
            let reserve = if usage.main_window().is_some() {
                74.0
            } else {
                50.0
            };
            if label.width <= half - px(reserve) {
                add_visual(
                    c,
                    content,
                    &label.brush.cast()?,
                    start + px(25.0),
                    (bar_height - label.height) / 2.0,
                    label.width,
                    label.height,
                )?;
            }
        }
        if half >= px(84.0) {
            let ring = gfx.usage_ring(
                usage.main_window().map(|window| window.used_percent),
                scale,
                *accent,
            )?;
            add_visual(
                c,
                content,
                &ring.brush.cast()?,
                inset + (index + 1) as f32 * half - px(16.0) - ring.width,
                (bar_height - ring.height) / 2.0,
                ring.width,
                ring.height,
            )?;
        }
    }
    Ok(anchors)
}

fn draw_bubble_usage(
    gfx: &Gfx,
    content: &VisualCollection,
    geometry: IslandGeometry,
    pill_height: f32,
    providers: &[IslandProvider<'_>],
) -> Result<Vec<(Provider, f32)>> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let scale = geometry.scale;
    let inset = px(8.0);
    let slot = (geometry.width - 2.0 * inset) / providers.len().max(1) as f32;
    if slot < px(20.0) {
        let icon = gfx.glyph_brush(&[('\u{E9D9}', 1.0)], scale, ink)?;
        add_visual(
            c,
            content,
            &icon.brush.cast()?,
            (geometry.width - icon.width) / 2.0,
            (pill_height - icon.height) / 2.0,
            icon.width,
            icon.height,
        )?;
        let centre = geometry.width / 2.0;
        return Ok(providers
            .iter()
            .map(|(provider, ..)| (*provider, centre))
            .collect());
    }
    let icon_size = px(15.0) as u32;
    let gap = px(4.0);
    let mut anchors = Vec::with_capacity(providers.len());
    for (index, (provider, usage, _accent)) in providers.iter().enumerate() {
        let slot_left = inset + index as f32 * slot;
        anchors.push((*provider, slot_left + slot / 2.0));
        let percent = usage.compact_text();
        let label = if slot >= px(44.0) {
            let label = gfx.text_brush_sized(&percent, scale, 11.5, true, ink)?;
            (label.width + icon_size as f32 + gap <= slot).then_some(label)
        } else {
            None
        };
        let group_width = icon_size as f32 + label.as_ref().map_or(0.0, |text| gap + text.width);
        let start = slot_left + ((slot - group_width) / 2.0).max(0.0);
        if let Some(image) = usage::logo(*provider) {
            let image_brush = gfx.icon_brush(image, icon_size)?;
            let brush: CompositionBrush =
                if !matches!(provider, Provider::Claude | Provider::Gemini) && geometry.light {
                    let tinted = c.CreateMaskBrush()?;
                    tinted.SetMask(&image_brush)?;
                    tinted.SetSource(&c.CreateColorBrushWithColor(rgba(27, 34, 45, 255))?)?;
                    tinted.cast()?
                } else {
                    image_brush.cast()?
                };
            add_visual(
                c,
                content,
                &brush,
                start,
                (pill_height - icon_size as f32) / 2.0,
                icon_size as f32,
                icon_size as f32,
            )?;
        }
        if let Some(label) = label {
            add_visual(
                c,
                content,
                &label.brush.cast()?,
                start + icon_size as f32 + gap,
                (pill_height - label.height) / 2.0,
                label.width,
                label.height,
            )?;
        }
    }
    Ok(anchors)
}

fn track_title(track: &Track) -> &str {
    if track.title.trim().is_empty() {
        strings::text().media_title
    } else {
        &track.title
    }
}

fn draw_compact_player(
    gfx: &Gfx,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    width: f32,
    height: f32,
    track: &Track,
    cover: Option<&CompositionSurfaceBrush>,
) -> Result<()> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let side = px(18.0).min(height).min(width);
    let x = px(23.0) + side;
    let show_title = width > x + px(18.0);
    let note_x = if show_title {
        px(18.0)
    } else {
        (width - side) / 2.0
    };
    if let Some(cover) = cover {
        add_visual(
            c,
            layers,
            &cover.cast()?,
            note_x,
            (height - side) / 2.0,
            side,
            side,
        )?
        .SetOpacity(if track.playing { 1.0 } else { 0.7 })?;
    }
    if show_title {
        let label = gfx.text_brush_fitted(
            track_title(track),
            geometry.scale,
            11.5,
            true,
            ink,
            width - x - px(13.0),
        )?;
        add_visual(
            c,
            layers,
            &label.brush.cast()?,
            x,
            (height - label.height) / 2.0,
            label.width,
            label.height,
        )?;
    }
    Ok(())
}

struct BubbleMusic {
    artwork_size: f32,
    label: LabelBrush,
    width: f32,
}

fn prepare_bubble_music(gfx: &Gfx, look: &Look, track: &Track) -> Result<BubbleMusic> {
    let px = |v: f32| (v * look.scale).round();
    let ink = if look.light { Ink::Dark } else { Ink::Light };
    let artwork_size = px(18.0);
    let left_pad = px(10.0);
    let gap = px(6.0);
    let right_pad = px(8.0);
    let max_text =
        (px(BUBBLE_MUSIC_WIDTH) - left_pad - artwork_size - gap - right_pad).max(px(18.0));
    let label = gfx.text_brush_fitted(track_title(track), look.scale, 11.5, true, ink, max_text)?;
    let width = (left_pad + artwork_size + gap + label.width + right_pad)
        .min(px(BUBBLE_MUSIC_WIDTH))
        .ceil();
    Ok(BubbleMusic {
        artwork_size,
        label,
        width,
    })
}

fn draw_bubble_player(
    gfx: &Gfx,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    height: f32,
    track: &Track,
    prepared: &BubbleMusic,
    cover: Option<&CompositionSurfaceBrush>,
) -> Result<()> {
    let width = geometry.width;
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let side = prepared.artwork_size.min(height).min(width);
    let left_pad = px(10.0);
    let gap = px(6.0);
    let right_pad = px(8.0);
    let text_x = left_pad + side + gap;
    let max_label_width = width - text_x - right_pad;
    let show_title = max_label_width >= px(18.0);
    let note_x = if show_title {
        left_pad
    } else {
        (width - side) / 2.0
    };
    if let Some(cover) = cover {
        add_visual(
            c,
            layers,
            &cover.cast()?,
            note_x,
            (height - side) / 2.0,
            side,
            side,
        )?
        .SetOpacity(if track.playing { 1.0 } else { 0.7 })?;
    }
    if show_title {
        let fitted;
        let label = if prepared.label.width <= max_label_width {
            &prepared.label
        } else {
            fitted = gfx.text_brush_fitted(
                track_title(track),
                geometry.scale,
                11.5,
                true,
                ink,
                max_label_width,
            )?;
            &fitted
        };
        add_visual(
            c,
            layers,
            &label.brush.cast()?,
            text_x,
            (height - label.height) / 2.0,
            label.width,
            label.height,
        )?;
    }
    Ok(())
}

impl Drop for MenuBar {
    fn drop(&mut self) {
        self.search_watch = None;
        self.settings_watch = None;
        self.privacy_watch = None;
        self.bluetooth = None;
        self.accessories = None;
        self.exit_animation = None;
        self.media = None;
        self.control_center = None;
        ISLAND_HOOK_WINDOW.store(0, Ordering::Relaxed);
        edge_watch::set_menu_bar(None);
        if let Some(hook) = self.outside_click_hook.take() {
            // SAFETY: Do not leave a callback pointing at a destroyed window.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
        // Give the reserved strip back before the window goes away.
        self.appbar = None;
        // SAFETY: Our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod island_tests {
    use super::*;

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic preferences, no Windows settings writes"]
    fn native_status_items_follow_windows_preferences_without_overrides() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        look.battery = Some(Battery {
            percent: 75,
            charging: false,
            saver: false,
        });
        look.accessories = [None, None];
        look.status.bluetooth_on = false;
        let mut battery_width = [0.0; 2];
        for mask in 0..16 {
            look.status.buttons = TaskbarButtons {
                search: mask & 1 != 0,
                task_view: mask & 2 != 0,
                widgets: mask & 4 != 0,
            };
            look.status.battery_percentage = mask & 8 != 0;
            bar.apply_look(&gfx, look.clone())?;
            for (action, on) in [
                (Action::Search, look.status.buttons.search),
                (Action::TaskView, look.status.buttons.task_view),
                (Action::Widgets, look.status.buttons.widgets),
            ] {
                assert_eq!(
                    bar.zones.iter().filter(|(_, _, a)| *a == action).count(),
                    usize::from(on)
                );
            }
            // The battery is the one Power zone. Its percentage must add
            // label width, without introducing another clickable control.
            let mut power = bar.zones.iter().filter(|(_, _, a)| *a == Action::Power);
            let battery = power.next().context("battery zone")?;
            assert!(power.next().is_none());
            battery_width[usize::from(look.status.battery_percentage)] = battery.1 - battery.0;
        }
        assert!(battery_width[1] > battery_width[0]);
        // No phantom battery or percentage on a desktop without a battery.
        look.battery = None;
        bar.apply_look(&gfx, look)?;
        let count = |action: Action| bar.zones.iter().filter(|(_, _, a)| *a == action).count();
        // Only the control center opens all of quick settings; the network
        // icon has its own list (issue #55).
        assert_eq!(count(Action::QuickSettings), 1);
        assert_eq!(count(Action::Network), 1);
        assert_eq!(count(Action::Power), 0);

        // The native registry handler reads the real Windows values without
        // replacing the media/provider snapshot or resetting the clock timer.
        bar.registry_changed(&gfx)?;
        let current = bar.look.as_ref().context("updated look")?;
        assert!(current.status.buttons == status::taskbar_buttons());
        assert_eq!(
            current.status.battery_percentage,
            status::battery_percentage_enabled()
        );
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic preferences, nothing is opened"]
    fn native_notification_bell_takes_the_right_end_only_when_chosen() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for notifications in [false, true, false] {
            look.widgets.notifications = notifications;
            bar.apply_look(&gfx, look.clone())?;
            // Zones are pushed from the right edge inward.
            let order: Vec<Action> = bar.zones.iter().map(|(_, _, a)| *a).collect();
            let expected: &[Action] = if notifications {
                &[Action::Notifications, Action::Clock, Action::QuickSettings]
            } else {
                &[Action::Clock, Action::QuickSettings]
            };
            assert_eq!(order.get(..expected.len()), Some(expected));
            assert_eq!(
                order
                    .iter()
                    .filter(|a| **a == Action::Notifications)
                    .count(),
                usize::from(notifications)
            );
        }
        // The clock target is not drawn: choosing one keeps the same look.
        let drawn = bar.look.clone();
        bar.set_clock_target(Some("ms-clock:"));
        assert_eq!(bar.clock_target.as_deref(), Some("ms-clock:"));
        assert!(bar.look == drawn);
        bar.set_clock_target(None);
        assert_eq!(bar.clock_target, None);
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic network states"]
    fn native_network_moments_wait_out_brief_drops_and_tell_once() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, false, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.cue = None;
        bar.online = None;
        let act = |bar: &mut MenuBar| bar.cue.take().map(|(act, _)| act);
        // The first reading is only the starting point.
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // A brief drop says nothing.
        bar.status.network = Network::Offline;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // Down for the grace period: told once.
        bar.status.network = Network::Offline;
        bar.watch_network();
        bar.offline_since = Some(Instant::now() - OFFLINE_GRACE);
        bar.watch_network();
        assert_eq!(act(&mut bar), Some(mascot::Act::Offline));
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // Back: relief, also once.
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), Some(mascot::Act::Online));
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // A hidden bar hands the moment over without a point (the dock's).
        bar.cue = Some((mascot::Act::Online, 10));
        assert!(bar.take_cue().is_some_and(|(_, point)| point.is_none()));
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic coverage changes"]
    fn native_intelligent_bar_hides_only_while_covered_and_reserves_nothing() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, false, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.set_visible(true)?;
        assert!(bar.reserved_strip().is_some());
        bar.set_smart_hide(true)?;
        // Intelligent: shown, but maximized windows get the whole screen.
        assert!(!bar.auto_hide && bar.reserved_strip().is_none());
        bar.set_covered(&gfx, true)?;
        assert!(bar.auto_hide);
        bar.set_covered(&gfx, false)?;
        assert!(!bar.auto_hide && !bar.revealed && bar.hide_until.is_none());
        // Automatically hide wins: coverage changes nothing.
        bar.set_auto_hide(true)?;
        bar.set_covered(&gfx, true)?;
        assert!(bar.auto_hide);
        bar.set_covered(&gfx, false)?;
        assert!(bar.auto_hide);
        // Back to intelligent while covered: hidden at once.
        bar.set_covered(&gfx, true)?;
        bar.set_auto_hide(false)?;
        assert!(bar.auto_hide);
        // Always visible: reserves its strip again and shows.
        bar.set_smart_hide(false)?;
        assert!(!bar.auto_hide && bar.reserved_strip().is_some());
        bar.set_visible(false)?;
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic monitor readings"]
    fn native_system_monitor_keeps_its_place_and_samples_only_while_shown() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let widgets = MenuBarWidgets {
            system_monitor: true,
            ..MenuBarWidgets::default()
        };
        let mut bar = MenuBar::create(&gfx, false, false, false, widgets)?;
        bar.set_visible(false)?;
        // Hidden, nothing is sampled.
        assert!(bar.monitor.is_none());
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let lines = |cpu: &str, down: &str| {
            [
                format!("CPU {cpu}"),
                "RAM 48%".to_owned(),
                "↑ 12 KB/s".to_owned(),
                format!("↓ {down}"),
            ]
        };
        let zone = |bar: &MenuBar| {
            bar.zones
                .iter()
                .find(|(_, _, action)| *action == Action::SystemMonitor)
                .map(|(left, right, _)| (*left, *right))
        };
        look.monitor = None;
        bar.apply_look(&gfx, look.clone())?;
        assert_eq!(zone(&bar), None);
        look.monitor = Some(lines("5%", "0 KB/s"));
        bar.apply_look(&gfx, look.clone())?;
        let narrow = zone(&bar).context("monitor zone")?;
        // The last zone pushed is the leftmost: the monitor sits left of
        // every status icon.
        assert_eq!(
            bar.zones.last().map(|(_, _, action)| *action),
            Some(Action::SystemMonitor)
        );
        look.monitor = Some(lines("100%", "35,5 MB/s"));
        bar.apply_look(&gfx, look.clone())?;
        let wide = zone(&bar).context("monitor zone")?;
        assert!(wide.0 < narrow.0 && wide.1 == narrow.1);
        // Narrower digits again: the columns keep their widest size, so the
        // rest of the bar does not shift back and forth.
        look.monitor = Some(lines("5%", "0 KB/s"));
        bar.apply_look(&gfx, look)?;
        assert_eq!(zone(&bar), Some(wide));
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic Bluetooth states, no hardware changes"]
    fn native_bluetooth_indicator_appears_only_for_an_enabled_system_radio() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.bluetooth = None;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for on in [false, true, false] {
            look.status.bluetooth_on = on;
            bar.status.bluetooth_on = on;
            bar.apply_look(&gfx, look.clone())?;
            assert_eq!(
                bar.zones
                    .iter()
                    .filter(|(_, _, action)| *action == Action::Bluetooth)
                    .count(),
                usize::from(on)
            );
        }
        // The native event handler updates the cached Look instead of running
        // provider/process scans or re-reading unrelated Windows preferences.
        look.status.bluetooth_on = true;
        bar.apply_look(&gfx, look)?;
        bar.bluetooth_changed(&gfx)?;
        assert!(
            !bar.zones
                .iter()
                .any(|(_, _, action)| *action == Action::Bluetooth)
        );
        Ok(())
    }

    #[test]
    #[ignore = "Native popup layout/input regression; hidden window, no audio writes"]
    fn native_popup_overflow_and_layout_changes_keep_input_in_sync() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        // SAFETY: All composition objects stay on this test's STA thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.audio = None;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: -1920,
            top: -768,
            right: 0,
            bottom: 0,
        };
        look.track = None;
        look.panel = Some(Island::Usage);
        look.usage_enabled = true;
        look.active_providers.0.fill(true);
        for provider in Provider::ALL {
            look.usage.providers[provider as usize] = ProviderUsage {
                state: State::Ready,
                windows: [300, 10_080]
                    .into_iter()
                    .map(|minutes| usage::Window {
                        label: None,
                        minutes,
                        used_percent: 30,
                        resets_at: None,
                    })
                    .collect(),
                fetched_at: Some(std::time::SystemTime::now()),
            };
        }
        let opencode = &mut look.usage.providers[Provider::OpenCode as usize];
        opencode.windows.push(usage::Window {
            label: None,
            minutes: 43_200,
            used_percent: 10,
            resets_at: None,
        });
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.5, 2.0] {
                look.scale = scale;
                for light in [false, true] {
                    look.light = light;
                    bar.usage_scroll = 0.0;
                    bar.build(&gfx, &look)?;
                    assert!(bar.visual_height <= 769, "popup must fit a 768 px display");
                    if scale == 1.0 {
                        assert!(bar.usage_viewport.is_none());
                        continue;
                    }
                    let viewport = bar
                        .usage_viewport
                        .as_ref()
                        .context("overflow needs a viewport")?;
                    let bounds = viewport.bounds;
                    let maximum = viewport.maximum;
                    assert!(maximum > 0.0);
                    let body = viewport.content.clone();
                    let down = viewport.down.clone();
                    let root = bar.surface.Children()?.First()?.Current()?;
                    for _ in 0..20 {
                        assert!(bar.scroll_usage(48.0)?);
                    }
                    assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
                    assert_eq!(bar.usage_scroll, maximum);
                    assert_eq!(body.Offset()?.Y, -(maximum * scale).round());
                    assert_eq!(down.Opacity()?, 0.22);
                    let up = bar
                        .island_hits
                        .iter()
                        .find(|hit| matches!(hit.action, IslandAction::ScrollUsage(-1)))
                        .context("scroll up button")?;
                    let (left, top, right, bottom) = up.bounds;
                    assert!(
                        left >= bounds.0
                            && top >= bounds.1
                            && right <= bounds.2
                            && bottom <= bounds.3
                    );
                    bar.click(&gfx, (left + right) / 2.0, (top + bottom) / 2.0)?;
                    assert!(bar.usage_scroll < maximum);
                    assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
                    assert!(bar.scroll_usage(-10_000.0)?);
                    assert_eq!(bar.usage_scroll, 0.0);
                }
            }
        }
        // A busy refresh is not another active input target.
        look.usage_refreshing = true;
        bar.build(&gfx, &look)?;
        assert!(
            !bar.island_hits
                .iter()
                .any(|hit| matches!(hit.action, IslandAction::Refresh))
        );
        look.usage_refreshing = false;
        bar.scroll_usage(10_000.0)?;
        look.active_providers.0.fill(false);
        look.active_providers.0[Provider::Codex as usize] = true;
        bar.build(&gfx, &look)?;
        assert_eq!(bar.usage_scroll, 0.0); // A refresh that removes rows clamps the offset.
        assert!(bar.usage_viewport.is_none());

        look.panel = Some(Island::Music);
        look.track = Some(Track::test_fixture());
        bar.panel = look.panel;
        bar.status.volume = Some(Volume {
            level: 2,
            muted: false,
            percent: 50,
        });
        look.status = bar.status;
        bar.apply_look(&gfx, look.clone())?;
        bar.apply_volume_reading(
            &gfx,
            Some(Volume {
                level: 0,
                muted: true,
                percent: 0,
            }),
        )?;
        // Mute/glyph changes render the current snapshot; a full refresh would
        // replace this monitor and scan live processes/system status again.
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|current| current.screen == look.screen
                    && current.status.volume.is_some_and(|volume| volume.muted))
        );
        let span = bar
            .island_hits
            .iter()
            .find_map(|hit| match hit.action {
                IslandAction::Volume(span) => Some(span),
                _ => None,
            })
            .context("slider")?;
        bar.slider_drag = Some(SliderDrag::VerticalVolume(span)); // Never capture the user's mouse in a test.
        bar.queue_slider(SliderDrag::VerticalVolume(span), span.center_x, span.top);
        look.scale = 1.5;
        bar.apply_look(&gfx, look.clone())?;
        assert!(
            bar.slider_drag.is_none()
                && bar.pending_volume.is_none()
                && bar.volume_target.is_none()
        );
        assert!(bar.release_volume(Some((span.center_x, span.bottom)))?); // Late up must not activate another button.
        let span = bar
            .island_hits
            .iter()
            .find_map(|hit| match hit.action {
                IslandAction::Volume(span) => Some(span),
                _ => None,
            })
            .context("new slider")?;
        bar.slider_drag = Some(SliderDrag::VerticalVolume(span));
        bar.queue_slider(SliderDrag::VerticalVolume(span), span.center_x, span.bottom);
        // Move the capsule by changing its anchor style, without changing the
        // monitor/DPI. A provider count can now leave the capsule in place.
        look.widgets.island_style = IslandStyle::Notch;
        bar.apply_look(&gfx, look.clone())?;
        assert!(bar.slider_drag.is_none() && bar.pending_volume.is_none());

        // A trigger that cannot fit must not leave a latent open popup.
        look.screen.right = look.screen.left + 160;
        bar.apply_look(&gfx, look.clone())?;
        assert!(bar.panel.is_none() && bar.look.as_ref().is_some_and(|look| look.panel.is_none()));
        let mut wide = bar.look.clone().context("crowded look")?;
        wide.screen.right = wide.screen.left + 1920;
        bar.apply_look(&gfx, wide)?;
        assert!(bar.panel_span.width() <= 0.0 && bar.usage_viewport.is_none());
        Ok(())
    }

    #[test]
    fn vertical_volume_coordinates_increase_upwards_and_clamp_at_both_ends() {
        let span = VolumeTrack {
            center_x: 200.0,
            top: 100.0,
            bottom: 300.0,
        };
        for (y, expected) in [
            (-100.0, 100),
            (100.0, 100),
            (200.0, 50),
            (299.0, 1),
            (300.0, 0),
            (500.0, 0),
            (f32::NAN, 0),
            (f32::INFINITY, 0),
        ] {
            assert_eq!(volume_percent(span, y), expected);
        }
        assert_eq!(volume_percent(VolumeTrack::default(), 1.0), 0);
    }

    #[test]
    fn volume_capsule_stays_beside_the_player_and_inside_each_screen_edge() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (left, right, width) in [
                (12.0, 372.0, 800.0),
                (428.0, 788.0, 800.0),
                (220.0, 580.0, 800.0),
                (12.0, 380.0, 392.0),
            ] {
                let (music, capsule) = volume_popover_layout(
                    Span {
                        left: left * scale,
                        right: right * scale,
                    },
                    width * scale,
                    scale,
                );
                assert!(music.left >= 0.0 && capsule.left >= 0.0);
                assert!(music.right <= width * scale && capsule.right <= width * scale);
                assert!(
                    music.right < capsule.left || capsule.right < music.left,
                    "never cover playback controls"
                );
                assert!(music.width() > capsule.width());
            }
        }
        let panel = Span {
            left: 428.0,
            right: 788.0,
        };
        let (music, capsule) = volume_popover_layout(panel, 800.0, 1.0);
        assert!(music == panel && capsule.right < music.left);
    }

    #[test]
    fn playback_timestamps_include_hours_without_affecting_short_tracks() {
        for (seconds, expected) in [
            (0, "0:00"),
            (65, "1:05"),
            (3599, "59:59"),
            (3600, "1:00:00"),
            (3661, "1:01:01"),
        ] {
            assert_eq!(media_time(seconds), expected);
        }
    }

    #[test]
    fn percentage_changes_keep_the_same_indicator_but_mute_and_device_changes_do_not() {
        let volume = Volume {
            level: 2,
            muted: false,
            percent: 40,
        };
        assert!(same_volume_indicator(
            Some(volume),
            Some(Volume {
                percent: 60,
                ..volume
            })
        ));
        assert!(!same_volume_indicator(
            Some(volume),
            Some(Volume {
                muted: true,
                ..volume
            })
        ));
        assert!(!same_volume_indicator(
            Some(volume),
            Some(Volume { level: 3, ..volume })
        ));
        assert!(!same_volume_indicator(Some(volume), None));
        assert!(!same_volume_indicator(None, Some(volume)));
    }

    #[test]
    #[ignore = "Native compositor coverage; hidden test window, no audio writes"]
    fn native_music_artwork_and_volume_fit_both_styles_and_release_drag_state() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.audio = None; // This fixture can never change the user's audio.
        bar.status.volume = Some(Volume {
            level: 2,
            muted: false,
            percent: 42,
        });
        let mut look = bar.look.clone().context("initial look")?;
        look.status = bar.status;
        look.panel = Some(Island::Music);
        look.track = Some(Track::test_fixture());
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.25, 1.5, 2.0] {
                look.scale = scale;
                for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
                    look.light = light;
                    look.glass = glass;
                    bar.build(&gfx, &look)?;
                    assert!(bar.cover.is_some());
                    assert!(bar.progress_controls.is_some());
                    let capsule = bar
                        .volume_controls
                        .as_ref()
                        .context("volume capsule should be visible with the player")?
                        .geometry;
                    let progress = bar.progress_controls.as_ref().context("progress")?;
                    assert_eq!(
                        progress.width,
                        progress.geometry.width - progress.geometry.px(52.0),
                        "the progress track keeps only a balanced right inset"
                    );
                    assert!(
                        progress.geometry.left + progress.geometry.width <= capsule.left
                            || progress.geometry.left >= capsule.left + capsule.width,
                        "the volume capsule stays beside the player"
                    );
                    let brush = bar.cover.as_ref().context("missing cover")?.1.clone();
                    bar.build(&gfx, &look)?;
                    assert_eq!(bar.cover.as_ref().context("cached cover")?.1, brush);
                    let volume = bar
                        .island_hits
                        .iter()
                        .find(|hit| matches!(hit.action, IslandAction::Volume(_)))
                        .context("missing volume hit")?;
                    let (left, top, right, bottom) = volume.bounds;
                    assert!(left >= bar.panel_span.left && right <= bar.panel_span.right);
                    assert!(
                        top >= HEIGHT * scale
                            && bottom <= (HEIGHT + PANEL_GAP + look.panel_height()) * scale + 1.0
                    );
                    let geometry = bar
                        .volume_controls
                        .as_ref()
                        .context("volume controls")?
                        .geometry;
                    // Click the visible speaker's centre, not merely somewhere
                    // inside an independently specified hit rectangle.
                    let speaker = bar
                        .island_hits
                        .iter()
                        .find(|hit| {
                            hit.at(
                                geometry.left + geometry.width / 2.0,
                                geometry.top + geometry.px(155.0),
                            )
                        })
                        .context("speaker has no hit target")?;
                    assert!(matches!(speaker.action, IslandAction::Mute));
                    let slider = bar
                        .island_hits
                        .iter()
                        .find(|hit| hit.at((left + right) / 2.0, geometry.top + geometry.px(82.0)))
                        .context("slider has no hit target")?;
                    assert!(matches!(slider.action, IslandAction::Volume(_)));
                }
            }
        }
        // Commit the synthetic player to input/cache state after the layout
        // matrix. build() alone does not update the open panel or cached Look.
        bar.apply_look(&gfx, look.clone())?;
        let image = Arc::new(nosih_shell::icons::IconImage {
            width: 8,
            height: 8,
            bgra: [20, 40, 80, 255].repeat(64),
        });
        look.track.as_mut().context("track")?.artwork = Some(image.clone());
        bar.build(&gfx, &look)?;
        assert!(
            bar.cover
                .as_ref()
                .and_then(|(source, _)| source.as_ref())
                .is_some_and(|source| Arc::ptr_eq(source, &image))
        );
        // Compare the old full redraw path with retained updates without ever
        // writing the real audio endpoint. Tree identity is the regression
        // assertion; timings are informational, not a flaky speed threshold.
        let started = Instant::now();
        for step in 0..64 {
            bar.status.volume = Some(Volume {
                level: 2,
                muted: false,
                percent: 40 + step % 20,
            });
            look.status = bar.status;
            bar.build(&gfx, &look)?;
        }
        let full_redraw = started.elapsed();
        bar.look = Some(look.clone());
        let root = bar.surface.Children()?.First()?.Current()?;
        let mut update = look.track.clone().context("track for progress update")?;
        update.timeline = Track::test_fixture().timeline;
        bar.media = Some(media::Watch::test_snapshot(update));
        bar.media_changed(&gfx)?;
        assert_eq!(
            bar.surface.Children()?.First()?.Current()?,
            root,
            "position events retain the visual tree"
        );
        bar.visible = true; // The native window stays hidden throughout the fixture.
        bar.revealed = true;
        bar.sync_progress_watch();
        assert!(bar.progress_timer);
        bar.visible = false;
        bar.sync_progress_watch();
        assert!(
            !bar.progress_timer,
            "hidden popups must have no progress timer"
        );
        let started = Instant::now();
        for step in 0..64 {
            bar.apply_volume_reading(
                &gfx,
                Some(Volume {
                    level: 2,
                    muted: false,
                    percent: 40 + step % 20,
                }),
            )?;
            assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
            assert_eq!(
                bar.volume_controls
                    .as_ref()
                    .context("retained volume")?
                    .label_percent,
                40 + step % 20
            );
        }
        eprintln!(
            "64 volume updates: full redraw {:?}, retained {:?} (no audio writes)",
            full_redraw,
            started.elapsed()
        );
        let span = VolumeTrack {
            center_x: 200.0,
            top: 100.0,
            bottom: 300.0,
        };
        bar.slider_drag = Some(SliderDrag::VerticalVolume(span)); // No SetCapture in a test.
        bar.queue_slider(SliderDrag::VerticalVolume(span), span.center_x, 260.0);
        bar.pointer_moved(&gfx, 999.0, 140.0)?;
        let player = bar.surface.Children()?.First()?.Current()?;
        bar.refresh_backdrops(&gfx)?;
        assert_eq!(bar.surface.Children()?.First()?.Current()?, player);
        assert!(bar.slider_drag.is_some());
        assert_eq!(bar.pending_volume, Some(80));
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("immediate percentage")?
                .label_percent,
            80
        );
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("immediate fill")?
                .position_percent,
            Some(80)
        );
        let controls = bar.volume_controls.as_ref().context("vertical fill")?;
        assert!((controls.fill.Size()?.Y - controls.height * 0.8).abs() < 0.01);
        assert!(
            (controls.fill.Offset()?.Y + controls.fill.Size()?.Y - controls.height).abs() < 0.01,
            "fill stays attached to the bottom"
        );
        let label = controls.value.Size()?;
        assert!(
            (controls.value.Offset()?.X + label.X / 2.0 - controls.geometry.width / 2.0).abs()
                < 0.01
        );
        // An endpoint acknowledgement must not drag the fill backwards while
        // a newer pointer target is still waiting for its coalesced write.
        bar.apply_volume_reading(
            &gfx,
            Some(Volume {
                level: 2,
                muted: false,
                percent: 50,
            }),
        )?;
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("optimistic fill")?
                .position_percent,
            Some(80)
        );
        assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
        assert!(bar.release_volume(Some((span.center_x, 300.0)))?);
        assert!(bar.pending_volume.is_none() && bar.slider_drag.is_none());
        assert!(!bar.release_volume(None)?);
        assert!(bar.release_volume(Some((span.center_x, 200.0)))?); // Late up after capture loss/timer.
        assert!(!bar.release_volume(Some((span.center_x, 200.0)))?);
        bar.status.volume = None;
        look.status = bar.status;
        // Wallpaper/DWM changes renew only the host sample. Cached content and
        // the open music popup remain intact when the theme itself is unchanged.
        bar.look = Some(look.clone());
        let previous_material = bar.surface.Children()?.First()?.Current()?;
        bar.refresh_backdrops(&gfx)?;
        assert!(bar.look.as_ref() == Some(&look));
        bar.apply_look(&gfx, look.clone())?;
        assert_eq!(
            bar.surface.Children()?.First()?.Current()?,
            previous_material
        );
        bar.build(&gfx, &look)?;
        assert!(
            !bar.island_hits
                .iter()
                .any(|hit| matches!(hit.action, IslandAction::Volume(_) | IslandAction::Mute))
        );
        look.track.as_mut().context("track")?.timeline = None;
        bar.apply_look(&gfx, look)?;
        assert!(bar.progress_controls.is_none() && !bar.progress_timer);
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows composition; hidden window and no provider requests"]
    fn opencode_only_measured_go_appears_in_both_island_styles() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        // SAFETY: The dispatcher and hidden composition objects belong to this thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.track = None;
        look.active_providers = ActiveProviders::default();
        look.active_providers.0[Provider::OpenCode as usize] = true;
        // Yesterday's saved quota is not evidence that Codex is running now.
        look.usage.providers[Provider::Codex as usize] = ProviderUsage {
            state: State::Ready,
            windows: vec![usage::Window {
                label: None,
                minutes: 300,
                used_percent: 42,
                resets_at: None,
            }],
            fetched_at: Some(std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60)),
        };
        let quota: Vec<_> = [300, 10_080, 43_200]
            .into_iter()
            .map(|minutes| usage::Window {
                label: None,
                minutes,
                used_percent: 15,
                resets_at: Some(chrono::Utc::now() + chrono::Duration::hours(2)),
            })
            .collect();
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.25, 1.5, 2.0] {
                look.scale = scale;
                for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
                    look.light = light;
                    look.glass = glass;
                    look.panel = Some(Island::Usage);
                    look.usage_enabled = true;
                    look.usage.providers[Provider::OpenCode as usize] = ProviderUsage::default();
                    bar.apply_look(&gfx, look.clone())?;
                    assert!(bar.island_hits.is_empty() && bar.panel.is_none());
                    assert_eq!(bar.triggers_span.width(), 0.0);
                    look.usage.providers[Provider::OpenCode as usize] = ProviderUsage {
                        state: State::Ready,
                        windows: quota.clone(),
                        fetched_at: Some(std::time::SystemTime::now()),
                    };
                    bar.apply_look(&gfx, look.clone())?;
                    let current = bar.look.as_ref().context("rendered Go")?;
                    let providers = island_providers(current);
                    assert_eq!(providers.len(), 1);
                    assert!(providers.iter().all(|entry| entry.0 == Provider::OpenCode));
                    assert_eq!(
                        current.panel_content_height(),
                        74.0 + ISLAND_ROW_HEIGHT + 26.0
                    );
                    assert!(bar.panel_span.width() > 0.0);
                    assert!(
                        bar.island_hits
                            .iter()
                            .any(|hit| matches!(hit.action, IslandAction::Toggle(Island::Usage)))
                    );
                    assert!(
                        bar.island_hits
                            .iter()
                            .any(|hit| matches!(hit.action, IslandAction::Refresh))
                    );
                }
            }
        }
        // Removing Go credentials/quota closes its open popup and removes the trigger.
        look.usage.providers[Provider::OpenCode as usize] = ProviderUsage {
            state: State::SignIn,
            ..ProviderUsage::default()
        };
        bar.apply_look(&gfx, look)?;
        assert!(bar.panel.is_none() && bar.panel_span.width() == 0.0);
        assert!(bar.island_hits.is_empty() && bar.usage_viewport.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; keeps the bar hidden"]
    fn accessory_opt_out_blocks_startup_resume_and_retry() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        assert!(!bar.accessories_enabled && bar.accessories.is_none());
        bar.restart_accessories(&gfx)?;
        bar.tick(&gfx)?;
        assert!(bar.accessories.is_none());
        bar.set_accessories_enabled(&gfx, true)?;
        assert!(bar.accessories.is_some());
        bar.set_accessories_enabled(&gfx, false)?;
        bar.accessory_retry_at = Instant::now() - Duration::from_secs(1);
        bar.restart_accessories(&gfx)?;
        bar.tick(&gfx)?;
        assert!(!bar.accessories_enabled && bar.accessories.is_none());
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|look| look.accessories == [None, None])
        );
        Ok(())
    }

    #[test]
    fn accessory_status_ignores_hidden_metadata_and_hides_incomplete_readings() {
        use nosih_shell::accessories::{Device, State};
        let mut snapshot = AccessorySnapshot {
            state: State::Ready,
            ..Default::default()
        };
        assert_eq!(accessory_readings(&snapshot), [None, None]);
        snapshot.devices = vec![
            Device {
                id: "mouse".into(),
                name: "Mouse".into(),
                kind: AccessoryKind::Mouse,
                percent: Some(0),
                conflicting: false,
                active_audio: false,
            },
            Device {
                id: "headset".into(),
                name: "Headset".into(),
                kind: AccessoryKind::Headset,
                percent: None,
                conflicting: false,
                active_audio: true,
            },
        ];
        let before = accessory_readings(&snapshot);
        assert!(before[0].is_some_and(|reading| reading.percent == Some(0)));
        assert!(before[1].is_some_and(|reading| reading.percent.is_none()));
        snapshot.devices[0].name = "New name".into();
        assert!(accessory_readings(&snapshot) == before);
        for state in [State::Loading, State::Unavailable] {
            snapshot.state = state;
            assert_eq!(accessory_readings(&snapshot), [None, None]);
        }
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; keeps the test bar hidden"]
    fn accessory_indicators_render_without_controls_or_popups() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.accessories = None;
        let mut look = bar.look.clone().context("missing initial menu bar")?;
        look.track = None;
        for (scale, material) in [1.0, 1.25, 1.5, 2.0].into_iter().zip(Material::ALL) {
            for light in [true, false] {
                for glass in [true, false] {
                    look.scale = scale;
                    look.material = material;
                    look.light = light;
                    look.glass = glass;
                    look.accessories = [None, None];
                    bar.build(&gfx, &look)?;
                    let root: ContainerVisual =
                        bar.surface.Children()?.First()?.Current()?.cast()?;
                    let empty_visuals = root.Children()?.Count()?;
                    let controls = bar.zones.len();
                    look.accessories = [
                        Some(AccessoryReading {
                            kind: AccessoryKind::Mouse,
                            percent: Some(76),
                        }),
                        Some(AccessoryReading {
                            kind: AccessoryKind::Headset,
                            percent: Some(58),
                        }),
                    ];
                    bar.build(&gfx, &look)?;
                    let root: ContainerVisual =
                        bar.surface.Children()?.First()?.Current()?.cast()?;
                    assert_eq!(root.Children()?.Count()?, empty_visuals + 4);
                    assert_eq!(bar.zones.len(), controls);
                    assert!(bar.island_hits.is_empty());
                    assert!(bar.outside_click_hook.is_none());
                    assert_eq!(bar.panel, None);
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn changing_ui_language_redraws_unchanged_menu_bar_content() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        struct RestoreLanguage(Language);
        impl Drop for RestoreLanguage {
            fn drop(&mut self) {
                strings::set_language(self.0);
            }
        }
        let _restore = RestoreLanguage(strings::language());
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        strings::set_language(Language::Es);
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|look| look.language == Language::Es)
        );
        for language in [
            Language::En,
            Language::Fr,
            Language::Zh,
            Language::Hi,
            Language::Ar,
            Language::Bn,
            Language::Pt,
            Language::Es,
        ] {
            let previous = bar.surface.Children()?.First()?.Current()?;
            strings::set_language(language);
            bar.refresh(&gfx)?;
            assert_ne!(
                previous,
                bar.surface.Children()?.First()?.Current()?,
                "translation-only changes must rebuild the content"
            );
            assert!(
                bar.look
                    .as_ref()
                    .is_some_and(|look| look.language == language)
            );
            // Exercise Windows shaping/font fallback and compact surface bounds,
            // including mixed Arabic + Latin text and Hindi/Bengali conjuncts.
            for text in [
                strings::text().usage_title,
                strings::text().media_title,
                strings::text().usage_claude_sign_in,
            ] {
                let label = gfx.text_brush_fitted(text, 1.0, 13.0, false, Ink::Dark, 160.0)?;
                assert!(label.width > 0.0 && label.width <= 160.0);
                assert!(label.height > 0.0 && label.height < 100.0);
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn auto_hidden_bar_can_reveal_and_hide_repeatedly() -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::IsWindowVisible,
        };
        // SAFETY: The dispatcher and bar belong to this test thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        assert!(
            !unsafe { IsWindowVisible(bar.hwnd) }.as_bool(),
            "auto-hide must apply before the first frame"
        );
        bar.set_auto_hide(true)?;
        for _ in 0..3 {
            assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            assert_eq!(
                bar.surface.Opacity()?,
                0.0,
                "hidden content must stay transparent"
            );
            let point = POINT {
                x: (bar.bounds.left + bar.bounds.right) / 2,
                y: bar.bounds.top,
            };
            bar.edge_rested(&gfx, point)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            // A queued leave from before the reveal must not hide it while
            // the pointer is still at the edge, including its first pixel.
            bar.pointer_left_at(&gfx, point)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            let surface = bar.surface.clone();
            bar.set_app_name(&gfx, "Nosih regression test")?;
            assert_eq!(
                bar.surface, surface,
                "redraws must preserve the animation parent"
            );
            let outside = POINT {
                y: bar.bounds.bottom,
                ..point
            };
            bar.pointer_left_at(&gfx, outside)?;
            if bar.hide_until.is_some() {
                // Returning during the exit cancels it. Its already queued
                // completion must not close the bar after that reversal.
                bar.pointer_left_at(&gfx, point)?;
                let old_epoch = bar.animation_epoch;
                bar.finish_hide_at(
                    &gfx,
                    Instant::now() + Duration::from_secs(1),
                    outside,
                    Some(old_epoch.wrapping_sub(1)),
                )?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                bar.pointer_left_at(&gfx, outside)?;
                // An old completion must also be ignored during a newer exit.
                bar.finish_hide_at(
                    &gfx,
                    Instant::now() + Duration::from_secs(1),
                    outside,
                    Some(old_epoch),
                )?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                let epoch = bar.animation_epoch;
                bar.finish_hide_at(&gfx, Instant::now(), outside, None)?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                bar.finish_hide_at(&gfx, Instant::now(), outside, Some(epoch))?;
            }
        }
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.set_auto_hide(false)?;
        assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.set_auto_hide(true)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.edge_rested(
            &gfx,
            POINT {
                x: bar.bounds.right / 2,
                y: bar.bounds.top,
            },
        )?;
        bar.set_visible(false)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        assert!(bar.hide_until.is_none());
        bar.set_visible(true)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; runs visible transitions"]
    fn compositor_completion_hides_only_the_current_exit() -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::{
                DispatchMessageW, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
            },
        };
        // SAFETY: The dispatcher, queue, window and compositor are test-owned.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_app_name(&gfx, "Nosih animation test")?;
        bar.set_auto_hide(true)?;
        let inside = POINT {
            x: bar.bounds.left + 100,
            y: bar.bounds.top,
        };
        let outside = POINT {
            y: bar.bounds.bottom + 50,
            ..inside
        };
        let pump = |bar: &mut MenuBar, duration: Duration, point: POINT| -> Result<usize> {
            let until = Instant::now() + duration;
            let mut completions = 0;
            let mut msg = MSG::default();
            while Instant::now() < until {
                // Pump the dispatcher so real compositor completion events run.
                // Pointer state is deterministic; no mouse input is injected.
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                    if msg.hwnd == bar.hwnd && msg.message == HIDE_ANIMATION_DONE {
                        completions += 1;
                        bar.finish_hide_at(&gfx, Instant::now(), point, Some(msg.wParam.0))?;
                    } else if msg.hwnd == bar.hwnd
                        && msg.message == WM_TIMER
                        && msg.wParam.0 == HIDE_ANIMATION_TIMER
                    {
                        bar.finish_hide_at(&gfx, Instant::now(), point, None)?;
                    } else {
                        unsafe { DispatchMessageW(&msg) };
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(completions)
        };
        pump(&mut bar, Duration::from_millis(300), outside)?;
        for _ in 0..3 {
            bar.hover(&gfx, true)?;
            pump(&mut bar, Duration::from_millis(500), inside)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            bar.hover(&gfx, false)?;
            if system::animations_enabled() {
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            }
            let completions = pump(&mut bar, Duration::from_millis(500), outside)?;
            if system::animations_enabled() {
                assert_eq!(completions, 1, "exit must complete through the compositor");
            }
            assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            assert_eq!(bar.surface.Opacity()?, 0.0);
        }
        Ok(())
    }

    #[test]
    fn clicking_usage_replaces_music_and_clicking_again_closes_it() {
        let open = toggle_panel(Some(Island::Music), Island::Usage);
        assert_eq!(open, Some(Island::Usage));
        assert_eq!(toggle_panel(open, Island::Usage), None);
    }

    #[test]
    fn usage_panel_covers_the_last_provider_even_when_the_trigger_is_wide() {
        let screen = 1920.0;
        let triggers = trigger_layout(
            Span {
                left: 210.0,
                right: 1540.0,
            },
            screen / 2.0,
            1.25,
            true,
            5,
        );
        let Some(usage) = triggers[1].1 else {
            panic!("Usage trigger should fit");
        };
        let panel = panel_span(usage, screen, 550.0);
        assert!(panel.left <= usage.left && panel.right >= usage.right);
    }

    #[test]
    fn narrow_bar_still_gives_each_trigger_its_own_clickable_span() {
        let bounds = Span {
            left: 200.0,
            right: 390.0,
        };
        let [(_, Some(music)), (_, Some(usage))] = trigger_layout(bounds, 300.0, 1.5, true, 3)
        else {
            panic!("Both triggers should fit");
        };
        assert!(
            music.left >= bounds.left && music.right < usage.left && usage.right <= bounds.right
        );
    }

    #[test]
    fn bubble_layout_aligns_to_right_status_edge_and_stays_compact() {
        let bounds = Span {
            left: 210.0,
            right: 1420.0,
        };
        let [(_, Some(short_music)), (_, Some(short_usage))] =
            bubble_trigger_layout(bounds, 1.0, Some(92.0), 2)
        else {
            panic!("Both bubble segments should fit");
        };
        let [(_, Some(long_music)), (_, Some(long_usage))] =
            bubble_trigger_layout(bounds, 1.0, Some(240.0), 2)
        else {
            panic!("Both bubble segments should fit");
        };
        assert_eq!(short_usage.right, bounds.right);
        assert_eq!(long_usage.right, bounds.right);
        assert!(short_music.width() < long_music.width());
        assert_eq!(long_music.width(), BUBBLE_MUSIC_WIDTH);
        assert!(short_music.left >= bounds.left && short_music.right < short_usage.left);

        let short_bubble = Span {
            left: short_music.left,
            right: short_usage.right,
        };
        let long_bubble = Span {
            left: long_music.left,
            right: long_usage.right,
        };
        let short_panel = bubble_panel_span(short_bubble, 1920.0, 360.0);
        let long_panel = bubble_panel_span(long_bubble, 1920.0, 360.0);
        assert_eq!(short_panel.left, long_panel.left);
        assert_eq!(short_panel.right, long_panel.right);
    }

    #[test]
    fn losing_a_session_closes_its_panel_without_opening_the_other() {
        assert_eq!(available_panel(Some(Island::Music), false, true), None);
        assert_eq!(available_panel(Some(Island::Usage), true, false), None);
        assert_eq!(
            available_panel(Some(Island::ControlCenter), false, false),
            Some(Island::ControlCenter)
        );
    }

    #[test]
    fn control_center_slider_percentage_clamps_and_rounds_cleanly() {
        let track = HorizontalTrack {
            left: 100.0,
            right: 300.0,
        };
        assert_eq!(horizontal_percent(track, 50.0), 0);
        assert_eq!(horizontal_percent(track, 100.0), 0);
        assert_eq!(horizontal_percent(track, 200.0), 50);
        assert_eq!(horizontal_percent(track, 300.0), 100);
        assert_eq!(horizontal_percent(track, 450.0), 100);
        assert_eq!(horizontal_percent(track, f32::NAN), 0);
    }

    #[test]
    fn control_center_height_expands_for_now_playing_and_toggles_cleanly() {
        assert_eq!(control_center_height(false), 320.0);
        assert_eq!(control_center_height(true), 388.0);
        let opened = toggle_panel(None, Island::ControlCenter);
        assert_eq!(opened, Some(Island::ControlCenter));
        assert_eq!(toggle_panel(opened, Island::ControlCenter), None);
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn popup_surface_builds_in_both_themes_with_and_without_transparency() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };

        // SAFETY: The controller initializes the test thread's own queue.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
            let root = gfx.compositor.CreateContainerVisual()?;
            let theme = Theme::for_popup(light, glass);
            let geometry = IslandGeometry {
                left: 90.0,
                top: HEIGHT + PANEL_GAP,
                width: 360.0,
                scale: 1.0,
                light,
            };
            popup_surface(
                &gfx,
                theme,
                &root.Children()?,
                geometry,
                136.0,
                geometry.px(PANEL_RADIUS),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod mascot_tests {
    use super::*;
    use crate::usage::{ProviderUsage, Snapshot, State, Window};

    fn at(percent: Option<u8>) -> Snapshot {
        let mut snapshot = Snapshot::default();
        if let Some(percent) = percent {
            snapshot.providers[Provider::Claude as usize] = ProviderUsage {
                state: State::Ready,
                windows: vec![Window {
                    label: None,
                    minutes: 300,
                    used_percent: percent,
                    resets_at: None,
                }],
                fetched_at: None,
            };
        }
        snapshot
    }

    fn moment(before: Option<u8>, now: Option<u8>) -> Option<mascot::Act> {
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        usage_moment(&at(before), &at(now), &active).map(|(_, act)| act)
    }

    #[test]
    fn usage_moment_should_fire_only_when_a_line_is_crossed() {
        assert_eq!(moment(Some(60), Some(0)), Some(mascot::Act::Fresh));
        assert_eq!(moment(Some(85), Some(91)), Some(mascot::Act::Nervous));
        assert_eq!(moment(Some(95), Some(100)), Some(mascot::Act::Exhausted));
        // Straight to 100 is "exhausted", not "nervous".
        assert_eq!(moment(Some(50), Some(100)), Some(mascot::Act::Exhausted));
        // Staying above a line, a small reset, or the first figure: nothing.
        assert_eq!(moment(Some(92), Some(95)), None);
        assert_eq!(moment(Some(10), Some(0)), None);
        assert_eq!(moment(None, Some(0)), None);
        assert_eq!(moment(None, Some(100)), None);
    }

    #[test]
    fn usage_moment_ignores_failed_reads_and_different_quota_windows() {
        let before = at(Some(95));
        let mut after = at(Some(0));
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        after.providers[Provider::Claude as usize].windows[0].minutes = 10080;
        assert!(usage_moment(&before, &after, &active).is_none());
        after.providers[Provider::Claude as usize].windows[0].minutes = 300;
        after.providers[Provider::Claude as usize].state = State::Unavailable;
        assert!(usage_moment(&before, &after, &active).is_none());
    }

    #[test]
    fn usage_moment_ignores_out_of_order_measurements() {
        let mut before = at(Some(95));
        let mut after = at(Some(0));
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        let now = std::time::SystemTime::now();
        before.providers[Provider::Claude as usize].fetched_at = Some(now);
        after.providers[Provider::Claude as usize].fetched_at =
            now.checked_sub(std::time::Duration::from_secs(60));
        assert!(usage_moment(&before, &after, &active).is_none());
    }
}
