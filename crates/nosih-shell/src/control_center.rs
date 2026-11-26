//! Direct Windows system controls backing Nosih's macOS-style Control Center.
//!
//! Covers Wi-Fi SSID queries, Dark/Light mode toggling, Do Not Disturb
//! (notification muting), display brightness (DDC/CI with WMI fallback for
//! laptop panels), screen lock, screen snip, and opening Nosih Settings.

use std::{env, mem::size_of, os::windows::process::CommandExt, process::Command};

use windows::{
    Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation},
    Win32::{
        Foundation::{FreeLibrary, HANDLE, LPARAM, POINT, WPARAM},
        Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint},
        System::{
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
                RRF_RT_REG_DWORD, RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
            },
        },
        UI::WindowsAndMessaging::{
            HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
        },
    },
    core::{BOOL, PCWSTR, Result, s, w},
};

const PERSONALIZE_KEY: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
const PUSH_NOTIFICATIONS_KEY: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\PushNotifications");
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

type RawProc = unsafe extern "system" fn() -> isize;

/* ------------------------------------------------------------------ Wi-Fi */

/// Name (SSID) of the active Wi-Fi connection, if connected over WLAN.
#[must_use]
pub fn wifi_name() -> Option<String> {
    let profile = NetworkInformation::GetInternetConnectionProfile().ok()?;
    let level = profile
        .GetNetworkConnectivityLevel()
        .unwrap_or(NetworkConnectivityLevel::None);
    if level == NetworkConnectivityLevel::None || !profile.IsWlanConnectionProfile().unwrap_or(false)
    {
        return None;
    }
    let name = profile.ProfileName().ok()?.to_string_lossy();
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/* -------------------------------------------------------------- Dark mode */

/// `true` while Windows uses the dark theme for its shell and taskbar.
#[must_use]
pub fn is_dark_mode() -> bool {
    !crate::system::uses_light_theme()
}

/// Switches Windows between dark and light mode for both system UI and apps,
/// then broadcasts `WM_SETTINGCHANGE` (`ImmersiveColorSet`) so Windows and
/// Nosih immediately reflect the change.
pub fn set_dark_mode(dark: bool) -> Result<()> {
    let light = u32::from(!dark).to_le_bytes();
    let mut key = HKEY::default();
    // SAFETY: Standard HKCU write; the opened key is always closed.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PERSONALIZE_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let system_result =
            RegSetValueExW(key, w!("SystemUsesLightTheme"), None, REG_DWORD, Some(&light));
        let apps_result =
            RegSetValueExW(key, w!("AppsUseLightTheme"), None, REG_DWORD, Some(&light));
        let _ = RegCloseKey(key);
        system_result.ok()?;
        apps_result.ok()?;
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1_000,
            None,
        );
    }
    Ok(())
}

/* --------------------------------------------------------- Do Not Disturb */

/// `true` while Windows toast notifications are muted (Do Not Disturb).
#[must_use]
pub fn do_not_disturb() -> bool {
    let mut value = 1u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` and `size` describe a valid 4-byte buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PUSH_NOTIFICATIONS_KEY,
            w!("ToastEnabled"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 0
}

/// Enables or disables Do Not Disturb by toggling Windows' global toast
/// notification preference (`ToastEnabled`) under HKCU.
pub fn set_do_not_disturb(enabled: bool) -> Result<()> {
    let toast_enabled = u32::from(!enabled).to_le_bytes();
    let mut key = HKEY::default();
    // SAFETY: Standard HKCU write; the opened key is always closed.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PUSH_NOTIFICATIONS_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result = RegSetValueExW(
            key,
            w!("ToastEnabled"),
            None,
            REG_DWORD,
            Some(&toast_enabled),
        );
        let _ = RegCloseKey(key);
        result.ok()
    }
}

/* ----------------------------------------------------- Display brightness */

#[repr(C)]
struct PhysicalMonitor {
    handle: HANDLE,
    #[allow(dead_code)]
    description: [u16; 128],
}

type GetNumberOfPhysicalMonitorsFn = unsafe extern "system" fn(HMONITOR, *mut u32) -> BOOL;
type GetPhysicalMonitorsFn =
    unsafe extern "system" fn(HMONITOR, u32, *mut PhysicalMonitor) -> BOOL;
type GetMonitorBrightnessFn =
    unsafe extern "system" fn(HANDLE, *mut u32, *mut u32, *mut u32) -> BOOL;
type SetMonitorBrightnessFn = unsafe extern "system" fn(HANDLE, u32) -> BOOL;
type DestroyPhysicalMonitorsFn = unsafe extern "system" fn(u32, *mut PhysicalMonitor) -> BOOL;

/// Reads primary monitor brightness (0..=100). Tries DDC/CI via `dxva2.dll`
/// first (fast path for external monitors), then falls back to WMI
/// `WmiMonitorBrightness` for laptop internal displays.
///
/// Call from a background worker thread, never on the UI thread.
#[must_use]
pub fn display_brightness() -> Option<u8> {
    ddc_get_brightness().or_else(wmi_get_brightness)
}

/// Sets primary monitor brightness (0..=100). Tries DDC/CI first, then WMI.
///
/// Call from a background worker thread, never on the UI thread.
pub fn set_display_brightness(percent: u8) -> bool {
    let clamped = percent.min(100);
    ddc_set_brightness(clamped) || wmi_set_brightness(clamped)
}

fn ddc_get_brightness() -> Option<u8> {
    // SAFETY: Dynamically loads standard Windows `dxva2.dll`, resolves well-known
    // DDC/CI functions, and frees both physical monitor handles and the DLL.
    unsafe {
        let module = LoadLibraryExW(w!("dxva2.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let result = (|| -> Option<u8> {
            let get_count = std::mem::transmute::<RawProc, GetNumberOfPhysicalMonitorsFn>(
                GetProcAddress(module, s!("GetNumberOfPhysicalMonitorsFromHMONITOR"))?,
            );
            let get_monitors = std::mem::transmute::<RawProc, GetPhysicalMonitorsFn>(
                GetProcAddress(module, s!("GetPhysicalMonitorsFromHMONITOR"))?,
            );
            let get_brightness = std::mem::transmute::<RawProc, GetMonitorBrightnessFn>(
                GetProcAddress(module, s!("GetMonitorBrightness"))?,
            );
            let destroy_monitors = std::mem::transmute::<RawProc, DestroyPhysicalMonitorsFn>(
                GetProcAddress(module, s!("DestroyPhysicalMonitors"))?,
            );

            let hmonitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
            let mut count = 0u32;
            if !get_count(hmonitor, &mut count).as_bool() || count == 0 {
                return None;
            }
            let mut monitors: Vec<PhysicalMonitor> = (0..count)
                .map(|_| PhysicalMonitor {
                    handle: HANDLE::default(),
                    description: [0u16; 128],
                })
                .collect();
            if !get_monitors(hmonitor, count, monitors.as_mut_ptr()).as_bool() {
                return None;
            }
            let reading = monitors.iter().find_map(|monitor| {
                let (mut min_v, mut cur_v, mut max_v) = (0u32, 0u32, 0u32);
                if get_brightness(monitor.handle, &mut min_v, &mut cur_v, &mut max_v).as_bool()
                    && max_v > min_v
                {
                    let ratio = f64::from(cur_v.saturating_sub(min_v)) / f64::from(max_v - min_v);
                    Some((ratio.clamp(0.0, 1.0) * 100.0).round() as u8)
                } else {
                    None
                }
            });
            let _ = destroy_monitors(count, monitors.as_mut_ptr());
            reading
        })();
        let _ = FreeLibrary(module);
        result
    }
}

fn ddc_set_brightness(percent: u8) -> bool {
    // SAFETY: Dynamically loads standard Windows `dxva2.dll`, resolves well-known
    // DDC/CI functions, and frees both physical monitor handles and the DLL.
    unsafe {
        let Ok(module) = LoadLibraryExW(w!("dxva2.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else {
            return false;
        };
        let result = (|| -> bool {
            let Some(get_count) =
                GetProcAddress(module, s!("GetNumberOfPhysicalMonitorsFromHMONITOR"))
            else {
                return false;
            };
            let Some(get_monitors) = GetProcAddress(module, s!("GetPhysicalMonitorsFromHMONITOR"))
            else {
                return false;
            };
            let Some(get_brightness) = GetProcAddress(module, s!("GetMonitorBrightness")) else {
                return false;
            };
            let Some(set_brightness) = GetProcAddress(module, s!("SetMonitorBrightness")) else {
                return false;
            };
            let Some(destroy_monitors) = GetProcAddress(module, s!("DestroyPhysicalMonitors"))
            else {
                return false;
            };
            let get_count = std::mem::transmute::<RawProc, GetNumberOfPhysicalMonitorsFn>(get_count);
            let get_monitors = std::mem::transmute::<RawProc, GetPhysicalMonitorsFn>(get_monitors);
            let get_brightness =
                std::mem::transmute::<RawProc, GetMonitorBrightnessFn>(get_brightness);
            let set_brightness =
                std::mem::transmute::<RawProc, SetMonitorBrightnessFn>(set_brightness);
            let destroy_monitors =
                std::mem::transmute::<RawProc, DestroyPhysicalMonitorsFn>(destroy_monitors);

            let hmonitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
            let mut count = 0u32;
            if !get_count(hmonitor, &mut count).as_bool() || count == 0 {
                return false;
            }
            let mut monitors: Vec<PhysicalMonitor> = (0..count)
                .map(|_| PhysicalMonitor {
                    handle: HANDLE::default(),
                    description: [0u16; 128],
                })
                .collect();
            if !get_monitors(hmonitor, count, monitors.as_mut_ptr()).as_bool() {
                return false;
            }
            let mut any_ok = false;
            for monitor in &monitors {
                let (mut min_v, mut cur_v, mut max_v) = (0u32, 0u32, 0u32);
                if get_brightness(monitor.handle, &mut min_v, &mut cur_v, &mut max_v).as_bool()
                    && max_v > min_v
                {
                    let target = min_v
                        + ((f64::from(max_v - min_v) * f64::from(percent) / 100.0).round() as u32);
                    any_ok |= set_brightness(monitor.handle, target.clamp(min_v, max_v)).as_bool();
                }
            }
            let _ = destroy_monitors(count, monitors.as_mut_ptr());
            any_ok
        })();
        let _ = FreeLibrary(module);
        result
    }
}

fn wmi_get_brightness() -> Option<u8> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1).CurrentBrightness",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value: u8 = text.trim().parse().ok()?;
    Some(value.min(100))
}

fn wmi_set_brightness(percent: u8) -> bool {
    let script = format!(
        "$m = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction SilentlyContinue | Select-Object -First 1; if ($m) {{ $null = $m | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1; Brightness={percent}}} }} else {{ exit 1 }}"
    );
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|status| status.success())
}

/* ---------------------------------------------------------- Quick actions */

/// Locks the Windows session immediately, like macOS Control Center / Lock Screen.
pub fn lock_workstation() {
    type LockWorkStationFn = unsafe extern "system" fn() -> BOOL;
    // SAFETY: Dynamically invokes `user32.dll!LockWorkStation` with no arguments.
    unsafe {
        if let Ok(module) = LoadLibraryExW(w!("user32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) {
            if let Some(proc) = GetProcAddress(module, s!("LockWorkStation")) {
                let lock = std::mem::transmute::<RawProc, LockWorkStationFn>(proc);
                let _ = lock();
            }
            let _ = FreeLibrary(module);
        }
    }
}

/// Opens Windows Screen Snip (`ms-screenclip:`).
pub fn open_screen_snip() {
    let _ = crate::launcher::open("ms-screenclip:");
}

/// Opens Nosih's own Settings app (`nosih-settings.exe` alongside the dock),
/// falling back to Windows Settings if the companion binary is not present.
pub fn open_nosih_settings() {
    let settings = env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("nosih-settings.exe"));
    if let Some(path) = settings
        && path.exists()
        && crate::launcher::open(&path.to_string_lossy()).is_ok()
    {
        return;
    }
    let _ = crate::launcher::open("ms-settings:");
}

/* ------------------------------------------------------------ Power modes */

const GUID_POWER_SAVER: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
const GUID_BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const GUID_HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

/// Windows power plan exposed as a one-click cycle inside Control Center.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PowerMode {
    PowerSaver,
    #[default]
    Balanced,
    HighPerformance,
}

impl PowerMode {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::PowerSaver => Self::Balanced,
            Self::Balanced => Self::HighPerformance,
            Self::HighPerformance => Self::PowerSaver,
        }
    }

    const fn guid(self) -> &'static str {
        match self {
            Self::PowerSaver => GUID_POWER_SAVER,
            Self::Balanced => GUID_BALANCED,
            Self::HighPerformance => GUID_HIGH_PERF,
        }
    }
}

fn parse_power_mode(output: &str) -> PowerMode {
    let lower = output.to_ascii_lowercase();
    if lower.contains(GUID_POWER_SAVER) {
        PowerMode::PowerSaver
    } else if lower.contains(GUID_HIGH_PERF) {
        PowerMode::HighPerformance
    } else {
        PowerMode::Balanced
    }
}

/// Reads the active Windows power plan. Call from a background worker thread.
#[must_use]
pub fn power_mode() -> PowerMode {
    let Ok(output) = Command::new("powercfg.exe")
        .arg("/getactivescheme")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return PowerMode::Balanced;
    };
    if !output.status.success() {
        return PowerMode::Balanced;
    }
    parse_power_mode(&String::from_utf8_lossy(&output.stdout))
}

/// Switches the active Windows power plan. Call from a background worker thread.
pub fn set_power_mode(mode: PowerMode) -> bool {
    Command::new("powercfg.exe")
        .args(["/setactive", mode.guid()])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|status| status.success())
}

/* ----------------------------------------------------- System menu actions */

/// Opens Windows "About This PC" settings page.
pub fn open_about_pc() {
    let _ = crate::launcher::open("ms-settings:about");
}

/// Puts the computer to sleep using `SetSuspendState(false, false, false)`.
pub fn sleep_computer() {
    // SAFETY: Standard Win32 power-state call with no pointers.
    unsafe {
        let _ = windows::Win32::System::Power::SetSuspendState(false, false, false);
    }
}

/// Restarts Windows immediately.
pub fn restart_computer() {
    let _ = Command::new("shutdown.exe")
        .args(["/r", "/t", "0"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// Shuts down Windows immediately.
pub fn shut_down_computer() {
    let _ = Command::new("shutdown.exe")
        .args(["/s", "/t", "0"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_mode_cycles_in_order() {
        assert_eq!(PowerMode::PowerSaver.next(), PowerMode::Balanced);
        assert_eq!(PowerMode::Balanced.next(), PowerMode::HighPerformance);
        assert_eq!(PowerMode::HighPerformance.next(), PowerMode::PowerSaver);
    }

    #[test]
    fn parse_power_mode_matches_standard_windows_guids() {
        assert_eq!(
            parse_power_mode("Power Scheme GUID: a1841308-3541-4fab-bc81-f71556f20b4a  (Power saver)"),
            PowerMode::PowerSaver
        );
        assert_eq!(
            parse_power_mode("Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)"),
            PowerMode::HighPerformance
        );
        assert_eq!(
            parse_power_mode("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)"),
            PowerMode::Balanced
        );
    }
}
