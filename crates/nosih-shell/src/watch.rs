//! Telling a window when a registry key changes, without polling and without
//! a thread of its own.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WPARAM},
        System::{
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET,
                REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC, RegCloseKey,
                RegNotifyChangeKeyValue, RegOpenKeyExW,
            },
            Threading::{
                CreateEventW, INFINITE, RegisterWaitForSingleObject, UnregisterWaitEx,
                WT_EXECUTEONLYONCE,
            },
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::PCWSTR,
};

/// Posts `message` to a window when a key under `HKEY_CURRENT_USER` changes:
/// a value is written, or (with `subtree`) a key below it is added, removed
/// or written.
///
/// The registry signals an event and the system thread pool posts the
/// message. Each registration fires once; the receiver calls
/// [`RegistryWatch::arm`] again when it handles the message, so a burst of
/// changes costs one message until then.
pub struct RegistryWatch {
    key: HKEY,
    subtree: bool,
    event: HANDLE,
    wait: HANDLE,
    /// Where the thread pool posts: window and message. Boxed so its address,
    /// handed to the thread pool, never moves.
    target: Box<Target>,
}

struct Target {
    window: isize,
    message: u32,
    notified: AtomicBool,
}

impl RegistryWatch {
    #[must_use]
    pub fn new(path: PCWSTR, subtree: bool, hwnd: isize, message: u32) -> Option<Self> {
        let mut key = HKEY::default();
        // SAFETY: Opens a key for notifications only; closed in `Drop`.
        unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, path, None, KEY_NOTIFY, &mut key)
                .ok()
                .ok()?;
        }
        // SAFETY: An auto-reset event owned by this watch, closed in `Drop`.
        let Ok(event) = (unsafe { CreateEventW(None, false, false, None) }) else {
            // SAFETY: The key opened above.
            unsafe {
                let _ = RegCloseKey(key);
            }
            return None;
        };
        let mut watch = Self {
            key,
            subtree,
            event,
            wait: HANDLE::default(),
            target: Box::new(Target {
                window: hwnd,
                message,
                notified: AtomicBool::new(false),
            }),
        };
        watch.arm().then_some(watch)
    }

    /// Listens for the next change.
    pub fn arm(&mut self) -> bool {
        // Several watches can share one UI notification. Do not register a
        // second notification on a key whose existing wait has not fired.
        if !self.wait.is_invalid() && !self.target.notified.swap(false, Ordering::AcqRel) {
            return true;
        }
        // SAFETY: Every handle belongs to this watch; `target` outlives the
        // wait, which `Drop` cancels before freeing it.
        unsafe {
            if !self.wait.is_invalid() {
                // The callback only posts a message. Finish it before replacing
                // the wait so Drop can account for every use of the target.
                let _ = UnregisterWaitEx(self.wait, Some(INVALID_HANDLE_VALUE));
                self.wait = HANDLE::default();
            }
            // Thread-agnostic, so the registration survives whichever thread made it.
            let filter =
                REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_THREAD_AGNOSTIC;
            if RegNotifyChangeKeyValue(self.key, self.subtree, filter, Some(self.event), true)
                .is_err()
            {
                return false;
            }
            RegisterWaitForSingleObject(
                &mut self.wait,
                self.event,
                Some(changed),
                Some(std::ptr::from_ref(&*self.target).cast()),
                INFINITE,
                WT_EXECUTEONLYONCE,
            )
            .is_ok()
        }
    }
}

unsafe extern "system" fn changed(context: *mut std::ffi::c_void, _timed_out: bool) {
    // SAFETY: `context` points at the watch's boxed target, alive until the
    // wait is cancelled.
    let target = unsafe { &*context.cast::<Target>() };
    target.notified.store(true, Ordering::Release);
    // SAFETY: Posting is asynchronous; a closed window just drops it.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(target.window as *mut _)),
            target.message,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

impl Drop for RegistryWatch {
    fn drop(&mut self) {
        // SAFETY: Waits for a running callback to finish before the target
        // is freed, then releases this watch's handles once.
        unsafe {
            if !self.wait.is_invalid() {
                let _ = UnregisterWaitEx(self.wait, Some(INVALID_HANDLE_VALUE));
            }
            let _ = CloseHandle(self.event);
            let _ = RegCloseKey(self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use windows::{
        Win32::{
            System::Registry::{
                KEY_SET_VALUE, REG_DWORD, REG_OPTION_VOLATILE, RegCreateKeyExW, RegDeleteKeyW,
                RegSetValueExW,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW, WM_APP, WS_POPUP,
            },
        },
        core::{HSTRING, Result, w},
    };

    #[test]
    #[ignore = "Native registry notification test; writes only a temporary test key"]
    fn native_shared_notifications_do_not_duplicate_waits_and_can_rearm() -> Result<()> {
        let path = HSTRING::from(format!(
            r"Software\NosihRegistryWatchTest-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &path,
                None,
                None,
                REG_OPTION_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()?;
        }
        struct Key(HKEY, HSTRING);
        impl Drop for Key {
            fn drop(&mut self) {
                unsafe {
                    let _ = RegCloseKey(self.0);
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &self.1);
                }
            }
        }
        let _key = Key(key, path.clone());
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Nosih watch test"),
                WS_POPUP,
                -32000,
                -32000,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        };
        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let _window = Window(window);
        let mut watch = RegistryWatch::new(
            PCWSTR(path.as_ptr()),
            false,
            window.0 as isize,
            WM_APP + 300,
        )
        .ok_or_else(windows::core::Error::from_thread)?;
        let initial_wait = watch.wait;
        for _ in 0..10 {
            assert!(watch.arm());
            assert_eq!(
                watch.wait, initial_wait,
                "unrelated notifications must keep the existing wait"
            );
        }
        for value in [0u32, 1, 0] {
            unsafe {
                RegSetValueExW(
                    key,
                    w!("visibility"),
                    None,
                    REG_DWORD,
                    Some(&value.to_le_bytes()),
                )
                .ok()?;
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut message = MSG::default();
            loop {
                if unsafe {
                    PeekMessageW(
                        &mut message,
                        Some(window),
                        WM_APP + 300,
                        WM_APP + 300,
                        PM_REMOVE,
                    )
                    .as_bool()
                } {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "registry notification deadline exceeded"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(watch.target.notified.load(Ordering::Acquire));
            assert!(watch.arm());
            assert!(!watch.target.notified.load(Ordering::Acquire));
        }
        Ok(())
    }
}
