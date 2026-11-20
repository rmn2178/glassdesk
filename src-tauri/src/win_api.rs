use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindowInfo {
    pub title: String,
    pub executable: String,
    pub process_id: u32,
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStatus {
    pub percent: Option<u8>,
    pub is_charging: bool,
    pub ac_connected: bool,
    pub has_battery: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub volume: f32,
    pub volume_percent: u8,
    pub muted: bool,
    pub output_device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    pub connected: bool,
    pub internet: bool,
    pub network_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDescriptor {
    pub index: usize,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSessionInfo {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source_app: String,
    pub is_playing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub active_window: ActiveWindowInfo,
    pub battery: BatteryStatus,
    pub keyboard_layout: String,
    pub audio: AudioStatus,
    pub network: NetworkStatus,
    pub is_fullscreen_active: bool,
    pub media: MediaSessionInfo,
}

// ============================================================================
// Windows 11 Native Implementation (windows = "0.58")
// ============================================================================

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };
    use windows::Win32::Foundation::{
        CloseHandle, BOOL, HWND, LPARAM, RECT, RPC_E_CHANGED_MODE,
    };
    use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SISO639LANGNAME2};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::Networking::NetworkListManager::{
        INetwork, INetworkListManager, NetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET,
        NLM_CONNECTIVITY_IPV6_INTERNET, NLM_ENUM_NETWORK_CONNECTED,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, QueryFullProcessImageNameW, SetProcessWorkingSetSize,
        CREATE_NO_WINDOW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, GetKeyboardLayout, KEYEVENTF_KEYUP, VK_MEDIA_NEXT_TRACK,
        VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, ShellExecuteW, QUNS_BUSY, QUNS_PRESENTATION_MODE,
        QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId,
        SetWindowLongW, SetWindowPos, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNORMAL, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    struct ComGuard {
        should_uninit: bool,
    }

    impl ComGuard {
        fn acquire() -> Result<Self, String> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                if hr.is_ok() {
                    Ok(Self {
                        should_uninit: true,
                    })
                } else if hr == RPC_E_CHANGED_MODE {
                    Ok(Self {
                        should_uninit: false,
                    })
                } else {
                    Err(format!("CoInitializeEx failed: {hr:?}"))
                }
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn window_to_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
        let raw = window
            .hwnd()
            .map_err(|e| format!("Failed to obtain Win32 HWND: {e}"))?;
        Ok(HWND(raw.0 as isize as *mut c_void))
    }

    pub fn get_active_window() -> ActiveWindowInfo {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return ActiveWindowInfo {
                    title: String::new(),
                    executable: String::new(),
                    process_id: 0,
                    is_self: false,
                };
            }

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                return ActiveWindowInfo {
                    title: String::new(),
                    executable: String::new(),
                    process_id: pid,
                    is_self: true,
                };
            }

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            let title = if len > 0 {
                String::from_utf16_lossy(&title_buf[..len as usize])
            } else {
                String::new()
            };

            let mut executable = String::new();
            if pid != 0 {
                if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut path_buf = [0u16; 1024];
                    let mut size = path_buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(path_buf.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok()
                    {
                        let full = String::from_utf16_lossy(&path_buf[..size as usize]);
                        executable = Path::new(&full)
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&full)
                            .to_string();
                    }
                    let _ = CloseHandle(handle);
                }
            }

            ActiveWindowInfo {
                title,
                executable,
                process_id: pid,
                is_self: false,
            }
        }
    }

    pub fn get_battery_status() -> BatteryStatus {
        unsafe {
            let mut status = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut status).is_err() {
                return BatteryStatus {
                    percent: None,
                    is_charging: false,
                    ac_connected: true,
                    has_battery: false,
                };
            }

            let has_battery = status.BatteryFlag != 128 && status.BatteryFlag != 255;
            let percent = if has_battery && status.BatteryLifePercent <= 100 {
                Some(status.BatteryLifePercent)
            } else {
                None
            };
            let ac_connected = status.ACLineStatus == 1;
            let is_charging = (status.BatteryFlag & 8) != 0
                || (has_battery && ac_connected && percent.unwrap_or(100) < 100);

            BatteryStatus {
                percent,
                is_charging,
                ac_connected,
                has_battery,
            }
        }
    }

    pub fn get_keyboard_layout_code() -> String {
        unsafe {
            let hwnd = GetForegroundWindow();
            let thread_id = if !hwnd.0.is_null() {
                GetWindowThreadProcessId(hwnd, None)
            } else {
                0
            };

            let hkl = GetKeyboardLayout(thread_id);
            let lang_id = (hkl.0 as usize & 0xFFFF) as u32;

            let mut buf = [0u16; 16];
            let written = GetLocaleInfoW(lang_id, LOCALE_SISO639LANGNAME2, Some(&mut buf));
            if written > 1 {
                let code = String::from_utf16_lossy(&buf[..(written as usize - 1)]);
                return code.to_ascii_uppercase();
            }

            "ENG".to_string()
        }
    }

    fn with_endpoint_volume<T, F>(f: F) -> Result<T, String>
    where
        F: FnOnce(&IAudioEndpointVolume) -> Result<T, String>,
    {
        let _com = ComGuard::acquire()?;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("Failed to create IMMDeviceEnumerator: {e}"))?;

            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("No default audio render endpoint: {e}"))?;

            let endpoint: IAudioEndpointVolume = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("Failed to activate IAudioEndpointVolume: {e}"))?;

            f(&endpoint)
        }
    }

    pub fn get_audio_status() -> AudioStatus {
        with_endpoint_volume(|ep| unsafe {
            let vol = ep
                .GetMasterVolumeLevelScalar()
                .map_err(|e| format!("GetMasterVolumeLevelScalar failed: {e}"))?;
            let muted = ep
                .GetMute()
                .map_err(|e| format!("GetMute failed: {e}"))?
                .as_bool();
            let clamped = vol.clamp(0.0, 1.0);
            Ok(AudioStatus {
                volume: clamped,
                volume_percent: (clamped * 100.0).round() as u8,
                muted,
                output_device: "Default Output".to_string(),
            })
        })
        .unwrap_or(AudioStatus {
            volume: 0.0,
            volume_percent: 0,
            muted: true,
            output_device: "No Output Device".to_string(),
        })
    }

    pub fn set_volume(level: f32) -> Result<AudioStatus, String> {
        let clamped = level.clamp(0.0, 1.0);
        with_endpoint_volume(|ep| unsafe {
            ep.SetMasterVolumeLevelScalar(clamped, std::ptr::null())
                .map_err(|e| format!("SetMasterVolumeLevelScalar failed: {e}"))?;
            let muted = ep.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            Ok(AudioStatus {
                volume: clamped,
                volume_percent: (clamped * 100.0).round() as u8,
                muted,
                output_device: "Default Output".to_string(),
            })
        })
    }

    pub fn toggle_mute() -> Result<AudioStatus, String> {
        with_endpoint_volume(|ep| unsafe {
            let currently_muted = ep
                .GetMute()
                .map_err(|e| format!("GetMute failed: {e}"))?
                .as_bool();
            let next_muted = !currently_muted;
            ep.SetMute(BOOL::from(next_muted), std::ptr::null())
                .map_err(|e| format!("SetMute failed: {e}"))?;
            let vol = ep
                .GetMasterVolumeLevelScalar()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            Ok(AudioStatus {
                volume: vol,
                volume_percent: (vol * 100.0).round() as u8,
                muted: next_muted,
                output_device: "Default Output".to_string(),
            })
        })
    }

    pub fn get_network_status() -> NetworkStatus {
        let result: Result<NetworkStatus, String> = (|| {
            let _com = ComGuard::acquire()?;
            unsafe {
                let manager: INetworkListManager =
                    CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance(NetworkListManager) failed: {e}"))?;

                let connected = manager
                    .IsConnected()
                    .map(|v| v.as_bool())
                    .unwrap_or(false);

                let connectivity = manager.GetConnectivity().ok();
                let has_internet_flag = connectivity
                    .map(|c| {
                        (c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0))
                            != 0
                    })
                    .unwrap_or(false);

                let internet = manager
                    .IsConnectedToInternet()
                    .map(|v| v.as_bool())
                    .unwrap_or(has_internet_flag);

                let mut network_name = None;
                if connected {
                    if let Ok(enum_nets) = manager.GetNetworks(NLM_ENUM_NETWORK_CONNECTED) {
                        let mut buf: [Option<INetwork>; 1] = [None];
                        let mut fetched: u32 = 0;
                        if enum_nets.Next(&mut buf, Some(&mut fetched)).is_ok() && fetched > 0 {
                            if let Some(net) = buf[0].take() {
                                if let Ok(bstr) = net.GetName() {
                                    let name = bstr.to_string();
                                    if !name.is_empty() {
                                        network_name = Some(name);
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(NetworkStatus {
                    connected,
                    internet,
                    network_name,
                })
            }
        })();

        result.unwrap_or(NetworkStatus {
            connected: false,
            internet: false,
            network_name: None,
        })
    }

    pub fn is_fullscreen_app_active() -> bool {
        unsafe {
            match SHQueryUserNotificationState() {
                Ok(state) => {
                    state == QUNS_BUSY
                        || state == QUNS_RUNNING_D3D_FULL_SCREEN
                        || state == QUNS_PRESENTATION_MODE
                }
                Err(_) => false,
            }
        }
    }

    unsafe extern "system" fn enum_monitor_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<MONITORINFOEXW>);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmonitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool()
        {
            list.push(info);
        }
        BOOL(1)
    }

    pub fn enumerate_monitors(window: &WebviewWindow) -> Vec<MonitorDescriptor> {
        let scale_fallback = window.scale_factor().unwrap_or(1.0);
        let tauri_monitors = window.available_monitors().unwrap_or_default();

        let mut raw_list: Vec<MONITORINFOEXW> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(enum_monitor_proc),
                LPARAM(&mut raw_list as *mut Vec<MONITORINFOEXW> as isize),
            );
        }

        if raw_list.is_empty() {
            return tauri_monitors
                .into_iter()
                .enumerate()
                .map(|(index, m)| MonitorDescriptor {
                    index,
                    name: m
                        .name()
                        .cloned()
                        .unwrap_or_else(|| format!("Monitor {}", index + 1)),
                    x: m.position().x,
                    y: m.position().y,
                    width: m.size().width,
                    height: m.size().height,
                    scale_factor: m.scale_factor(),
                    is_primary: index == 0,
                })
                .collect();
        }

        raw_list
            .into_iter()
            .enumerate()
            .map(|(index, info)| {
                let rc = info.monitorInfo.rcMonitor;
                let width = (rc.right - rc.left).max(1) as u32;
                let height = (rc.bottom - rc.top).max(1) as u32;
                let is_primary = (info.monitorInfo.dwFlags & 1) != 0;

                let name_end = info
                    .szDevice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(info.szDevice.len());
                let name = String::from_utf16_lossy(&info.szDevice[..name_end]);

                let scale_factor = tauri_monitors
                    .iter()
                    .find(|m| m.position().x == rc.left && m.position().y == rc.top)
                    .map(|m| m.scale_factor())
                    .unwrap_or(scale_fallback);

                MonitorDescriptor {
                    index,
                    name,
                    x: rc.left,
                    y: rc.top,
                    width,
                    height,
                    scale_factor,
                    is_primary,
                }
            })
            .collect()
    }

    pub fn apply_overlay_window_styles(window: &WebviewWindow) -> Result<(), String> {
        let hwnd = window_to_hwnd(window)?;
        unsafe {
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let updated = ex_style
                | (WS_EX_TOOLWINDOW.0 as i32)
                | (WS_EX_TOPMOST.0 as i32)
                | (WS_EX_NOACTIVATE.0 as i32);
            SetWindowLongW(hwnd, GWL_EXSTYLE, updated);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
            .map_err(|e| format!("SetWindowPos failed: {e}"))?;
        }
        Ok(())
    }

    pub fn set_overlay_bounds(
        window: &WebviewWindow,
        logical_height: u32,
        focusable: bool,
        monitor_index: Option<usize>,
        floating_width: Option<u32>,
        horizontal_offset: i32,
        always_on_top: bool,
    ) -> Result<(), String> {
        let monitors = enumerate_monitors(window);
        let target = monitor_index
            .and_then(|idx| monitors.get(idx))
            .or_else(|| monitors.iter().find(|m| m.is_primary))
            .or_else(|| monitors.first());

        let (mon_x, mon_y, mon_w, mon_h, scale) = match target {
            Some(m) => (m.x, m.y, m.width, m.height, m.scale_factor),
            None => (0, 0, 1920, 1080, 1.0),
        };

        let clamped_logical = logical_height.clamp(28, 860);
        let phys_height = ((clamped_logical as f64) * scale)
            .round()
            .min((mon_h as f64) * 0.88) as i32;

        let (win_x, win_w) = match floating_width {
            Some(fw) => {
                let phys_w = ((fw.clamp(280, 1200) as f64) * scale)
                    .round()
                    .min(mon_w as f64) as i32;
                let phys_off = ((horizontal_offset as f64) * scale).round() as i32;
                let centered = mon_x + ((mon_w as i32 - phys_w) / 2) + phys_off;
                let max_x = mon_x + (mon_w as i32 - phys_w).max(0);
                (centered.clamp(mon_x, max_x), phys_w)
            }
            None => (mon_x, mon_w as i32),
        };

        let hwnd = window_to_hwnd(window)?;
        unsafe {
            let mut ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            ex_style |= WS_EX_TOOLWINDOW.0 as i32;
            if always_on_top {
                ex_style |= WS_EX_TOPMOST.0 as i32;
            } else {
                ex_style &= !(WS_EX_TOPMOST.0 as i32);
            }
            if focusable {
                ex_style &= !(WS_EX_NOACTIVATE.0 as i32);
            } else {
                ex_style |= WS_EX_NOACTIVATE.0 as i32;
            }
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style);

            let mut flags = SWP_FRAMECHANGED | SWP_SHOWWINDOW;
            if !focusable {
                flags |= SWP_NOACTIVATE;
            }

            let z_order = if always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };

            SetWindowPos(hwnd, z_order, win_x, mon_y, win_w, phys_height, flags)
                .map_err(|e| format!("SetWindowPos(set_overlay_bounds) failed: {e}"))?;
        }

        if focusable {
            let _ = window.set_focus();
        }
        Ok(())
    }

    pub fn open_file_default(path: &str) -> Result<(), String> {
        let op = to_wide_null("open");
        let target = to_wide_null(path);
        unsafe {
            let res = ShellExecuteW(
                HWND::default(),
                PCWSTR::from_raw(op.as_ptr()),
                PCWSTR::from_raw(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (res.0 as isize) <= 32 {
                return Err(format!(
                    "ShellExecuteW failed to open '{path}' (code {})",
                    res.0 as isize
                ));
            }
        }
        Ok(())
    }

    pub fn reveal_in_explorer(path: &str) -> Result<(), String> {
        let sanitized = path.replace('/', "\\");
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{sanitized}\""))
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .map_err(|e| format!("Failed to launch explorer.exe: {e}"))?;
        Ok(())
    }

    pub fn get_media_session() -> MediaSessionInfo {
        let empty = MediaSessionInfo {
            has_session: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            source_app: String::new(),
            is_playing: false,
        };

        let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() else {
            return empty;
        };
        let Ok(manager) = op.get() else {
            return empty;
        };
        let Ok(session) = manager.GetCurrentSession() else {
            return empty;
        };

        let is_playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
            .unwrap_or(false);

        let source_app = session
            .SourceAppUserModelId()
            .map(|s| {
                let raw = s.to_string();
                raw.split('!').next().unwrap_or(&raw).replace(".exe", "")
            })
            .unwrap_or_default();

        let (title, artist, album) = session
            .TryGetMediaPropertiesAsync()
            .ok()
            .and_then(|p| p.get().ok())
            .map(|props| {
                (
                    props.Title().map(|s| s.to_string()).unwrap_or_default(),
                    props.Artist().map(|s| s.to_string()).unwrap_or_default(),
                    props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default(),
                )
            })
            .unwrap_or_default();

        if title.is_empty() && artist.is_empty() {
            return empty;
        }

        MediaSessionInfo {
            has_session: true,
            title,
            artist,
            album,
            source_app,
            is_playing,
        }
    }

    fn send_media_vk(vk: u8) {
        unsafe {
            keybd_event(vk, 0, Default::default(), 0);
            keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
        }
    }

    pub fn media_control(action: &str) -> Result<MediaSessionInfo, String> {
        let handled = (|| -> Option<bool> {
            let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .ok()?
                .get()
                .ok()?;
            let session = manager.GetCurrentSession().ok()?;
            let ok = match action {
                "play_pause" => session.TryTogglePlayPauseAsync().ok()?.get().ok()?,
                "next" => session.TrySkipNextAsync().ok()?.get().ok()?,
                "previous" => session.TrySkipPreviousAsync().ok()?.get().ok()?,
                _ => return Some(false),
            };
            Some(ok)
        })()
        .unwrap_or(false);

        if !handled {
            match action {
                "play_pause" => send_media_vk(VK_MEDIA_PLAY_PAUSE.0 as u8),
                "next" => send_media_vk(VK_MEDIA_NEXT_TRACK.0 as u8),
                "previous" => send_media_vk(VK_MEDIA_PREV_TRACK.0 as u8),
                _ => return Err(format!("Unknown media action: {action}")),
            }
        }

        Ok(get_media_session())
    }

    pub fn start_native_file_drag(window: &WebviewWindow, paths: Vec<String>) -> Result<(), String> {
        let valid_paths: Vec<std::path::PathBuf> = paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        if valid_paths.is_empty() {
            return Err("No existing files to drag".to_string());
        }
        let icon = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
        drag::start_drag(
            window,
            drag::DragItem::Files(valid_paths),
            icon,
            |_, _| {},
            drag::Options::default(),
        )
        .map_err(|e| format!("Native drag-out failed: {e:?}"))
    }

    pub fn trim_working_set() {
        unsafe {
            let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }

    pub fn open_windows_settings(uri: &str) -> Result<(), String> {
        const ALLOWED: &[&str] = &[
            "ms-settings:",
            "ms-settings:network",
            "ms-settings:network-wifi",
            "ms-settings:network-airplanemode",
            "ms-settings:bluetooth",
            "ms-settings:sound",
            "ms-settings:batterysaver",
            "ms-settings:regionlanguage",
            "ms-settings:display",
            "ms-settings:notifications",
            "ms-settings:quiethours",
        ];
        if !ALLOWED.contains(&uri) {
            return Err(format!("Unsupported Windows Settings URI: {uri}"));
        }
        open_file_default(uri)
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;
    use tauri::{PhysicalPosition, PhysicalSize};

    pub fn get_active_window() -> ActiveWindowInfo {
        ActiveWindowInfo {
            title: String::new(),
            executable: String::new(),
            process_id: 0,
            is_self: false,
        }
    }

    pub fn get_battery_status() -> BatteryStatus {
        BatteryStatus {
            percent: None,
            is_charging: false,
            ac_connected: true,
            has_battery: false,
        }
    }

    pub fn get_keyboard_layout_code() -> String {
        "ENG".to_string()
    }

    pub fn get_audio_status() -> AudioStatus {
        AudioStatus {
            volume: 0.5,
            volume_percent: 50,
            muted: false,
            output_device: "Default Output".to_string(),
        }
    }

    pub fn set_volume(level: f32) -> Result<AudioStatus, String> {
        let clamped = level.clamp(0.0, 1.0);
        Ok(AudioStatus {
            volume: clamped,
            volume_percent: (clamped * 100.0).round() as u8,
            muted: false,
            output_device: "Default Output".to_string(),
        })
    }

    pub fn toggle_mute() -> Result<AudioStatus, String> {
        Ok(AudioStatus {
            volume: 0.5,
            volume_percent: 50,
            muted: true,
            output_device: "Default Output".to_string(),
        })
    }

    pub fn get_network_status() -> NetworkStatus {
        NetworkStatus {
            connected: false,
            internet: false,
            network_name: None,
        }
    }

    pub fn is_fullscreen_app_active() -> bool {
        false
    }

    pub fn enumerate_monitors(window: &WebviewWindow) -> Vec<MonitorDescriptor> {
        window
            .available_monitors()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, m)| MonitorDescriptor {
                index,
                name: m
                    .name()
                    .cloned()
                    .unwrap_or_else(|| format!("Monitor {}", index + 1)),
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
                scale_factor: m.scale_factor(),
                is_primary: index == 0,
            })
            .collect()
    }

    pub fn apply_overlay_window_styles(_window: &WebviewWindow) -> Result<(), String> {
        Ok(())
    }

    pub fn set_overlay_bounds(
        window: &WebviewWindow,
        logical_height: u32,
        focusable: bool,
        monitor_index: Option<usize>,
        floating_width: Option<u32>,
        horizontal_offset: i32,
        _always_on_top: bool,
    ) -> Result<(), String> {
        let monitors = enumerate_monitors(window);
        let target = monitor_index
            .and_then(|idx| monitors.get(idx))
            .or_else(|| monitors.first());

        let (mon_x, mon_y, mon_w, scale) = match target {
            Some(m) => (m.x, m.y, m.width, m.scale_factor),
            None => (0, 0, 1920, 1.0),
        };

        let phys_h = ((logical_height.clamp(28, 860) as f64) * scale).round() as u32;
        let (x, w) = match floating_width {
            Some(fw) => {
                let phys_w = ((fw.clamp(280, 1200) as f64) * scale).round() as u32;
                let centered = mon_x + ((mon_w as i32 - phys_w as i32) / 2) + horizontal_offset;
                (centered.max(mon_x), phys_w.min(mon_w))
            }
            None => (mon_x, mon_w),
        };
        window
            .set_position(PhysicalPosition::new(x, mon_y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(PhysicalSize::new(w, phys_h))
            .map_err(|e| e.to_string())?;
        if focusable {
            let _ = window.set_focus();
        }
        Ok(())
    }

    pub fn open_file_default(_path: &str) -> Result<(), String> {
        Err("open_file_default is only supported on Windows".to_string())
    }

    pub fn reveal_in_explorer(_path: &str) -> Result<(), String> {
        Err("reveal_in_explorer is only supported on Windows".to_string())
    }

    pub fn get_media_session() -> MediaSessionInfo {
        MediaSessionInfo {
            has_session: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            source_app: String::new(),
            is_playing: false,
        }
    }

    pub fn media_control(_action: &str) -> Result<MediaSessionInfo, String> {
        Ok(get_media_session())
    }

    pub fn start_native_file_drag(
        _window: &WebviewWindow,
        _paths: Vec<String>,
    ) -> Result<(), String> {
        Err("Native file drag-out is supported on Windows".to_string())
    }

    pub fn trim_working_set() {}

    pub fn open_windows_settings(_uri: &str) -> Result<(), String> {
        Err("open_windows_settings is only supported on Windows".to_string())
    }
}

pub use imp::*;

pub fn get_system_snapshot() -> SystemSnapshot {
    SystemSnapshot {
        active_window: get_active_window(),
        battery: get_battery_status(),
        keyboard_layout: get_keyboard_layout_code(),
        audio: get_audio_status(),
        network: get_network_status(),
        is_fullscreen_active: is_fullscreen_app_active(),
        media: get_media_session(),
    }
}
