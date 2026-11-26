//! Native macOS-style Notification Center store, calendar grid, and Windows
//! toast notification bridge.
//!
//! Replaces Windows' normal notification flyout and toast popups with Nosih's
//! own DirectComposition glass banners and Notification Center panel.

use std::{
    collections::HashSet,
    os::windows::process::CommandExt,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicIsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use chrono::{Datelike, Local, NaiveDate};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_ITEMS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Network,
    Bluetooth,
    App,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub app: String,
    pub title: String,
    pub body: String,
    pub timestamp: String,
    pub category: Category,
}

/// Snapshot rendered by `MenuBar` for both the Notification Center panel and
/// auto-popping glass notification banners.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub banner: Option<Item>,
}

pub struct Center {
    next_id: u64,
    items: Vec<Item>,
    banner: Option<Item>,
    seen_windows_ids: HashSet<u32>,
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Center {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: 1,
            items: Vec::new(),
            banner: None,
            seen_windows_ids: HashSet::new(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            items: self.items.clone(),
            banner: self.banner.clone(),
        }
    }

    /// Records a notification and, unless Do Not Disturb is active, marks it as
    /// the active pop-up banner. Returns `true` when a banner should pop up.
    pub fn push(
        &mut self,
        app: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        timestamp: impl Into<String>,
        category: Category,
        do_not_disturb: bool,
    ) -> bool {
        let app = app.into();
        let title = title.into();
        let body = body.into();
        // Avoid duplicating an identical consecutive notification.
        if let Some(latest) = self.items.first()
            && latest.app == app
            && latest.title == title
            && latest.body == body
            && latest.category == category
        {
            return false;
        }
        let item = Item {
            id: self.next_id,
            app,
            title,
            body,
            timestamp: timestamp.into(),
            category,
        };
        self.next_id = self.next_id.wrapping_add(1);
        self.items.insert(0, item.clone());
        self.items.truncate(MAX_ITEMS);
        if do_not_disturb {
            false
        } else {
            self.banner = Some(item);
            true
        }
    }

    /// Merges notifications fetched from Windows' `UserNotificationListener`.
    /// Returns `true` if a new Windows toast arrived and should pop up.
    pub fn merge_windows(&mut self, incoming: Vec<WindowsToast>, do_not_disturb: bool) -> bool {
        let mut newest_banner: Option<Item> = None;
        // Iterate in reverse so newest-first `incoming` preserves newest-first order in `items`.
        for toast in incoming.into_iter().rev() {
            if !self.seen_windows_ids.insert(toast.windows_id) {
                continue;
            }
            let item = Item {
                id: self.next_id,
                app: toast.app,
                title: toast.title,
                body: toast.body,
                timestamp: toast.timestamp,
                category: Category::App,
            };
            self.next_id = self.next_id.wrapping_add(1);
            self.items.insert(0, item.clone());
            self.items.truncate(MAX_ITEMS);
            newest_banner = Some(item);
        }
        if !do_not_disturb && let Some(banner) = newest_banner {
            self.banner = Some(banner);
            return true;
        }
        false
    }

    pub fn dismiss(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
        if self.banner.as_ref().is_some_and(|banner| banner.id == id) {
            self.banner = None;
        }
    }

    pub fn dismiss_banner(&mut self) -> bool {
        self.banner.take().is_some()
    }

    pub fn clear_all(&mut self) {
        self.items.clear();
        self.banner = None;
    }
}

/* ----------------------------------------------------- Compact Calendar */

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarMonth {
    pub title: String,
    pub today: u32,
    /// 0 = Sunday .. 6 = Saturday.
    pub first_weekday: usize,
    pub days_in_month: u32,
}

#[must_use]
pub fn current_month() -> CalendarMonth {
    let today = Local::now().date_naive();
    calendar_for(today.year(), today.month(), today.day())
}

fn calendar_for(year: i32, month: u32, today: u32) -> CalendarMonth {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(NaiveDate::MIN);
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    let days_in_month = next_month
        .map_or(30, |next| (next - first).num_days().clamp(28, 31) as u32);
    let first_weekday = first.weekday().num_days_from_sunday() as usize;
    let month_name = MONTHS
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("");
    CalendarMonth {
        title: format!("{month_name} {year}"),
        today,
        first_weekday,
        days_in_month,
    }
}

/* ----------------------------------------- Background Windows Toast Sync */

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsToast {
    pub windows_id: u32,
    pub app: String,
    pub title: String,
    pub body: String,
    pub timestamp: String,
}

pub struct Watcher {
    window: Arc<AtomicIsize>,
    inbox: Arc<Mutex<Vec<WindowsToast>>>,
    wake: SyncSender<()>,
}

impl Watcher {
    pub fn start(hwnd: HWND, message: u32) -> Option<Self> {
        let window = Arc::new(AtomicIsize::new(hwnd.0 as isize));
        let inbox = Arc::new(Mutex::new(Vec::new()));
        let (wake, receiver) = mpsc::sync_channel(2);
        {
            let window = window.clone();
            let inbox = inbox.clone();
            thread::Builder::new()
                .name("nosih-notifications".into())
                .spawn(move || run(&window, message, &inbox, &receiver))
                .ok()?;
        }
        Some(Self {
            window,
            inbox,
            wake,
        })
    }

    pub fn refresh(&self) {
        let _ = self.wake.try_send(());
    }

    pub fn take_incoming(&self) -> Vec<WindowsToast> {
        self.inbox
            .lock()
            .map_or_else(|_| Vec::new(), |mut guard| std::mem::take(&mut *guard))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.window.store(0, Ordering::Release);
    }
}

fn run(
    window: &AtomicIsize,
    message: u32,
    inbox: &Mutex<Vec<WindowsToast>>,
    receiver: &Receiver<()>,
) {
    while receiver.recv().is_ok() {
        if window.load(Ordering::Acquire) == 0 {
            break;
        }
        let fetched = query_windows_toasts();
        if fetched.is_empty() {
            continue;
        }
        if let Ok(mut guard) = inbox.lock() {
            guard.extend(fetched);
        }
        let hwnd = window.load(Ordering::Acquire);
        if hwnd != 0 {
            // SAFETY: Posts a payload-free notification to the live MenuBar window.
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Queries Windows Action Center toast notifications when the OS permits
/// `UserNotificationListener` access; returns empty cleanly if restricted.
fn query_windows_toasts() -> Vec<WindowsToast> {
    let script = r#"
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
  $null = [Windows.UI.Notifications.Management.UserNotificationListener, Windows.UI.Notifications, ContentType = WindowsRuntime]
  $listener = [Windows.UI.Notifications.Management.UserNotificationListener]::Current
  if ($listener.GetAccessStatus() -ne 'Allowed') { exit 0 }
  $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.IsGenericMethod -and $_.GetGenericArguments().Count -eq 1 -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
  $op = $listener.GetNotificationsAsync([Windows.UI.Notifications.NotificationKinds]::Toast)
  $task = $asTask.MakeGenericMethod([System.Collections.Generic.IReadOnlyList[Windows.UI.Notifications.UserNotification]]).Invoke($null, @($op))
  $task.Wait(1500) | Out-Null
  if (-not $task.IsCompleted) { exit 0 }
  $out = foreach ($n in ($task.Result | Select-Object -First 8)) {
    $app = $n.AppInfo.DisplayInfo.DisplayName
    $binding = $n.Notification.Visual.GetBinding([Windows.UI.Notifications.KnownNotificationBindings]::ToastGeneric)
    $texts = @()
    if ($binding) { $texts = @($binding.GetTextElements() | ForEach-Object { $_.Text }) }
    [PSCustomObject]@{
      id = [uint32]$n.Id
      app = [string]$app
      title = if ($texts.Count -ge 1) { [string]$texts[0] } else { [string]$app }
      body = if ($texts.Count -ge 2) { [string]($texts[1..($texts.Count-1)] -join ' ') } else { '' }
      time = $n.CreationTime.LocalDateTime.ToString('HH:mm')
    }
  }
  if ($out) { $out | ConvertTo-Json -Compress }
} catch { }
"#;
    let Ok(output) = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    parse_toasts_json(raw.trim())
}

fn parse_toasts_json(raw: &str) -> Vec<WindowsToast> {
    if raw.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let entries = match value {
        serde_json::Value::Array(items) => items,
        obj @ serde_json::Value::Object(_) => vec![obj],
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let windows_id = u32::try_from(entry.get("id")?.as_u64()?).ok()?;
            let app = entry
                .get("app")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Windows")
                .trim()
                .to_owned();
            let title = entry
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&app)
                .trim()
                .to_owned();
            let body = entry
                .get("body")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            let timestamp = entry
                .get("time")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            (!title.is_empty()).then_some(WindowsToast {
                windows_id,
                app,
                title,
                body,
                timestamp,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_records_notifications_suppresses_banner_under_dnd_and_dismisses() {
        let mut center = Center::new();
        assert!(center.push(
            "Nosih",
            "Network Connected",
            "HomeWiFi",
            "now",
            Category::Network,
            false
        ));
        assert!(center.snapshot().banner.is_some());
        assert_eq!(center.snapshot().items.len(), 1);

        // Consecutive duplicate is ignored.
        assert!(!center.push(
            "Nosih",
            "Network Connected",
            "HomeWiFi",
            "now",
            Category::Network,
            false
        ));
        assert_eq!(center.snapshot().items.len(), 1);

        // Under Do Not Disturb, item is stored without popping up a banner.
        assert!(center.dismiss_banner());
        assert!(!center.push(
            "Nosih",
            "Bluetooth Disabled",
            "Off",
            "now",
            Category::Bluetooth,
            true
        ));
        assert!(center.snapshot().banner.is_none());
        assert_eq!(center.snapshot().items.len(), 2);

        let first_id = center.snapshot().items[0].id;
        center.dismiss(first_id);
        assert_eq!(center.snapshot().items.len(), 1);

        center.clear_all();
        assert!(center.snapshot().items.is_empty());
        assert!(center.snapshot().banner.is_none());
    }

    #[test]
    fn calendar_grid_handles_leap_years_and_weekdays() {
        let feb_leap = calendar_for(2024, 2, 29);
        assert_eq!(feb_leap.title, "February 2024");
        assert_eq!(feb_leap.days_in_month, 29);
        assert_eq!(feb_leap.first_weekday, 4); // Thursday

        let feb_std = calendar_for(2026, 2, 14);
        assert_eq!(feb_std.days_in_month, 28);
        assert_eq!(feb_std.first_weekday, 0); // Sunday
    }

    #[test]
    fn windows_toast_json_parses_single_object_and_array() {
        let single = r#"{"id":42,"app":"Mail","title":"Hello","body":"World","time":"10:15"}"#;
        let parsed = parse_toasts_json(single);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].windows_id, 42);
        assert_eq!(parsed[0].title, "Hello");

        let array = r#"[{"id":1,"app":"A","title":"T1","body":"B1","time":"11:00"},{"id":2,"app":"B","title":"T2","body":"","time":"11:01"}]"#;
        assert_eq!(parse_toasts_json(array).len(), 2);
        assert!(parse_toasts_json("").is_empty());
    }
}
