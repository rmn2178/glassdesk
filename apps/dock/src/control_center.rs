//! Background worker for Nosih's native macOS-style Control Center.
//!
//! All WinRT radio toggles (Wi-Fi, Bluetooth), display brightness reads/writes,
//! Dark Mode switches, and Do Not Disturb updates run on a dedicated MTA thread
//! so the menu bar UI thread never blocks.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicIsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

use nosih_shell::control_center as shell_cc;
use windows::{
    Devices::Radios::{Radio, RadioKind, RadioState},
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::RuntimeType,
};
use windows_future::{AsyncOperationCompletedHandler, IAsyncOperation};

const RADIO_TIMEOUT: Duration = Duration::from_secs(3);

/// Snapshot of Control Center states rendered by `MenuBar`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub wifi_on: bool,
    pub wifi_name: Option<String>,
    pub bluetooth_on: bool,
    pub do_not_disturb: bool,
    pub dark_mode: bool,
    pub brightness: Option<u8>,
    pub power_mode: shell_cc::PowerMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Refresh,
    ToggleWifi,
    ToggleBluetooth,
    ToggleDoNotDisturb,
    ToggleDarkMode,
    SetBrightness(u8),
    CyclePowerMode,
}

pub struct Controller {
    window: Arc<AtomicIsize>,
    state: Arc<Mutex<State>>,
    sender: SyncSender<Command>,
}

impl Controller {
    pub fn start(hwnd: HWND, message: u32) -> Option<Self> {
        let window = Arc::new(AtomicIsize::new(hwnd.0 as isize));
        let initial = State {
            wifi_name: shell_cc::wifi_name(),
            do_not_disturb: shell_cc::do_not_disturb(),
            dark_mode: shell_cc::is_dark_mode(),
            ..State::default()
        };
        let state = Arc::new(Mutex::new(initial));
        let (sender, receiver) = mpsc::sync_channel(16);
        {
            let window = window.clone();
            let state = state.clone();
            thread::Builder::new()
                .name("nosih-control-center".into())
                .spawn(move || {
                    // SAFETY: New dedicated worker thread; apartment is balanced on exit.
                    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok() {
                        run(&window, message, &state, &receiver);
                        unsafe { RoUninitialize() };
                    }
                })
                .ok()?;
        }
        Some(Self {
            window,
            state,
            sender,
        })
    }

    pub fn snapshot(&self) -> State {
        snapshot_state(&self.state)
    }

    pub fn refresh(&self) {
        let _ = self.sender.try_send(Command::Refresh);
    }

    pub fn toggle_wifi(&self) {
        let _ = self.sender.try_send(Command::ToggleWifi);
    }

    pub fn toggle_bluetooth(&self) {
        let _ = self.sender.try_send(Command::ToggleBluetooth);
    }

    pub fn toggle_do_not_disturb(&self) {
        let _ = self.sender.try_send(Command::ToggleDoNotDisturb);
    }

    pub fn toggle_dark_mode(&self) {
        let _ = self.sender.try_send(Command::ToggleDarkMode);
    }

    pub fn set_brightness(&self, percent: u8) {
        let _ = self.sender.try_send(Command::SetBrightness(percent.min(100)));
    }

    pub fn cycle_power_mode(&self) {
        let _ = self.sender.try_send(Command::CyclePowerMode);
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.window.store(0, Ordering::Release);
    }
}

fn complete<T: RuntimeType>(operation: IAsyncOperation<T>) -> Option<T> {
    let (send, receive) = mpsc::channel();
    operation
        .SetCompleted(&AsyncOperationCompletedHandler::<T>::new(move |_, _| {
            let _ = send.send(());
            Ok(())
        }))
        .ok()?;
    match receive.recv_timeout(RADIO_TIMEOUT) {
        Ok(()) => operation.GetResults().ok(),
        Err(_) => {
            let _ = operation.Cancel();
            None
        }
    }
}

fn query_radios() -> (bool, bool) {
    let Some(radios) = Radio::GetRadiosAsync().ok().and_then(complete) else {
        return (false, false);
    };
    let mut wifi_on = false;
    let mut bt_on = false;
    for radio in radios {
        let Ok(kind) = radio.Kind() else {
            continue;
        };
        let on = radio.State().is_ok_and(|state| state == RadioState::On);
        match kind {
            RadioKind::WiFi => wifi_on |= on,
            RadioKind::Bluetooth => bt_on |= on,
            _ => {}
        }
    }
    (wifi_on, bt_on)
}

fn set_radio_kind(target_kind: RadioKind, enable: bool) {
    // RequestAccessAsync is required before SetStateAsync on Windows 10/11.
    let _ = Radio::RequestAccessAsync().ok().and_then(complete);
    let Some(radios) = Radio::GetRadiosAsync().ok().and_then(complete) else {
        return;
    };
    let target = if enable {
        RadioState::On
    } else {
        RadioState::Off
    };
    for radio in radios {
        if radio.Kind().ok() != Some(target_kind) {
            continue;
        }
        if let Ok(op) = radio.SetStateAsync(target) {
            let _ = complete(op);
        }
    }
}

fn read_full_state() -> State {
    let (wifi_on, bluetooth_on) = query_radios();
    let wifi_name = shell_cc::wifi_name();
    State {
        wifi_on: wifi_on || wifi_name.is_some(),
        wifi_name,
        bluetooth_on,
        do_not_disturb: shell_cc::do_not_disturb(),
        dark_mode: shell_cc::is_dark_mode(),
        brightness: shell_cc::display_brightness(),
        power_mode: shell_cc::power_mode(),
    }
}

fn snapshot_state(state: &Mutex<State>) -> State {
    state.lock().map_or_else(|_| State::default(), |guard| guard.clone())
}

fn publish(window: &AtomicIsize, message: u32, state: &Mutex<State>, next: State) {
    let changed = match state.lock() {
        Ok(mut guard) => {
            if *guard == next {
                false
            } else {
                *guard = next;
                true
            }
        }
        Err(_) => false,
    };
    if changed {
        let hwnd = window.load(Ordering::Acquire);
        if hwnd != 0 {
            // SAFETY: Posts a payload-free notification to the live MenuBar window.
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Coalesces consecutive `SetBrightness` commands so dragging never builds a backlog.
fn coalesce_brightness(first: u8, receiver: &Receiver<Command>) -> (u8, Option<Command>) {
    let mut latest = first;
    while let Ok(next) = receiver.try_recv() {
        match next {
            Command::SetBrightness(value) => latest = value,
            other => return (latest, Some(other)),
        }
    }
    (latest, None)
}

fn run(
    window: &AtomicIsize,
    message: u32,
    state: &Mutex<State>,
    receiver: &Receiver<Command>,
) {
    let mut deferred: Option<Command> = None;
    loop {
        if window.load(Ordering::Acquire) == 0 {
            break;
        }
        let cmd = match deferred.take() {
            Some(cmd) => cmd,
            None => match receiver.recv() {
                Ok(cmd) => cmd,
                Err(_) => break,
            },
        };
        if window.load(Ordering::Acquire) == 0 {
            break;
        }
        match cmd {
            Command::Refresh => {
                let next = read_full_state();
                publish(window, message, state, next);
            }
            Command::ToggleWifi => {
                let (wifi_on, _) = query_radios();
                set_radio_kind(RadioKind::WiFi, !wifi_on);
                let next = read_full_state();
                publish(window, message, state, next);
            }
            Command::ToggleBluetooth => {
                let (_, bluetooth_on) = query_radios();
                set_radio_kind(RadioKind::Bluetooth, !bluetooth_on);
                let next = read_full_state();
                publish(window, message, state, next);
            }
            Command::ToggleDoNotDisturb => {
                let next_dnd = !shell_cc::do_not_disturb();
                let _ = shell_cc::set_do_not_disturb(next_dnd);
                let mut next = snapshot_state(state);
                next.do_not_disturb = shell_cc::do_not_disturb();
                publish(window, message, state, next);
            }
            Command::ToggleDarkMode => {
                let next_dark = !shell_cc::is_dark_mode();
                let _ = shell_cc::set_dark_mode(next_dark);
                let mut next = snapshot_state(state);
                next.dark_mode = shell_cc::is_dark_mode();
                publish(window, message, state, next);
            }
            Command::SetBrightness(first) => {
                let (target, leftover) = coalesce_brightness(first, receiver);
                deferred = leftover;
                if shell_cc::set_display_brightness(target) {
                    let mut next = snapshot_state(state);
                    next.brightness = Some(target);
                    publish(window, message, state, next);
                }
            }
            Command::CyclePowerMode => {
                let target = shell_cc::power_mode().next();
                let _ = shell_cc::set_power_mode(target);
                let mut next = snapshot_state(state);
                next.power_mode = shell_cc::power_mode();
                publish(window, message, state, next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_drag_coalesces_to_latest_value_without_dropping_other_commands() {
        let (tx, rx) = mpsc::sync_channel(8);
        let _ = tx.send(Command::SetBrightness(25));
        let _ = tx.send(Command::SetBrightness(60));
        let _ = tx.send(Command::SetBrightness(82));
        let _ = tx.send(Command::ToggleDarkMode);
        let (target, next) = coalesce_brightness(10, &rx);
        assert_eq!(target, 82);
        assert_eq!(next, Some(Command::ToggleDarkMode));
    }

    #[test]
    fn brightness_drag_coalesces_all_pending_values_when_queue_ends() {
        let (tx, rx) = mpsc::sync_channel(8);
        let _ = tx.send(Command::SetBrightness(40));
        let _ = tx.send(Command::SetBrightness(95));
        let (target, next) = coalesce_brightness(15, &rx);
        assert_eq!(target, 95);
        assert_eq!(next, None);
    }
}
