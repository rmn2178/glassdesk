//! Read-only radio status. Windows supplies state and hotplug events; there is
//! no polling thread, permission prompt or attempt to change the system radio.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
};
use windows::{
    Devices::{
        Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher},
        Radios::{Radio, RadioKind, RadioState},
    },
    Foundation::TypedEventHandler,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::{Ref, Result},
};
use windows_future::{AsyncOperationCompletedHandler, IAsyncOperation};

struct Subscription {
    radio: Radio,
    token: i64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.radio.RemoveStateChanged(self.token);
    }
}

struct Device {
    on: AtomicBool,
    pending: Mutex<Option<IAsyncOperation<Radio>>>,
    subscription: Mutex<Option<Subscription>>,
    owner: Weak<Shared>,
}

impl Device {
    fn refresh(&self, radio: &Radio) {
        // Serialize reads with the initial reading, so an earlier reading
        // cannot overwrite a newer state event. The UI reads only the atomic.
        if let Ok(_guard) = self.subscription.lock() {
            self.on.store(
                radio.State().is_ok_and(|state| state == RadioState::On),
                Ordering::Release,
            );
        }
        if let Some(owner) = self.owner.upgrade() {
            owner.publish();
        }
    }

    fn connect(self: &Arc<Self>, radio: Radio) -> Result<()> {
        if radio.Kind()? != RadioKind::Bluetooth {
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        let token = radio.StateChanged(&TypedEventHandler::new(move |radio: Ref<Radio>, _| {
            if let (Some(device), Some(radio)) = (weak.upgrade(), radio.as_ref()) {
                device.refresh(radio);
            }
            Ok(())
        }))?;
        let subscription = Subscription {
            radio: radio.clone(),
            token,
        };
        let previous = self
            .subscription
            .lock()
            .ok()
            .and_then(|mut slot| slot.replace(subscription));
        drop(previous);
        self.refresh(&radio);
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if let Ok(pending) = self.pending.get_mut()
            && let Some(operation) = pending.take()
        {
            let _ = operation.Cancel();
        }
    }
}

struct Shared {
    window: AtomicIsize,
    message: u32,
    on: AtomicBool,
    devices: Mutex<HashMap<String, Arc<Device>>>,
}

impl Shared {
    fn publish(&self) {
        let Ok(devices) = self.devices.lock() else {
            return;
        };
        let on = devices
            .values()
            .any(|device| device.on.load(Ordering::Acquire));
        // Compare under the same lock as hotplug aggregation; two callbacks
        // cannot publish their snapshots in reverse order.
        if self.on.swap(on, Ordering::AcqRel) != on {
            let window = self.window.load(Ordering::Acquire);
            if window != 0 {
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        self.message,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }

    fn added(self: &Arc<Self>, info: &DeviceInformation) -> Result<()> {
        let id = info.Id()?;
        let device = Arc::new(Device {
            on: AtomicBool::new(false),
            pending: Mutex::new(None),
            subscription: Mutex::new(None),
            owner: Arc::downgrade(self),
        });
        let previous = match self.devices.lock() {
            Ok(mut devices) => {
                if self.window.load(Ordering::Acquire) == 0 {
                    return Ok(());
                }
                devices.insert(id.to_string_lossy(), device.clone())
            }
            Err(_) => return Ok(()),
        };
        drop(previous); // Unsubscribe outside the map lock.
        self.publish();
        let operation = Radio::FromIdAsync(&id)?;
        if let Ok(mut pending) = device.pending.lock() {
            *pending = Some(operation.clone());
        }
        let weak = Arc::downgrade(&device);
        let result =
            operation.SetCompleted(&AsyncOperationCompletedHandler::new(move |operation, _| {
                if let Some(device) = weak.upgrade() {
                    if let Some(operation) = operation.as_ref()
                        && let Ok(radio) = operation.GetResults()
                    {
                        let _ = device.connect(radio);
                    }
                    if let Ok(mut pending) = device.pending.lock() {
                        pending.take();
                    }
                }
                Ok(())
            }));
        if result.is_err()
            && let Ok(mut pending) = device.pending.lock()
            && let Some(operation) = pending.take()
        {
            let _ = operation.Cancel();
        }
        result
    }

    fn removed(&self, id: &str) {
        let removed = self
            .devices
            .lock()
            .ok()
            .and_then(|mut devices| devices.remove(id));
        drop(removed);
        self.publish();
    }
}

pub struct Monitor {
    shared: Arc<Shared>,
    watcher: DeviceWatcher,
    added: Option<i64>,
    removed: Option<i64>,
}

impl Monitor {
    pub fn new(window: isize, message: u32) -> Result<Self> {
        let mut monitor = Self {
            shared: Arc::new(Shared {
                window: AtomicIsize::new(window),
                message,
                on: AtomicBool::new(false),
                devices: Mutex::new(HashMap::new()),
            }),
            watcher: DeviceInformation::CreateWatcherAqsFilter(&Radio::GetDeviceSelector()?)?,
            added: None,
            removed: None,
        };
        let weak = Arc::downgrade(&monitor.shared);
        monitor.added = Some(monitor.watcher.Added(&TypedEventHandler::new(
            move |_, info: Ref<DeviceInformation>| {
                if let (Some(shared), Some(info)) = (weak.upgrade(), info.as_ref()) {
                    let _ = shared.added(info);
                }
                Ok(())
            },
        ))?);
        let weak = Arc::downgrade(&monitor.shared);
        monitor.removed = Some(monitor.watcher.Removed(&TypedEventHandler::new(
            move |_, info: Ref<DeviceInformationUpdate>| {
                if let (Some(shared), Some(info)) = (weak.upgrade(), info.as_ref())
                    && let Ok(id) = info.Id()
                {
                    shared.removed(&id.to_string_lossy());
                }
                Ok(())
            },
        ))?);
        monitor.watcher.Start()?;
        Ok(monitor)
    }

    pub fn is_on(&self) -> bool {
        self.shared.on.load(Ordering::Acquire)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        // Serialize shutdown with publish(), so an in-flight callback cannot
        // post to this HWND after its owner goes away and Windows reuses it.
        let devices = self.shared.devices.lock().ok().map(|mut devices| {
            self.shared.window.store(0, Ordering::Release);
            self.shared.on.store(false, Ordering::Release);
            std::mem::take(&mut *devices)
        });
        self.shared.window.store(0, Ordering::Release);
        if let Some(token) = self.added.take() {
            let _ = self.watcher.RemoveAdded(token);
        }
        if let Some(token) = self.removed.take() {
            let _ = self.watcher.RemoveRemoved(token);
        }
        let _ = self.watcher.Stop();
        drop(devices);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            window: AtomicIsize::new(0),
            message: 0,
            on: AtomicBool::new(false),
            devices: Mutex::new(HashMap::new()),
        })
    }

    fn device(owner: &Arc<Shared>, on: bool) -> Arc<Device> {
        Arc::new(Device {
            on: AtomicBool::new(on),
            pending: Mutex::new(None),
            subscription: Mutex::new(None),
            owner: Arc::downgrade(owner),
        })
    }

    #[test]
    fn enabled_adapters_are_aggregated_and_removing_the_last_one_hides_bluetooth()
    -> anyhow::Result<()> {
        let shared = shared();
        assert!(!shared.on.load(Ordering::Acquire));
        let first = device(&shared, true);
        let second = device(&shared, true);
        shared
            .devices
            .lock()
            .map_err(|_| anyhow::anyhow!("poisoned device map"))?
            .extend([("internal".into(), first.clone()), ("usb".into(), second)]);
        shared.publish();
        assert!(shared.on.load(Ordering::Acquire));
        first.on.store(false, Ordering::Release);
        shared.publish();
        assert!(shared.on.load(Ordering::Acquire));
        shared.removed("usb");
        assert!(!shared.on.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn an_old_removed_adapter_cannot_override_its_replacement() -> anyhow::Result<()> {
        let shared = shared();
        let old = device(&shared, true);
        let replacement = device(&shared, false);
        shared
            .devices
            .lock()
            .map_err(|_| anyhow::anyhow!("poisoned device map"))?
            .insert("usb".into(), replacement);
        old.on.store(true, Ordering::Release); // A callback already in flight.
        old.owner.upgrade().context("live monitor")?.publish();
        assert!(!shared.on.load(Ordering::Acquire));
        let weak = Arc::downgrade(&old);
        drop(old);
        assert!(
            weak.upgrade().is_none(),
            "callbacks must not retain removed devices"
        );
        Ok(())
    }

    #[test]
    #[ignore = "Read-only Windows radio watcher; does not toggle hardware or request access"]
    fn native_radio_watcher_reads_state_and_releases_subscriptions() -> anyhow::Result<()> {
        use std::time::{Duration, Instant};
        use windows::Devices::Enumeration::DeviceWatcherStatus;
        use windows::Win32::{
            System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
            UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WM_APP, WS_POPUP},
        };
        use windows::core::w;
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let result = (|| -> anyhow::Result<()> {
            let window = unsafe {
                CreateWindowExW(
                    Default::default(),
                    w!("STATIC"),
                    w!("Nosih radio test"),
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
            let monitor = Monitor::new(window.0 as isize, WM_APP + 300)?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let complete =
                    monitor.watcher.Status()? == DeviceWatcherStatus::EnumerationCompleted;
                let pending = monitor
                    .shared
                    .devices
                    .lock()
                    .map_err(|_| anyhow::anyhow!("poisoned device map"))?
                    .values()
                    .any(|device| {
                        device
                            .pending
                            .lock()
                            .map_or(true, |pending| pending.is_some())
                    });
                if complete && !pending {
                    break;
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "radio discovery exceeded its test deadline"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let shared = monitor.shared.clone();
            let devices = shared
                .devices
                .lock()
                .map_err(|_| anyhow::anyhow!("poisoned device map"))?;
            let mut expected = false;
            let mut tracked = 0;
            for device in devices.values() {
                let subscription = device
                    .subscription
                    .lock()
                    .map_err(|_| anyhow::anyhow!("poisoned radio slot"))?;
                if let Some(subscription) = subscription.as_ref() {
                    assert!(subscription.radio.Kind()? == RadioKind::Bluetooth);
                    expected |= subscription.radio.State()? == RadioState::On;
                    tracked += 1;
                }
            }
            drop(devices);
            assert_eq!(monitor.is_on(), expected);
            eprintln!("Bluetooth radios watched: {tracked}; enabled: {expected}");
            drop(monitor);
            assert_eq!(shared.window.load(Ordering::Acquire), 0);
            assert!(
                shared
                    .devices
                    .lock()
                    .map_err(|_| anyhow::anyhow!("poisoned device map"))?
                    .is_empty()
            );
            Ok(())
        })();
        unsafe {
            RoUninitialize();
        }
        result
    }
}
