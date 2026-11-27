//! Power and memory usage (plan section 8).
//!
//! - **EcoQoS** (Windows 11 efficiency mode): the scheduler runs the thread on
//!   efficiency cores at a lower frequency. This suits background dock work
//!   (window tracking and recomposition), but not animations, so it is toggled
//!   as needed.
//! - **Memory trimming**: an idle dock rarely touches most of its pages, so
//!   returning them to the system frees RAM for other apps. If needed again,
//!   they can be restored from the standby list in microseconds.

use std::{
    ffi::c_void,
    mem::size_of,
    sync::atomic::{AtomicU8, Ordering},
};
use windows::Win32::System::{
    ProcessStatus::EmptyWorkingSet,
    SystemServices::GUID_POWER_SAVING_STATUS,
    Threading::{
        GetCurrentProcess, GetCurrentThread, IDLE_PRIORITY_CLASS, MEMORY_PRIORITY_INFORMATION,
        MEMORY_PRIORITY_LOW, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        ProcessMemoryPriority, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
        SetThreadInformation, THREAD_POWER_THROTTLING_CURRENT_VERSION,
        THREAD_POWER_THROTTLING_EXECUTION_SPEED, THREAD_POWER_THROTTLING_STATE,
        ThreadPowerThrottling,
    },
};
use windows::core::GUID;

/// The newer notification distinguishes standard and high-savings modes.
const GUID_ENERGY_SAVER_STATUS: GUID = GUID::from_u128(0x550e8400_e29b_41d4_a716_446655440000);
/// `GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS`: fires on hardware brightness keys and OS slider changes.
const GUID_VIDEO_BRIGHTNESS: GUID = GUID::from_u128(0xaded5e82_b909_4619_9949_f5d71dac0bcb);
const UNKNOWN: u8 = 0;
const BATTERY_OFF: u8 = 1;
const BATTERY_ON: u8 = 2;
const ENERGY_OFF: u8 = 3;
const ENERGY_ON: u8 = 4;
static SAVER: SaverState = SaverState(AtomicU8::new(UNKNOWN));
static BRIGHTNESS_LAST: AtomicU8 = AtomicU8::new(u8::MAX);
static BRIGHTNESS_PENDING: AtomicU8 = AtomicU8::new(u8::MAX);

fn record_brightness(value: u32) {
    let pct = value.min(100) as u8;
    let prev = BRIGHTNESS_LAST.swap(pct, Ordering::Relaxed);
    if prev != u8::MAX && prev != pct {
        BRIGHTNESS_PENDING.store(pct, Ordering::Relaxed);
    }
}

/// Returns the latest hardware/OS display brightness percentage change, if any.
#[must_use]
pub fn take_brightness_change() -> Option<u8> {
    let pct = BRIGHTNESS_PENDING.swap(u8::MAX, Ordering::Relaxed);
    (pct <= 100).then_some(pct)
}

struct SaverState(AtomicU8);

impl SaverState {
    fn update(&self, setting: GUID, value: u32) {
        let next = match (setting, value) {
            (GUID_ENERGY_SAVER_STATUS, 0) => ENERGY_OFF,
            (GUID_ENERGY_SAVER_STATUS, 1 | 2) => ENERGY_ON,
            (GUID_POWER_SAVING_STATUS, 0) => BATTERY_OFF,
            (GUID_POWER_SAVING_STATUS, 1) => BATTERY_ON,
            _ => return,
        };
        // Once the modern source is available, a legacy notification cannot
        // overwrite it. Windows can deliver both to each registered window.
        let _ = self
            .0
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                (next >= ENERGY_OFF || current < ENERGY_OFF).then_some(next)
            });
    }

    fn get(&self) -> Option<bool> {
        match self.0.load(Ordering::Relaxed) {
            BATTERY_OFF | ENERGY_OFF => Some(false),
            BATTERY_ON | ENERGY_ON => Some(true),
            _ => None,
        }
    }
}

/// The last state supplied by Windows, or None before the initial notification.
pub(crate) fn notified_saver_state() -> Option<bool> {
    SAVER.get()
}

/// Records the DWORD carried by a PBT_POWERSETTINGCHANGE notification before
/// any redraw is deferred. This preserves the modern energy-saver state
/// instead of replacing it with the legacy battery-saver snapshot.
///
/// # Safety
/// `data` must be the live LPARAM Windows supplied for PBT_POWERSETTINGCHANGE.
/// Its pointed-to POWERBROADCAST_SETTING must remain valid during this call.
pub unsafe fn power_setting_changed(data: windows::Win32::Foundation::LPARAM) {
    // SAFETY: This function's caller guarantees the notification's lifetime.
    if let Some((setting, value)) = unsafe { saver_notification(data) } {
        if setting == GUID_VIDEO_BRIGHTNESS {
            record_brightness(value);
        } else {
            SAVER.update(setting, value);
        }
    }
}

unsafe fn saver_notification(data: windows::Win32::Foundation::LPARAM) -> Option<(GUID, u32)> {
    use windows::Win32::System::Power::POWERBROADCAST_SETTING;
    if data.0 == 0 {
        return None;
    }
    let setting = data.0 as *const POWERBROADCAST_SETTING;
    // SAFETY: The caller supplies the live broadcast structure. DataLength
    // guards reading the DWORD; read_unaligned handles the packed layout.
    unsafe {
        let guid = std::ptr::addr_of!((*setting).PowerSetting).read_unaligned();
        let size = std::ptr::addr_of!((*setting).DataLength).read_unaligned();
        if size == size_of::<u32>() as u32 {
            let value = std::ptr::addr_of!((*setting).Data)
                .cast::<u32>()
                .read_unaligned();
            return Some((guid, value));
        }
    }
    None
}

pub fn set_thread_efficiency(enabled: bool) {
    let state = THREAD_POWER_THROTTLING_STATE {
        Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if enabled {
            THREAD_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    };
    // SAFETY: The structure has its size initialized; if unsupported, the call
    // fails without effect.
    unsafe {
        let _ = SetThreadInformation(
            GetCurrentThread(),
            ThreadPowerThrottling,
            (&raw const state).cast::<c_void>(),
            size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

pub fn trim_memory() {
    // SAFETY: Trims this process's own working set.
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

/// Configures a process that only waits (the watchdog) with EcoQoS, minimum
/// CPU priority, and low-priority memory that the system can reclaim first.
pub fn run_as_background_process() {
    let throttling = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };
    let memory = MEMORY_PRIORITY_INFORMATION {
        MemoryPriority: MEMORY_PRIORITY_LOW,
    };
    // SAFETY: The structures have their sizes initialized and target this
    // process; a failure leaves the default settings unchanged.
    unsafe {
        let process = GetCurrentProcess();
        let _ = SetProcessInformation(
            process,
            ProcessPowerThrottling,
            (&raw const throttling).cast::<c_void>(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
        let _ = SetProcessInformation(
            process,
            ProcessMemoryPriority,
            (&raw const memory).cast::<c_void>(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        );
        let _ = SetPriorityClass(process, IDLE_PRIORITY_CLASS);
    }
    trim_memory();
}

/// Tells a window, with `WM_POWERBROADCAST` / `PBT_APMPOWERSTATUSCHANGE`, when
/// energy saver (battery saver on Windows 10) turns on or off: Windows then
/// stops or resumes see-through effects, and glass must follow. Plugging the
/// charger in or out already arrives as `PBT_APMPOWERSTATUSCHANGE`, but
/// turning the saver on by hand does not always.
///
/// Direct callbacks update the shared state before requesting a redraw. They
/// also supply the current value at registration, independently of the window's
/// message queue. Older systems retain the window-notification fallback.
/// No polling, extra timer or application-owned worker thread.
pub struct SaverWatch(Vec<SaverRegistration>);

struct SaverRegistration {
    handle: windows::Win32::System::Power::HPOWERNOTIFY,
    callback: bool,
}

unsafe extern "system" fn saver_changed(
    context: *const c_void,
    kind: u32,
    data: *const c_void,
) -> u32 {
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            PBT_APMPOWERSTATUSCHANGE, PBT_POWERSETTINGCHANGE, PostMessageW, WM_POWERBROADCAST,
        },
    };
    if kind == PBT_POWERSETTINGCHANGE {
        // SAFETY: Windows supplies the setting payload for this callback only.
        if let Some((setting, value)) = unsafe { saver_notification(LPARAM(data as isize)) } {
            if setting == GUID_VIDEO_BRIGHTNESS {
                record_brightness(value);
            } else {
                SAVER.update(setting, value);
            }
            if !context.is_null() {
                // Context is an HWND value, never a pointer into a window or
                // Rust object. No COM, UI borrow or synchronous send here.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(context.cast_mut())),
                        WM_POWERBROADCAST,
                        WPARAM(PBT_APMPOWERSTATUSCHANGE as usize),
                        LPARAM(0),
                    );
                }
            }
        }
    }
    0
}

impl SaverWatch {
    #[must_use]
    pub fn new(hwnd: isize) -> Self {
        use windows::Win32::{
            Foundation::{ERROR_SUCCESS, HANDLE},
            System::Power::{
                DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, PowerSettingRegisterNotification,
                RegisterPowerSettingNotification,
            },
            UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, DEVICE_NOTIFY_WINDOW_HANDLE},
        };
        let handles = [
            GUID_ENERGY_SAVER_STATUS,
            GUID_POWER_SAVING_STATUS,
            GUID_VIDEO_BRIGHTNESS,
        ]
            .iter()
            .filter_map(|setting| {
                let subscription = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                    Callback: Some(saver_changed),
                    Context: hwnd as *mut c_void,
                };
                let mut handle = std::ptr::null_mut();
                // SAFETY: Windows copies the callback and opaque HWND value.
                // Both remain valid without referencing this stack structure.
                if unsafe {
                    PowerSettingRegisterNotification(
                        setting,
                        DEVICE_NOTIFY_CALLBACK,
                        HANDLE((&raw const subscription).cast_mut().cast()),
                        &mut handle,
                    )
                } == ERROR_SUCCESS
                {
                    return Some(SaverRegistration {
                        handle: HPOWERNOTIFY(handle as isize),
                        callback: true,
                    });
                }
                // SAFETY: `hwnd` is the caller's live window; an unknown
                // setting on an older Windows just fails.
                unsafe {
                    RegisterPowerSettingNotification(
                        HANDLE(hwnd as *mut c_void),
                        setting,
                        DEVICE_NOTIFY_WINDOW_HANDLE,
                    )
                }
                .ok()
                .map(|handle| SaverRegistration {
                    handle,
                    callback: false,
                })
            })
            .collect();
        Self(handles)
    }
}

impl Drop for SaverWatch {
    fn drop(&mut self) {
        for registration in self.0.drain(..) {
            // SAFETY: Registered in `new` and released once.
            unsafe {
                if registration.callback {
                    let _ = windows::Win32::System::Power::PowerSettingUnregisterNotification(
                        registration.handle,
                    );
                } else {
                    let _ = windows::Win32::System::Power::UnregisterPowerSettingNotification(
                        registration.handle,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Hidden test-owned window; synthetic notifications do not change Windows power settings"]
    fn callback_updates_state_before_posting_redraw_and_never_forwards_payload_pointer()
    -> windows::core::Result<()> {
        use windows::Win32::{
            Foundation::{HWND, WPARAM},
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, MSG, PBT_APMPOWERSTATUSCHANGE,
                PBT_POWERSETTINGCHANGE, PM_REMOVE, PeekMessageW, WINDOW_EX_STYLE,
                WM_POWERBROADCAST, WS_POPUP,
            },
        };
        use windows::core::w;
        struct Fixture(HWND, u8);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
                SAVER.0.store(self.1, Ordering::Relaxed);
            }
        }
        let fixture = Fixture(
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("Nosih power test"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    None,
                    None,
                )?
            },
            SAVER.0.load(Ordering::Relaxed),
        );
        #[repr(C)]
        struct Payload {
            setting: GUID,
            length: u32,
            value: u32,
        }
        for (value, enabled) in [(1, true), (0, false)] {
            let payload = Payload {
                setting: GUID_ENERGY_SAVER_STATUS,
                length: 4,
                value,
            };
            unsafe {
                saver_changed(
                    fixture.0.0.cast_const(),
                    PBT_POWERSETTINGCHANGE,
                    (&raw const payload).cast(),
                );
            }
            assert_eq!(notified_saver_state(), Some(enabled));
            let mut message = MSG::default();
            assert!(
                unsafe {
                    PeekMessageW(
                        &mut message,
                        Some(fixture.0),
                        WM_POWERBROADCAST,
                        WM_POWERBROADCAST,
                        PM_REMOVE,
                    )
                }
                .as_bool()
            );
            assert_eq!(message.wParam, WPARAM(PBT_APMPOWERSTATUSCHANGE as usize));
            assert_eq!(message.lParam.0, 0);
        }
        Ok(())
    }

    #[test]
    #[ignore = "Read-only native power subscriptions; no settings changes or windows"]
    fn native_callbacks_supply_initial_state_without_a_window_message_loop() {
        use std::time::{Duration, Instant};
        for _ in 0..3 {
            SAVER.0.store(UNKNOWN, Ordering::Relaxed);
            let watch = SaverWatch::new(0);
            assert!(watch.0.iter().any(|registration| registration.callback));
            let end = Instant::now() + Duration::from_secs(2);
            while SAVER.get().is_none() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                SAVER.get().is_some(),
                "Windows did not supply its current saver state"
            );
            let current = SAVER.0.load(Ordering::Relaxed);
            drop(watch);
            if current >= ENERGY_OFF {
                // Simulate a stale startup cache, without changing Windows.
                SAVER.0.store(
                    if current == ENERGY_OFF {
                        ENERGY_ON
                    } else {
                        ENERGY_OFF
                    },
                    Ordering::Relaxed,
                );
                let _watch = SaverWatch::new(0);
                let end = Instant::now() + Duration::from_secs(2);
                while SAVER.0.load(Ordering::Relaxed) != current && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(SAVER.0.load(Ordering::Relaxed), current);
            }
        }
    }

    #[test]
    fn saver_notifications_follow_standard_high_savings_and_off() {
        let state = SaverState(AtomicU8::new(UNKNOWN));
        assert_eq!(state.get(), None);
        for (value, enabled) in [(0, false), (1, true), (2, true), (0, false)] {
            state.update(GUID_ENERGY_SAVER_STATUS, value);
            assert_eq!(state.get(), Some(enabled));
        }
    }

    #[test]
    fn legacy_notifications_cannot_override_the_modern_saver_status() {
        let state = SaverState(AtomicU8::new(UNKNOWN));
        state.update(GUID_POWER_SAVING_STATUS, 1);
        assert_eq!(state.get(), Some(true));
        state.update(GUID_POWER_SAVING_STATUS, 0);
        assert_eq!(state.get(), Some(false));
        state.update(GUID_ENERGY_SAVER_STATUS, 1);
        state.update(GUID_POWER_SAVING_STATUS, 0);
        assert_eq!(state.get(), Some(true));
        state.update(GUID_ENERGY_SAVER_STATUS, 0);
        state.update(GUID_POWER_SAVING_STATUS, 1);
        assert_eq!(state.get(), Some(false));
    }

    #[test]
    fn unknown_guids_and_values_preserve_the_last_known_status() {
        let state = SaverState(AtomicU8::new(UNKNOWN));
        state.update(GUID::zeroed(), 1);
        state.update(GUID_ENERGY_SAVER_STATUS, 99);
        assert_eq!(state.get(), None);
        state.update(GUID_ENERGY_SAVER_STATUS, 1);
        state.update(GUID_POWER_SAVING_STATUS, 99);
        assert_eq!(state.get(), Some(true));
    }

    #[test]
    fn notification_reader_checks_the_length_and_reads_the_dword() {
        use windows::Win32::{Foundation::LPARAM, System::Power::POWERBROADCAST_SETTING};
        #[repr(C)]
        struct Message {
            setting: GUID,
            length: u32,
            value: u32,
        }
        let message = Message {
            setting: GUID_ENERGY_SAVER_STATUS,
            length: 4,
            value: 2,
        };
        let data = LPARAM((&raw const message).cast::<POWERBROADCAST_SETTING>() as isize);
        // SAFETY: The fixture includes the complete header and DWORD payload.
        assert_eq!(
            unsafe { saver_notification(data) },
            Some((GUID_ENERGY_SAVER_STATUS, 2))
        );
        let short = Message {
            length: 1,
            ..message
        };
        let data = LPARAM((&raw const short).cast::<POWERBROADCAST_SETTING>() as isize);
        // SAFETY: The fixture remains live and contains the complete header.
        assert_eq!(unsafe { saver_notification(data) }, None);
        // SAFETY: Null is explicitly accepted as an absent notification.
        assert_eq!(unsafe { saver_notification(LPARAM(0)) }, None);
    }
}
