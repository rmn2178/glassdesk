use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use serde::Serialize;
use windows::{
    Devices::Enumeration::{
        DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher,
    },
    Foundation::{IPropertyValue, TypedEventHandler},
    core::{HSTRING, Interface},
};
use windows_collections::IIterable;

const PROPERTIES: [&str; 4] = [
    "System.Devices.Aep.IsConnected",
    "System.Devices.Aep.IsPaired",
    "System.Devices.Aep.ContainerId",
    "System.Devices.Aep.Category",
];
const SELECTOR: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";

enum Event {
    Added(DeviceInformation),
    Updated(DeviceInformationUpdate),
    Removed(DeviceInformationUpdate),
    Complete,
    Stopped,
}

#[derive(Serialize)]
pub struct Endpoint {
    pub name: String,
    pub connected: Option<bool>,
    pub paired: Option<bool>,
    #[serde(skip)]
    pub container: Option<String>,
    pub categories: Vec<String>,
}

#[derive(Default, Serialize)]
pub struct Observation {
    pub initial_enumeration_ms: Option<u128>,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub stopped_unexpectedly: bool,
    pub idle_cpu_ms: Option<f64>,
    pub idle_wall_ms: Option<u128>,
    pub changes_after_initial_enumeration: u64,
    pub changed_properties: BTreeSet<String>,
    pub endpoints: Vec<Endpoint>,
}

fn send(tx: &SyncSender<Event>, overflow: &AtomicBool, event: Event) {
    if matches!(tx.try_send(event), Err(mpsc::TrySendError::Full(_))) {
        overflow.store(true, Ordering::Relaxed);
    }
}

struct Watch {
    watcher: DeviceWatcher,
    tokens: Vec<(u8, i64)>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
        for (kind, token) in self.tokens.drain(..).rev() {
            let _ = match kind {
                0 => self.watcher.RemoveAdded(token),
                1 => self.watcher.RemoveUpdated(token),
                2 => self.watcher.RemoveRemoved(token),
                3 => self.watcher.RemoveEnumerationCompleted(token),
                _ => self.watcher.RemoveStopped(token),
            };
        }
    }
}

pub fn observe(seconds: u64) -> Result<Observation> {
    let properties: IIterable<HSTRING> = PROPERTIES.map(HSTRING::from).to_vec().into();
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        &HSTRING::from(SELECTOR),
        &properties,
        DeviceInformationKind::AssociationEndpoint,
    )?;
    let mut watch = Watch {
        watcher,
        tokens: Vec::with_capacity(5),
    };
    let (tx, rx) = mpsc::sync_channel(128);
    let overflow = Arc::new(AtomicBool::new(false));
    let (sender, full) = (tx.clone(), overflow.clone());
    watch.tokens.push((
        0,
        watch.watcher.Added(&TypedEventHandler::new(
            move |_, value: windows::core::Ref<DeviceInformation>| {
                if let Some(value) = value.as_ref() {
                    send(&sender, &full, Event::Added(value.clone()));
                }
                Ok(())
            },
        ))?,
    ));
    let (sender, full) = (tx.clone(), overflow.clone());
    watch.tokens.push((
        1,
        watch.watcher.Updated(&TypedEventHandler::new(
            move |_, value: windows::core::Ref<DeviceInformationUpdate>| {
                if let Some(value) = value.as_ref() {
                    send(&sender, &full, Event::Updated(value.clone()));
                }
                Ok(())
            },
        ))?,
    ));
    let (sender, full) = (tx.clone(), overflow.clone());
    watch.tokens.push((
        2,
        watch.watcher.Removed(&TypedEventHandler::new(
            move |_, value: windows::core::Ref<DeviceInformationUpdate>| {
                if let Some(value) = value.as_ref() {
                    send(&sender, &full, Event::Removed(value.clone()));
                }
                Ok(())
            },
        ))?,
    ));
    let (sender, full) = (tx.clone(), overflow.clone());
    watch.tokens.push((
        3,
        watch
            .watcher
            .EnumerationCompleted(&TypedEventHandler::new(move |_, _| {
                send(&sender, &full, Event::Complete);
                Ok(())
            }))?,
    ));
    let (tx, full) = (tx, overflow.clone());
    watch.tokens.push((
        4,
        watch.watcher.Stopped(&TypedEventHandler::new(move |_, _| {
            send(&tx, &full, Event::Stopped);
            Ok(())
        }))?,
    ));
    let started = Instant::now();
    watch.watcher.Start()?;
    let deadline = started + Duration::from_secs(seconds);
    let mut devices: BTreeMap<String, DeviceInformation> = BTreeMap::new();
    let mut out = Observation::default();
    let mut idle = None;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let event = match rx.recv_timeout(remaining) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(error) => return Err(error.into()),
        };
        if idle.is_some()
            && matches!(
                &event,
                Event::Added(_) | Event::Updated(_) | Event::Removed(_)
            )
        {
            out.changes_after_initial_enumeration += 1;
        }
        match event {
            Event::Added(device) => {
                out.added += 1;
                devices.insert(device.Id()?.to_string(), device);
                ensure!(devices.len() <= 128, "too many paired Bluetooth endpoints");
            }
            Event::Updated(update) => {
                out.updated += 1;
                let properties = update.Properties()?.First()?;
                while properties.HasCurrent()? {
                    out.changed_properties
                        .insert(properties.Current()?.Key()?.to_string());
                    properties.MoveNext()?;
                }
                if let Some(device) = devices.get(&update.Id()?.to_string()) {
                    device.Update(&update)?;
                }
            }
            Event::Removed(update) => {
                out.removed += 1;
                devices.remove(&update.Id()?.to_string());
            }
            Event::Complete => {
                out.initial_enumeration_ms = Some(started.elapsed().as_millis());
                idle = Some((Instant::now(), super::metrics::capture()?.cpu_ms));
            }
            Event::Stopped => {
                out.stopped_unexpectedly = true;
                break;
            }
        }
    }
    if let Some((start, cpu)) = idle {
        out.idle_cpu_ms = Some(super::metrics::capture()?.cpu_ms - cpu);
        out.idle_wall_ms = Some(start.elapsed().as_millis());
    }
    // Callback closures keep only bounded messages, never property reads or locks.
    // Closing the watch revokes every token, including after a partial setup failure.
    drop(watch);
    ensure!(
        !overflow.load(Ordering::Relaxed),
        "watcher queue overflow; snapshot is incomplete"
    );
    for device in devices.into_values() {
        let props = device.Properties()?;
        let read = |key: &str| {
            props
                .Lookup(&HSTRING::from(key))
                .ok()?
                .cast::<IPropertyValue>()
                .ok()
        };
        let mut categories = windows::core::Array::<HSTRING>::new();
        if let Some(value) = read(PROPERTIES[3]) {
            let _ = value.GetStringArray(&mut categories);
        }
        out.endpoints.push(Endpoint {
            name: device.Name()?.to_string(),
            connected: read(PROPERTIES[0]).and_then(|value| value.GetBoolean().ok()),
            paired: read(PROPERTIES[1]).and_then(|value| value.GetBoolean().ok()),
            container: read(PROPERTIES[2])
                .and_then(|value| value.GetGuid().ok())
                .map(|id| format!("{id:?}")),
            categories: categories.iter().map(ToString::to_string).collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bursts_are_bounded_and_mark_the_snapshot_incomplete() {
        let (tx, rx) = mpsc::sync_channel(1);
        let overflow = AtomicBool::new(false);
        send(&tx, &overflow, Event::Complete);
        for _ in 0..1024 {
            send(&tx, &overflow, Event::Stopped);
        }
        assert!(overflow.load(Ordering::Relaxed));
        assert!(matches!(rx.try_recv(), Ok(Event::Complete)));
        assert!(rx.try_recv().is_err());
    }
}
