use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail, ensure};
use nosih_shell::accessories::{Endpoint, Kind, Snapshot, State, assemble};
use windows::{
    Devices::Enumeration::{
        DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher,
    },
    Foundation::{IPropertyValue, TypedEventHandler},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{Array, HSTRING, Interface},
};
use windows_collections::IIterable;

use super::{Frame, usb, visibility};

// Optional Windows compatibility property, not a device protocol or a public
// promise of battery support. Missing/invalid values remain unknown.
const BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
const CONTAINER: &str = "System.Devices.ContainerId";
const AEP_CONTAINER: &str = "System.Devices.Aep.ContainerId";
const CONNECTED: &str = "System.Devices.Aep.IsConnected";
const CATEGORY: &str = "System.Devices.Aep.Category";
const AEP: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";
const NODES: &str = "System.Devices.ClassGuid:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR System.Devices.ClassGuid:=\"{745a17a0-74d3-11d0-b6fe-00a0c90f57da}\" OR System.Devices.ClassGuid:=\"{4d36e96c-e325-11ce-bfc1-08002be10318}\"";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Endpoint,
    Node,
    Usb,
}
pub(super) enum Event {
    Add(Source, DeviceInformation),
    Update(Source, DeviceInformationUpdate),
    Remove(Source, DeviceInformationUpdate),
    Complete(Source),
    Usb(usb::Completed),
    Visibility,
    Stopped(Source),
}
enum Subscription {
    Add(i64),
    Update(i64),
    Remove(i64),
    Complete(i64),
    Stop(i64),
}
struct Watch {
    watcher: DeviceWatcher,
    tokens: Vec<Subscription>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
        for token in self.tokens.drain(..) {
            let _ = match token {
                Subscription::Add(token) => self.watcher.RemoveAdded(token),
                Subscription::Update(token) => self.watcher.RemoveUpdated(token),
                Subscription::Remove(token) => self.watcher.RemoveRemoved(token),
                Subscription::Complete(token) => self.watcher.RemoveEnumerationCompleted(token),
                Subscription::Stop(token) => self.watcher.RemoveStopped(token),
            };
        }
    }
}

fn send(tx: &SyncSender<Event>, full: &AtomicBool, event: Event) {
    if matches!(tx.try_send(event), Err(mpsc::TrySendError::Full(_))) {
        full.store(true, Ordering::Release);
    }
}

fn watch(source: Source, tx: SyncSender<Event>, full: Arc<AtomicBool>) -> Result<Watch> {
    let props: IIterable<HSTRING> = match source {
        Source::Endpoint => vec![AEP_CONTAINER, CONNECTED, CATEGORY],
        Source::Node => vec![CONTAINER, BATTERY],
        Source::Usb => vec![CONTAINER],
    }
    .into_iter()
    .map(HSTRING::from)
    .collect::<Vec<_>>()
    .into();
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        &HSTRING::from(match source {
            Source::Endpoint => AEP.into(),
            Source::Node => NODES.into(),
            Source::Usb => usb::selector()?,
        }),
        &props,
        match source {
            Source::Endpoint => DeviceInformationKind::AssociationEndpoint,
            Source::Node => DeviceInformationKind::Device,
            Source::Usb => DeviceInformationKind::DeviceInterface,
        },
    )?;
    let mut watch = Watch {
        watcher,
        tokens: Vec::with_capacity(5),
    };
    let (sender, overflow) = (tx.clone(), full.clone());
    watch.tokens.push(Subscription::Add(watch.watcher.Added(
        &TypedEventHandler::new(move |_, arg: windows::core::Ref<DeviceInformation>| {
            if let Some(info) = arg.as_ref() {
                send(&sender, &overflow, Event::Add(source, info.clone()));
            }
            Ok(())
        }),
    )?));
    let (sender, overflow) = (tx.clone(), full.clone());
    watch
        .tokens
        .push(Subscription::Update(watch.watcher.Updated(
            &TypedEventHandler::new(move |_, arg: windows::core::Ref<DeviceInformationUpdate>| {
                if let Some(info) = arg.as_ref() {
                    send(&sender, &overflow, Event::Update(source, info.clone()));
                }
                Ok(())
            }),
        )?));
    let (sender, overflow) = (tx.clone(), full.clone());
    watch
        .tokens
        .push(Subscription::Remove(watch.watcher.Removed(
            &TypedEventHandler::new(move |_, arg: windows::core::Ref<DeviceInformationUpdate>| {
                if let Some(info) = arg.as_ref() {
                    send(&sender, &overflow, Event::Remove(source, info.clone()));
                }
                Ok(())
            }),
        )?));
    let (sender, overflow) = (tx.clone(), full.clone());
    watch
        .tokens
        .push(Subscription::Complete(watch.watcher.EnumerationCompleted(
            &TypedEventHandler::new(move |_, _| {
                send(&sender, &overflow, Event::Complete(source));
                Ok(())
            }),
        )?));
    watch.tokens.push(Subscription::Stop(watch.watcher.Stopped(
        &TypedEventHandler::new(move |_, _| {
            send(&tx, &full, Event::Stopped(source));
            Ok(())
        }),
    )?));
    watch.watcher.Start()?;
    Ok(watch)
}

fn property(info: &DeviceInformation, name: &str) -> Option<IPropertyValue> {
    info.Properties()
        .ok()?
        .Lookup(&HSTRING::from(name))
        .ok()?
        .cast()
        .ok()
}
fn container(info: &DeviceInformation, name: &str) -> Option<String> {
    let id = property(info, name)?.GetGuid().ok()?;
    (id != windows::core::GUID::zeroed()).then(|| format!("{id:?}"))
}

fn snapshot(
    endpoints: &BTreeMap<String, DeviceInformation>,
    nodes: &BTreeMap<String, DeviceInformation>,
    usb: &usb::Sources,
) -> Result<Snapshot> {
    let mut levels: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
    for node in nodes.values() {
        if let (Some(id), Some(percent)) = (
            container(node, CONTAINER),
            property(node, BATTERY)
                .and_then(|value| value.GetUInt8().ok())
                .filter(|value| *value <= 100),
        ) {
            levels.entry(id).or_default().insert(percent);
        }
    }
    let mut found = Vec::new();
    for info in endpoints.values() {
        let mut categories = Array::<HSTRING>::new();
        if let Some(value) = property(info, CATEGORY) {
            let _ = value.GetStringArray(&mut categories);
        }
        let Some(kind) = Kind::from_categories(
            &categories
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ) else {
            continue;
        };
        let id = container(info, AEP_CONTAINER)
            .unwrap_or_else(|| info.Id().map(|id| id.to_string()).unwrap_or_default());
        let name: String = info
            .Name()?
            .to_string()
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        if id.is_empty() || id.len() > 512 || name.is_empty() {
            continue;
        }
        found.push(Endpoint {
            id,
            name,
            kind,
            connected: property(info, CONNECTED).and_then(|value| value.GetBoolean().ok()),
        });
    }
    let mut devices = assemble(found, levels);
    for reading in usb.devices() {
        if let Some(existing) = devices.iter_mut().find(|device| device.id == reading.id) {
            if existing.percent.is_none() && !existing.conflicting {
                existing.percent = reading.percent;
            } else if reading.percent.is_some() && existing.percent != reading.percent {
                existing.percent = None;
                existing.conflicting = true;
            }
        } else {
            devices.push(reading.clone());
        }
    }
    devices.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    devices.truncate(nosih_shell::accessories::MAX_DEVICES);
    if !devices.is_empty()
        && let Some(active) = nosih_shell::status::default_audio_container()
    {
        for device in &mut devices {
            device.active_audio = device.kind == Kind::Headset && device.id == active;
        }
    }
    Ok(Snapshot {
        state: State::Ready,
        devices,
        received_at: Some(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()),
    })
}

pub(super) fn emit(frame: &Frame) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, frame)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Balanced initialization on this worker's MTA thread.
        unsafe {
            RoUninitialize();
        }
    }
}

pub fn run(visibility: Option<&str>) -> Result<()> {
    emit(&Frame::Busy)?;
    // SAFETY: This worker owns its MTA and exits independently of the UI.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
    let _apartment = Apartment;
    let (tx, rx) = mpsc::sync_channel(256);
    let full = Arc::new(AtomicBool::new(false));
    let visibility = visibility
        .map(|name| visibility::Watch::new(name, tx.clone(), full.clone()))
        .transpose()?;
    let _aep = watch(Source::Endpoint, tx.clone(), full.clone())?;
    let _nodes = watch(Source::Node, tx.clone(), full.clone())?;
    let _usb = watch(Source::Usb, tx.clone(), full.clone())
        .inspect_err(|_| eprintln!("nosih: USB accessory discovery unavailable"))
        .ok();
    let mut usb = usb::Sources::default();
    let (mut endpoints, mut nodes) = (BTreeMap::new(), BTreeMap::new());
    let mut complete = [false; 3];
    let mut deadline = Some(Instant::now() + Duration::from_millis(120));
    let mut busy = true;
    let mut last: Option<Snapshot> = None;
    loop {
        ensure!(
            !full.load(Ordering::Acquire),
            "accessory event queue overflow"
        );
        let active = visibility.as_ref().is_none_or(visibility::Watch::active);
        if active {
            usb.schedule(&tx, &full)?;
        }
        let wake = deadline
            .into_iter()
            .chain(active.then(|| usb.deadline()).flatten())
            .min();
        let event = if let Some(at) = wake {
            match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(event) => Some(event),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(error) => return Err(error.into()),
            }
        } else {
            Some(rx.recv()?)
        };
        let Some(event) = event else {
            if deadline.is_none_or(|at| at > Instant::now()) {
                continue;
            }
            deadline = None;
            // Optional USB discovery must not delay the established Bluetooth source.
            if complete[..2].iter().all(|done| *done) {
                let mut current = snapshot(&endpoints, &nodes, &usb)?;
                if let Some(previous) = &last
                    && current.devices == previous.devices
                {
                    current.received_at = previous.received_at;
                }
                emit(&Frame::Ready(current.clone()))?;
                last = Some(current);
                busy = false;
            }
            continue;
        };
        if matches!(event, Event::Visibility) {
            continue;
        }
        if !busy {
            emit(&Frame::Busy)?;
            busy = true;
        }
        deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(120));
        match event {
            Event::Visibility => {}
            Event::Add(Source::Usb, info) => {
                let path = info.Id()?.to_string();
                let id = container(&info, CONTAINER).unwrap_or_else(|| path.clone());
                if id.len() <= 500 {
                    usb.add(path, id);
                }
            }
            Event::Update(Source::Usb, _) => {}
            Event::Remove(Source::Usb, info) => usb.remove(&info.Id()?.to_string()),
            Event::Usb(result) => usb.complete(result),
            Event::Add(aep, info) => {
                let target = if aep == Source::Endpoint {
                    &mut endpoints
                } else {
                    &mut nodes
                };
                target.insert(info.Id()?.to_string(), info);
                ensure!(target.len() <= 512, "accessory cache exceeded its limit");
            }
            Event::Update(aep, update) => {
                if let Some(info) = (if aep == Source::Endpoint {
                    &endpoints
                } else {
                    &nodes
                })
                .get(&update.Id()?.to_string())
                {
                    info.Update(&update)?;
                }
            }
            Event::Remove(aep, update) => {
                (if aep == Source::Endpoint {
                    &mut endpoints
                } else {
                    &mut nodes
                })
                .remove(&update.Id()?.to_string());
            }
            Event::Complete(source) => complete[source as usize] = true,
            Event::Stopped(Source::Usb) => usb = usb::Sources::default(),
            Event::Stopped(_) => bail!("accessory watcher stopped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Enumerates allowlisted Windows USB metadata; never sends HID queries"]
    fn accessory_usb_discovery_enumerates_allowlist() -> Result<()> {
        // Own MTA, watcher and callbacks. No Sources pool or data handle exists.
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let _apartment = Apartment;
        let (tx, rx) = mpsc::sync_channel(256);
        let full = Arc::new(AtomicBool::new(false));
        let _watch = watch(Source::Usb, tx, full.clone())?;
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(end.saturating_duration_since(Instant::now()))? {
                Event::Complete(Source::Usb) => break,
                Event::Stopped(_) => bail!("USB discovery stopped before completing"),
                _ => {}
            }
        }
        ensure!(
            !full.load(Ordering::Acquire),
            "USB discovery queue overflow"
        );
        Ok(())
    }
}
