//! Installed keyboard layouts and a nonblocking change request to the window
//! the user was typing in before opening Nosih's menu.

use windows::{
    Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, ERROR_INVALID_WINDOW_HANDLE, LPARAM, WPARAM},
        Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDDISPLAYNAME},
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        UI::{
            Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL},
            WindowsAndMessaging::{
                GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
                IsWindow, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
            },
        },
    },
    core::{HSTRING, Result, w},
};

#[derive(Clone, Copy)]
pub struct Context {
    window: isize,
    target: isize,
    thread: u32,
    pub layout: isize,
}

pub struct Layout {
    pub handle: isize,
    pub name: String,
}

impl Context {
    #[must_use]
    pub fn current() -> Option<Self> {
        // SAFETY: Read-only queries. A missing focus window falls back to the
        // foreground window; the request validates it again before posting.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_invalid() {
                return None;
            }
            let thread = GetWindowThreadProcessId(window, None);
            if thread == 0 {
                return None;
            }
            let mut info = GUITHREADINFO {
                cbSize: size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let target =
                if GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid() {
                    info.hwndFocus
                } else {
                    window
                };
            let thread = GetWindowThreadProcessId(target, None);
            if thread == 0 {
                return None;
            }
            Some(Self {
                window: window.0 as isize,
                target: target.0 as isize,
                thread,
                layout: GetKeyboardLayout(thread).0 as isize,
            })
        }
    }

    /// Does not activate a layout in Nosih's thread or synthesize keyboard
    /// shortcuts. The receiving application remains in control of its input.
    pub fn request(self, layout: isize) -> Result<()> {
        use windows::Win32::Foundation::HWND;
        let window = HWND(self.window as *mut _);
        let target = HWND(self.target as *mut _);
        // SAFETY: Scalar handles are revalidated because the menu can outlive
        // its original application. UIPI refusals are returned to the caller.
        unsafe {
            if !IsWindow(Some(window)).as_bool()
                || !IsWindow(Some(target)).as_bool()
                || GetWindowThreadProcessId(target, None) != self.thread
            {
                return Err(ERROR_INVALID_WINDOW_HANDLE.into());
            }
            if !handles().contains(&layout) {
                return Err(ERROR_INVALID_PARAMETER.into());
            }
            PostMessageW(
                Some(target),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(layout),
            )
        }
    }
}

fn handles() -> Vec<isize> {
    // SAFETY: Windows writes at most the allocated number of layout handles.
    unsafe {
        let count = GetKeyboardLayoutList(None).max(0) as usize;
        let mut layouts = vec![HKL::default(); count];
        let read = GetKeyboardLayoutList(Some(&mut layouts)).max(0) as usize;
        layouts.truncate(read);
        layouts
            .into_iter()
            .map(|layout| layout.0 as isize)
            .collect()
    }
}

#[must_use]
pub fn layouts(fallback: &str) -> Vec<Layout> {
    handles()
        .into_iter()
        .enumerate()
        .map(|(index, handle)| {
            let name = layout_name(handle)
                .or_else(|| locale_name(handle))
                .unwrap_or_else(|| format!("{fallback} {}", index + 1));
            Layout { handle, name }
        })
        .collect()
}

fn registry_text(key: &str, value: &str) -> Option<String> {
    let path = HSTRING::from(format!(
        r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts\{key}"
    ));
    let value = HSTRING::from(value);
    let mut buffer = [0u16; 512];
    let mut bytes = size_of_val(&buffer) as u32;
    // SAFETY: Read-only registry query into a bounded UTF-16 buffer.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &path,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..end]);
    (!text.is_empty()).then_some(text)
}

fn direct_key(handle: isize) -> Option<String> {
    let raw = handle as u32;
    let device = raw >> 16;
    if device & 0xF000 == 0xF000 {
        return None;
    }
    Some(format!(
        "{:08X}",
        if device == raw & 0xFFFF {
            raw & 0xFFFF
        } else {
            raw
        }
    ))
}

fn layout_name(handle: isize) -> Option<String> {
    if let Some(key) = direct_key(handle) {
        return display_name(&key);
    }
    use windows::{
        Win32::System::Registry::{HKEY, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW},
        core::PWSTR,
    };
    let mut key = HKEY::default();
    // Device identifiers Fxxx refer to a registry "Layout Id", not a KLID.
    // Enumerate only when such a variant is present, and only when opening
    // the menu. No registry work runs on mouse moves or the menu bar clock.
    unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts"),
            None,
            KEY_READ,
            &mut key,
        )
        .ok()
        .ok()?;
        let result = (|| {
            for index in 0..1024 {
                let mut name = [0u16; 32];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(
                    key,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
                .is_err()
                {
                    break;
                }
                let name = String::from_utf16_lossy(&name[..len as usize]);
                let id = u32::from_str_radix(&name, 16).ok();
                if id.is_none_or(|id| id & 0xFFFF != handle as u32 & 0xFFFF) {
                    continue;
                }
                let variant = registry_text(&name, "Layout Id")
                    .and_then(|id| u32::from_str_radix(&id, 16).ok());
                if variant == Some((handle as u32 >> 16) & 0xFFF) {
                    return display_name(&name);
                }
            }
            None
        })();
        let _ = RegCloseKey(key);
        result
    }
}

fn display_name(key: &str) -> Option<String> {
    if let Some(resource) = registry_text(key, "Layout Display Name") {
        let mut name = [0u16; 512];
        // SAFETY: Resolves a Windows MUI resource into a bounded buffer.
        if unsafe {
            windows::Win32::UI::Shell::SHLoadIndirectString(
                &HSTRING::from(resource),
                &mut name,
                None,
            )
        }
        .is_ok()
        {
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            if end > 0 {
                return Some(String::from_utf16_lossy(&name[..end]));
            }
        }
    }
    registry_text(key, "Layout Text")
}

fn locale_name(handle: isize) -> Option<String> {
    let mut name = [0u16; 256];
    // SAFETY: Read-only locale query into a live UTF-16 buffer.
    let len = unsafe {
        GetLocaleInfoW(
            handle as u32 & 0xFFFF,
            LOCALE_SLOCALIZEDDISPLAYNAME,
            Some(&mut name),
        )
    };
    (len > 1).then(|| String::from_utf16_lossy(&name[..len as usize - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_handles_are_not_registry_identifiers() {
        assert_eq!(direct_key(0x04090409), Some("00000409".into()));
        assert_eq!(direct_key(0x080A080A), Some("0000080A".into()));
        assert_eq!(direct_key(0xE0010411), Some("E0010411".into()));
        assert_eq!(direct_key(0xF0020409), None);
    }

    #[test]
    fn installed_layouts_have_names_and_preserve_all_handles() {
        let expected = handles();
        assert!(!expected.is_empty());
        let layouts = layouts("Keyboard");
        assert_eq!(
            layouts
                .iter()
                .map(|layout| layout.handle)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(layouts.iter().all(|layout| !layout.name.is_empty()));
    }

    #[test]
    fn a_closed_input_target_cannot_receive_a_layout_request() {
        let context = Context {
            window: 0,
            target: 0,
            thread: 0,
            layout: 0,
        };
        assert!(context.request(0).is_err());
    }

    #[test]
    fn layout_change_is_posted_to_the_captured_target_only() -> Result<()> {
        use windows::Win32::{
            System::Threading::GetCurrentThreadId,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW, WS_POPUP,
            },
        };
        // A hidden test-owned window: no foreground or keyboard is changed.
        unsafe {
            let window = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Nosih keyboard test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?;
            let layout = GetKeyboardLayout(0).0 as isize;
            let context = Context {
                window: window.0 as isize,
                target: window.0 as isize,
                thread: GetCurrentThreadId(),
                layout,
            };
            let result = context.request(layout);
            let mut msg = MSG::default();
            let received = PeekMessageW(
                &mut msg,
                Some(window),
                WM_INPUTLANGCHANGEREQUEST,
                WM_INPUTLANGCHANGEREQUEST,
                PM_REMOVE,
            )
            .as_bool();
            let _ = DestroyWindow(window);
            result?;
            assert!(received);
            assert_eq!(msg.lParam.0, layout);
            assert_eq!(GetKeyboardLayout(0).0 as isize, layout);
            Ok(())
        }
    }
}
