use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySensors {
    pub mic_active_apps: Vec<String>,
    pub cam_active_apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub volume_percent: u32,
    pub output_muted: bool,
    pub mic_muted: bool,
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{AudioState, PrivacySensors};
    use std::ffi::c_void;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{BOOL, HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetMonitorInfoW, GetPixel, MonitorFromWindow, ReleaseDC, MONITORINFO,
        MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ,
    };
    use windows::Win32::System::Shutdown::LockWorkStation;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEINPUT, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowLongW, GetWindowRect,
        SetWindowDisplayAffinity, SetWindowLongW, GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    pub fn apply_overlay_styles(hwnd_raw: isize) {
        let hwnd = HWND(hwnd_raw as *mut c_void);
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let updated = ex
                | (WS_EX_TOOLWINDOW.0 as i32)
                | (WS_EX_NOACTIVATE.0 as i32)
                | (WS_EX_TOPMOST.0 as i32);
            SetWindowLongW(hwnd, GWL_EXSTYLE, updated);
        }
    }

    pub fn set_capture_exclusion(hwnd_raw: isize, exclude: bool) {
        let hwnd = HWND(hwnd_raw as *mut c_void);
        let flag = if exclude {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        unsafe {
            let _ = SetWindowDisplayAffinity(hwnd, flag);
        }
    }

    pub fn is_foreground_fullscreen() -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() {
                return false;
            }
            let mut rect = RECT::default();
            if GetWindowRect(fg, &mut rect).is_err() {
                return false;
            }
            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(mon, &mut mi).as_bool() {
                return false;
            }
            let mr = mi.rcMonitor;
            let covers = rect.left <= mr.left
                && rect.top <= mr.top
                && rect.right >= mr.right
                && rect.bottom >= mr.bottom;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            covers && width >= 640 && height >= 480
        }
    }

    pub fn pick_cursor_hex_color() -> Result<String, String> {
        unsafe {
            let mut pt = POINT::default();
            GetCursorPos(&mut pt).map_err(|e| e.to_string())?;
            let hdc = GetDC(HWND::default());
            let color = GetPixel(hdc, pt.x, pt.y);
            ReleaseDC(HWND::default(), hdc);
            let c = color.0;
            let r = c & 0xFF;
            let g = (c >> 8) & 0xFF;
            let b = (c >> 16) & 0xFF;
            Ok(format!("#{:02X}{:02X}{:02X}", r, g, b))
        }
    }

    pub fn get_audio_state() -> AudioState {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: Result<IMMDeviceEnumerator, _> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL);
            let Ok(enumerator) = enumerator else {
                return AudioState {
                    volume_percent: 50,
                    output_muted: false,
                    mic_muted: false,
                };
            };

            let (volume_percent, output_muted) = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .and_then(|dev| dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None))
                .map(|ep| {
                    let v = ep.GetMasterVolumeLevelScalar().unwrap_or(0.5);
                    let m = ep.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                    ((v * 100.0).round() as u32, m)
                })
                .unwrap_or((50, false));

            let mic_muted = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .and_then(|dev| dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None))
                .and_then(|ep| ep.GetMute())
                .map(|b| b.as_bool())
                .unwrap_or(false);

            AudioState {
                volume_percent,
                output_muted,
                mic_muted,
            }
        }
    }

    pub fn set_master_volume(percent: u32) -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;
            let dev = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| e.to_string())?;
            let ep: IAudioEndpointVolume = dev
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| e.to_string())?;
            let scalar = (percent.min(100) as f32) / 100.0;
            ep.SetMasterVolumeLevelScalar(scalar, std::ptr::null())
                .map_err(|e| e.to_string())
        }
    }

    pub fn toggle_mic_mute() -> Result<bool, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;
            let dev = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .map_err(|e| e.to_string())?;
            let ep: IAudioEndpointVolume = dev
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| e.to_string())?;
            let currently_muted = ep.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            let next = !currently_muted;
            ep.SetMute(BOOL::from(next), std::ptr::null())
                .map_err(|e| e.to_string())?;
            Ok(next)
        }
    }

    pub fn lock_workstation() {
        unsafe {
            let _ = LockWorkStation();
        }
    }

    pub fn send_mouse_move(dx: i32, dy: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_mouse_click(right: bool) {
        let (down_flag, up_flag) = if right {
            (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
        } else {
            (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
        };
        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: down_flag,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: up_flag,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_mouse_scroll(delta: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_unicode_text(text: &str) {
        let mut inputs = Vec::new();
        for ch in text.encode_utf16() {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
        if !inputs.is_empty() {
            unsafe {
                SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }

    pub fn send_virtual_key(vk: u16) {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn scan_consent_store(sensor: &str) -> Vec<String> {
        let mut active = Vec::new();
        let sub_path = format!(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{}\\NonPackaged\0",
            sensor
        );
        let wide: Vec<u16> = sub_path.encode_utf16().collect();
        let val_name: Vec<u16> = "LastUsedTimeStop\0".encode_utf16().collect();
        let mut hkey = HKEY::default();
        unsafe {
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
            .is_err()
            {
                return active;
            }
            let mut idx = 0u32;
            loop {
                let mut name_buf = [0u16; 260];
                let mut name_len = name_buf.len() as u32;
                let res = RegEnumKeyExW(
                    hkey,
                    idx,
                    PWSTR(name_buf.as_mut_ptr()),
                    &mut name_len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                );
                if res.is_err() {
                    break;
                }
                idx += 1;
                let key_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                let mut sub_wide: Vec<u16> = key_name.encode_utf16().collect();
                sub_wide.push(0);
                let mut sub_hkey = HKEY::default();
                if RegOpenKeyExW(hkey, PCWSTR(sub_wide.as_ptr()), 0, KEY_READ, &mut sub_hkey)
                    .is_ok()
                {
                    let mut data = [0u8; 8];
                    let mut data_len = 8u32;
                    if RegQueryValueExW(
                        sub_hkey,
                        PCWSTR(val_name.as_ptr()),
                        None,
                        None,
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    )
                    .is_ok()
                    {
                        let stop_time = u64::from_le_bytes(data);
                        if stop_time == 0 {
                            let clean = key_name
                                .rsplit('#')
                                .next()
                                .unwrap_or(&key_name)
                                .to_string();
                            active.push(clean);
                        }
                    }
                    let _ = RegCloseKey(sub_hkey);
                }
            }
            let _ = RegCloseKey(hkey);
        }
        active
    }

    pub fn get_privacy_sensors() -> PrivacySensors {
        PrivacySensors {
            mic_active_apps: scan_consent_store("microphone"),
            cam_active_apps: scan_consent_store("webcam"),
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::{AudioState, PrivacySensors};

    pub fn apply_overlay_styles(_hwnd_raw: isize) {}
    pub fn set_capture_exclusion(_hwnd_raw: isize, _exclude: bool) {}
    pub fn is_foreground_fullscreen() -> bool {
        false
    }
    pub fn pick_cursor_hex_color() -> Result<String, String> {
        Ok("#FFFFFF".to_string())
    }
    pub fn get_audio_state() -> AudioState {
        AudioState {
            volume_percent: 65,
            output_muted: false,
            mic_muted: false,
        }
    }
    pub fn set_master_volume(_percent: u32) -> Result<(), String> {
        Ok(())
    }
    pub fn toggle_mic_mute() -> Result<bool, String> {
        Ok(true)
    }
    pub fn lock_workstation() {}
    pub fn send_mouse_move(_dx: i32, _dy: i32) {}
    pub fn send_mouse_click(_right: bool) {}
    pub fn send_mouse_scroll(_delta: i32) {}
    pub fn send_unicode_text(_text: &str) {}
    pub fn send_virtual_key(_vk: u16) {}
    pub fn get_privacy_sensors() -> PrivacySensors {
        PrivacySensors::default()
    }
}

pub use imp::*;
