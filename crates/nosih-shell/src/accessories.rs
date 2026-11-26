//! Cached accessory data shared by the native reader and the menu bar.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const MAX_DEVICES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mouse,
    Headset,
}

impl Kind {
    pub fn from_categories(categories: &[String]) -> Option<Self> {
        categories.iter().find_map(|category| {
            if category == "Input.Mouse" {
                Some(Self::Mouse)
            } else if category == "Audio.Headphones"
                || category.starts_with("Communication.Headset")
            {
                Some(Self::Headset)
            } else {
                None
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub percent: Option<u8>,
    pub conflicting: bool,
    pub active_audio: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Loading,
    Ready,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub state: State,
    pub devices: Vec<Device>,
    /// Time Windows' cached data was received, not a measurement by the peripheral.
    pub received_at: Option<u64>,
}

impl Snapshot {
    pub fn valid(&self) -> bool {
        let mut ids = BTreeSet::new();
        self.devices.len() <= MAX_DEVICES
            && self.devices.iter().all(|device| {
                !device.id.is_empty()
                    && device.id.len() <= 512
                    && ids.insert(&device.id)
                    && !device.name.is_empty()
                    && device.name.len() <= 512
                    && device.percent.is_none_or(|percent| percent <= 100)
                    && !(device.conflicting && device.percent.is_some())
            })
    }

    /// Stable representatives, one of each category. A source failure never
    /// presents a previous percentage as a current reading.
    pub fn compact(&self) -> [Option<&Device>; 2] {
        if self.state != State::Ready {
            return [None, None];
        }
        [Kind::Mouse, Kind::Headset].map(|kind| {
            self.devices
                .iter()
                .find(|device| device.kind == kind && device.active_audio)
                .or_else(|| self.devices.iter().find(|device| device.kind == kind))
        })
    }
}

pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub connected: Option<bool>,
}

pub fn assemble(endpoints: Vec<Endpoint>, levels: BTreeMap<String, BTreeSet<u8>>) -> Vec<Device> {
    let mut devices = BTreeMap::new();
    for endpoint in endpoints {
        if endpoint.connected != Some(true) {
            continue;
        }
        let values: BTreeSet<u8> = levels
            .get(&endpoint.id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|value| *value <= 100)
            .collect();
        let percent = if values.len() == 1 {
            values.first().copied()
        } else {
            None
        };
        let device = Device {
            id: endpoint.id.clone(),
            name: endpoint.name,
            kind: endpoint.kind,
            percent,
            conflicting: values.len() > 1,
            active_audio: false,
        };
        // A2DP, hands-free and BLE endpoints may describe the same accessory.
        devices
            .entry(endpoint.id)
            .and_modify(|old: &mut Device| {
                if device.name < old.name {
                    old.name.clone_from(&device.name);
                }
            })
            .or_insert(device);
    }
    let mut devices: Vec<_> = devices.into_values().collect();
    devices.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    devices.truncate(MAX_DEVICES);
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, connected: Option<bool>) -> Endpoint {
        Endpoint {
            id: id.into(),
            name: "Same name".into(),
            kind: Kind::Mouse,
            connected,
        }
    }
    #[test]
    fn only_confirmed_connections_are_shown() {
        assert!(
            assemble(
                vec![endpoint("off", Some(false)), endpoint("unknown", None)],
                BTreeMap::new()
            )
            .is_empty()
        );
    }
    #[test]
    fn zero_missing_and_conflicting_levels_remain_distinct() {
        let rows = assemble(
            vec![
                endpoint("a", Some(true)),
                endpoint("b", Some(true)),
                endpoint("c", Some(true)),
            ],
            BTreeMap::from([
                ("a".into(), BTreeSet::from([0])),
                ("c".into(), BTreeSet::from([20, 80])),
            ]),
        );
        assert_eq!(rows[0].percent, Some(0));
        assert_eq!(rows[1].percent, None);
        assert!(!rows[1].conflicting);
        assert_eq!(rows[2].percent, None);
        assert!(rows[2].conflicting);
    }
    #[test]
    fn identities_not_names_deduplicate_endpoints() {
        let rows = assemble(
            vec![
                endpoint("a", Some(true)),
                endpoint("a", Some(true)),
                endpoint("b", Some(true)),
            ],
            BTreeMap::new(),
        );
        assert_eq!(rows.len(), 2);
    }
    #[test]
    fn source_failure_does_not_reuse_compact_percentages() {
        let mut snapshot = Snapshot {
            state: State::Ready,
            devices: assemble(vec![endpoint("a", Some(true))], BTreeMap::new()),
            received_at: Some(1),
        };
        assert_eq!(snapshot.compact().into_iter().flatten().count(), 1);
        snapshot.state = State::Unavailable;
        assert!(
            snapshot
                .compact()
                .into_iter()
                .all(|device| device.is_none())
        );
    }
    #[test]
    fn cached_computers_and_phones_are_not_accessories() {
        assert_eq!(Kind::from_categories(&["Communication.Phone".into()]), None);
        assert_eq!(Kind::from_categories(&["Computer.Laptop".into()]), None);
        assert_eq!(
            Kind::from_categories(&["Input.Mouse".into()]),
            Some(Kind::Mouse)
        );
        assert_eq!(
            Kind::from_categories(&["Communication.Headset.Wireless".into()]),
            Some(Kind::Headset)
        );
    }

    #[test]
    fn automatic_selection_prefers_active_audio_and_falls_back_on_disconnect() {
        let mut data = Snapshot {
            state: State::Ready,
            ..Default::default()
        };
        data.devices = ["a", "b"]
            .into_iter()
            .map(|id| Device {
                id: id.into(),
                name: "Same headset name".into(),
                kind: Kind::Headset,
                percent: Some(50),
                conflicting: false,
                active_audio: id == "b",
            })
            .collect();
        assert_eq!(
            data.compact()[1].map(|device| device.id.as_str()),
            Some("b")
        );
        data.devices.retain(|device| device.id != "b");
        assert_eq!(
            data.compact()[1].map(|device| device.id.as_str()),
            Some("a")
        );
        data.devices.clear();
        assert!(data.compact().into_iter().all(|device| device.is_none()));
    }

    #[test]
    fn protocol_rejects_duplicate_ids_and_invalid_levels() {
        let mut data = Snapshot {
            state: State::Ready,
            devices: assemble(vec![endpoint("a", Some(true))], BTreeMap::new()),
            received_at: None,
        };
        assert!(data.valid());
        data.devices[0].percent = Some(101);
        assert!(!data.valid());
        data.devices[0].percent = Some(50);
        data.devices.push(data.devices[0].clone());
        assert!(!data.valid());
    }
}
