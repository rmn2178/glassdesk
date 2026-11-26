//! Read-only protocol facts, independently implemented. References and device
//! limits live in ACCESSORIES.md. No settings, pairing or firmware commands.

use anyhow::{Result, ensure};
use nosih_shell::accessories::Kind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub vendor: u16,
    pub product: u16,
    pub page: u16,
    pub usage: u16,
    pub model: Model,
    /// Paired device slots to query; zero denotes a direct headset protocol.
    pub slots: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Logitech,
    LogitechHeadset,
    ProX2,
    NovaDiscrete,
    NovaPercent,
    Nova5,
    CloudAlpha,
    Cloud2Kingston,
}

impl Model {
    fn interface(self) -> Option<u8> {
        match self {
            Self::NovaDiscrete | Self::NovaPercent | Self::Nova5 | Self::ProX2 => Some(3),
            _ => None,
        }
    }
}

impl Profile {
    /// Reject unrelated collections before opening them for data access.
    pub fn accepts_collection(self, page: u16, usage: u16, input: usize, output: usize) -> bool {
        match self.model {
            Model::Logitech => {
                page == self.page && matches!((usage, input, output), (1, 7, 7) | (2, 20, 20))
            }
            Model::LogitechHeadset => {
                page == self.page && usage == self.usage && input == 20 && output == 20
            }
            Model::ProX2 => page == self.page && usage == self.usage && input == 64 && output == 64,
            Model::CloudAlpha => {
                page >= 0xff00 && (4..=64).contains(&input) && matches!(output, 31 | 32)
            }
            Model::Cloud2Kingston => page >= 0xff00 && (8..=64).contains(&input) && output == 62,
            Model::NovaDiscrete | Model::NovaPercent | Model::Nova5 => {
                page == self.page
                    && usage == self.usage
                    && (8..=129).contains(&input)
                    && (2..=65).contains(&output)
            }
        }
    }

    pub fn output_report(self, output: usize) -> u8 {
        match self.model {
            Model::Logitech if output == 7 => 0x10,
            Model::Logitech | Model::LogitechHeadset => 0x11,
            Model::ProX2 => 0x51,
            Model::CloudAlpha => 0x21,
            Model::Cloud2Kingston => 6,
            _ => 0,
        }
    }
}

const fn headset(vendor: u16, product: u16, page: u16, usage: u16, model: Model) -> Profile {
    Profile {
        vendor,
        product,
        page,
        usage,
        model,
        slots: 0,
    }
}

const fn logitech_receiver(product: u16, slots: u8) -> Profile {
    Profile {
        vendor: 0x046d,
        product,
        page: 0xff00,
        usage: 2,
        model: Model::Logitech,
        slots,
    }
}

// Receiver identities from Solaar's base_usb.py. Battery support is discovered
// on the paired device, never inferred from a marketing name or a PID range.
pub const PROFILES: &[Profile] = &[
    logitech_receiver(0xc52b, 6), // Unifying
    logitech_receiver(0xc532, 6),
    logitech_receiver(0xc548, 6), // Bolt
    logitech_receiver(0xc52e, 1), // Nano; HID++ 2.0 devices only
    logitech_receiver(0xc52f, 1),
    logitech_receiver(0xc531, 1),
    logitech_receiver(0xc534, 2),
    logitech_receiver(0xc537, 1),
    logitech_receiver(0xc539, 1), // Lightspeed
    logitech_receiver(0xc53a, 1),
    logitech_receiver(0xc53d, 1),
    logitech_receiver(0xc53f, 1), // G305 / G304
    logitech_receiver(0xc541, 1),
    logitech_receiver(0xc545, 1),
    logitech_receiver(0xc547, 1),
    logitech_receiver(0xc54d, 1),
    headset(0x046d, 0x0aa7, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0aaa, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0aba, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0afb, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0afc, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0a66, 0xff43, 0x0202, Model::LogitechHeadset),
    headset(0x046d, 0x0af7, 0xffa0, 1, Model::ProX2),
    headset(0x1038, 0x2232, 0xffc0, 1, Model::Nova5),
    headset(0x1038, 0x2253, 0xffc0, 1, Model::Nova5),
    headset(0x1038, 0x2206, 0xffc0, 1, Model::NovaDiscrete),
    headset(0x1038, 0x220a, 0xffc0, 1, Model::NovaDiscrete),
    headset(0x1038, 0x223a, 0xffc0, 1, Model::NovaDiscrete),
    headset(0x1038, 0x227a, 0xffc0, 1, Model::NovaDiscrete),
    headset(0x1038, 0x22a4, 0xffc0, 1, Model::NovaDiscrete),
    headset(0x1038, 0x2258, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x229e, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x22ad, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x22a9, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x22a5, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x22a7, 0xffc0, 1, Model::NovaPercent),
    headset(0x1038, 0x2298, 0xffc0, 1, Model::NovaPercent),
    // These revisions do not publish a stable usage in their protocol notes.
    // Discovery stays VID/PID + vendor-page filtered. Native validation also
    // requires the exact output length and report ID before any request.
    headset(0x03f0, 0x098d, 0, 0, Model::CloudAlpha),
    headset(0x0951, 0x1718, 0, 0, Model::Cloud2Kingston),
    Profile {
        vendor: 0x1038,
        product: 0x2202,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaDiscrete,
        slots: 0,
    },
    Profile {
        vendor: 0x1038,
        product: 0x22a1,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaPercent,
        slots: 0,
    },
    Profile {
        vendor: 0x1038,
        product: 0x227e,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaPercent,
        slots: 0,
    },
];

pub fn profile_path(path: &str) -> Option<Profile> {
    if path.len() > 1024 || path.contains('\0') {
        return None;
    }
    let path = path.to_ascii_lowercase();
    if !path.starts_with(r"\\?\hid#vid_") {
        return None;
    }
    PROFILES.iter().copied().find(|profile| {
        let prefix = format!(
            r"\\?\hid#vid_{:04x}&pid_{:04x}&",
            profile.vendor, profile.product
        );
        path.starts_with(&prefix)
            && profile.model.interface().is_none_or(|interface| {
                let expected = format!("mi_{interface:02x}");
                path.split('#')
                    .nth(1)
                    .is_some_and(|hardware| hardware.split('&').any(|part| part == expected))
            })
    })
}

/// Windows exposes short and long HID++ reports as separate collections.
/// Strip only their collection ordinal, preserving the physical USB instance
/// and interface so identical receivers never share a query or battery state.
pub fn receiver_key(path: &str) -> Option<String> {
    let profile = profile_path(path)?;
    let normalized = path.to_ascii_lowercase();
    if profile.model != Model::Logitech {
        return Some(normalized);
    }
    let parts: Vec<_> = normalized.split('#').collect();
    if parts.len() != 4 {
        return Some(normalized);
    }
    let mut hardware = Vec::new();
    let mut collection = false;
    for part in parts[1].split('&') {
        if let Some(ordinal) = part.strip_prefix("col") {
            if collection || ordinal.len() != 2 || !ordinal.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            collection = true;
        } else {
            hardware.push(part);
        }
    }
    let Some((instance, ordinal)) = parts[2].rsplit_once('&') else {
        return Some(normalized);
    };
    if !collection || ordinal.len() != 4 || !ordinal.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(normalized);
    }
    Some(format!(
        "{}#{}#{}#{}",
        parts[0],
        hardware.join("&"),
        instance,
        parts[3]
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    pub slot: u8,
    pub kind: Kind,
    pub percent: Option<u8>,
}

pub trait Wire {
    fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>>;
}

const SOFTWARE: u8 = 0x0c;
pub fn hidpp_request(slot: u8, feature: u8, function: u8, parameters: &[u8]) -> [u8; 20] {
    let mut packet = [0; 20];
    packet[..4].copy_from_slice(&[0x11, slot, feature, function << 4 | SOFTWARE]);
    let count = parameters.len().min(16);
    packet[4..4 + count].copy_from_slice(&parameters[..count]);
    packet
}
fn hidpp_call(
    wire: &mut impl Wire,
    slot: u8,
    feature: u8,
    function: u8,
    parameters: &[u8],
) -> Result<Vec<u8>> {
    let request = hidpp_request(slot, feature, function, parameters);
    let reply = wire.exchange(&request, |reply| hidpp_matches(&request, reply))?;
    ensure!(hidpp_matches(&request, &reply), "invalid HID++ response");
    ensure!(reply[2] == feature, "HID++ query rejected");
    Ok(reply[4..].to_vec())
}
pub fn hidpp_matches(request: &[u8; 20], reply: &[u8]) -> bool {
    matches!(
        (reply.len(), reply.first()),
        (7, Some(0x10)) | (20, Some(0x11))
    ) && reply[1] == request[1]
        && ((reply[2] == request[2] && reply[3] == request[3])
            || (matches!(reply[2], 0x8f | 0xff)
                && reply[3] == request[2]
                && reply[4] == request[3]))
}
fn feature(wire: &mut impl Wire, slot: u8, id: u16) -> Result<u8> {
    let answer = hidpp_call(wire, slot, 0, 0, &id.to_be_bytes())?;
    Ok(if answer[1] & 0x80 == 0 { answer[0] } else { 0 })
}

pub fn logitech(wire: &mut impl Wire, slot: u8, nonce: u8) -> Result<Option<Reading>> {
    let ping = hidpp_call(wire, slot, 0, 1, &[0, 0, nonce])?;
    ensure!(ping[0] >= 2 && ping[2] == nonce, "invalid HID++ ping");
    let name_feature = feature(wire, slot, 0x0005)?;
    if name_feature == 0 {
        return Ok(None);
    }
    let device_type = hidpp_call(wire, slot, name_feature, 2, &[])?;
    let kind = match device_type[0] {
        3..=5 => Kind::Mouse,
        8 => Kind::Headset,
        _ => return Ok(None),
    };
    Ok(Some(Reading {
        slot,
        kind,
        percent: logitech_percent(wire, slot)?,
    }))
}

fn logitech_percent(wire: &mut impl Wire, slot: u8) -> Result<Option<u8>> {
    let battery = feature(wire, slot, 0x1004)?;
    let mut percent = if battery != 0 {
        let capabilities = hidpp_call(wire, slot, battery, 0, &[])?;
        let status = hidpp_call(wire, slot, battery, 1, &[])?;
        unified_percent(&capabilities, &status)
    } else {
        None
    };
    if percent.is_none() {
        let battery = feature(wire, slot, 0x1000)?;
        if battery != 0 {
            percent = battery_status_percent(&hidpp_call(wire, slot, battery, 0, &[])?);
        }
    }
    Ok(percent)
}

pub fn logitech_headset(wire: &mut impl Wire, nonce: u8) -> Result<Option<Reading>> {
    let ping = hidpp_call(wire, 0xff, 0, 1, &[0, 0, nonce])?;
    ensure!(ping[0] >= 2 && ping[2] == nonce, "invalid headset ping");
    let percent = logitech_percent(wire, 0xff)?;
    if percent.is_none() {
        let adc = feature(wire, 0xff, 0x1f20)?;
        if adc == 0 {
            return Ok(None);
        }
        let status = hidpp_call(wire, 0xff, adc, 0, &[])?;
        let voltage = u16::from_be_bytes([status[0], status[1]]);
        if !(2500..=5000).contains(&voltage) || !matches!(status[2], 1 | 3 | 7 | 15) {
            return Ok(None);
        }
        // The radio answered with a battery measurement, but voltage is not SoC.
    }
    Ok(Some(Reading {
        slot: 0,
        kind: Kind::Headset,
        percent,
    }))
}

pub fn battery_status_percent(status: &[u8]) -> Option<u8> {
    // 0x1000 returns the device's own percentage (potentially coarse steps).
    // Zero means unavailable in this older feature, unlike 0x1004 with SoC.
    let &[level, _, charging, ..] = status else {
        return None;
    };
    (charging <= 4 && (1..=100).contains(&level)).then_some(level)
}

pub fn unified_percent(capabilities: &[u8], status: &[u8]) -> Option<u8> {
    // Bit 1 advertises an actual state of charge. Level flags are not percent.
    (capabilities.get(1)? & 2 != 0 && status.len() >= 3 && status[2] <= 3)
        .then(|| status[0])
        .filter(|level| *level <= 100)
}

pub fn nova(model: Model, raw: &[u8]) -> Option<Reading> {
    let body = if raw.first() == Some(&0) {
        &raw[1..]
    } else {
        raw
    };
    if body.len() < 7 || body[0] != 0xb0 {
        return None;
    }
    let percent = match model {
        Model::Nova5 if body.len() >= 16 && matches!(body[1], 0 | 1) && body[3] <= 100 => {
            Some(body[3])
        }
        Model::NovaPercent if matches!(body[3], 1..=3) && body[2] <= 100 => Some(body[2]),
        Model::NovaDiscrete if matches!(body[3], 1..=3) && body[2] <= 4 => None,
        _ => return None,
    };
    Some(Reading {
        slot: 0,
        kind: Kind::Headset,
        percent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Script(std::collections::VecDeque<([u8; 20], Vec<u8>)>);
    impl Wire for Script {
        fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
            let (expected, payload) = self
                .0
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("unexpected device command"))?;
            assert_eq!(request, expected);
            let length = if payload.len() <= 3 { 7 } else { 20 };
            let mut reply = vec![0; length];
            reply[..4].copy_from_slice(&expected[..4]);
            reply[0] = if length == 7 { 0x10 } else { 0x11 };
            reply[4..4 + payload.len()].copy_from_slice(&payload);
            assert!(matches(&reply));
            Ok(reply)
        }
    }

    #[test]
    fn headset_profiles_reject_unsafe_or_unrelated_collections() -> Result<()> {
        for (path, page, usage, input, output, report) in [
            (
                r"\\?\hid#vid_046d&pid_0af7&mi_03#instance",
                0xffa0,
                1,
                64,
                64,
                0x51,
            ),
            (
                r"\\?\hid#vid_046d&pid_0aaa&mi_03#instance",
                0xff43,
                0x0202,
                20,
                20,
                0x11,
            ),
            (
                r"\\?\hid#vid_1038&pid_2232&mi_03#instance",
                0xffc0,
                1,
                64,
                65,
                0,
            ),
            (
                r"\\?\hid#vid_03f0&pid_098d&mi_03#instance",
                0xff13,
                1,
                31,
                31,
                0x21,
            ),
            (
                r"\\?\hid#vid_0951&pid_1718&mi_03#instance",
                0xff13,
                1,
                64,
                62,
                6,
            ),
        ] {
            let profile =
                profile_path(path).ok_or_else(|| anyhow::anyhow!("missing headset profile"))?;
            assert!(profile.accepts_collection(page, usage, input, output));
            assert!(!profile.accepts_collection(1, usage, input, output));
            assert!(!profile.accepts_collection(page, usage, 0, output));
            assert!(!profile.accepts_collection(page, usage, input, 4096));
            assert_eq!(profile.output_report(output), report);
        }
        // HP/DTS Cloud II connection reads have known side effects. Never probe.
        assert!(profile_path(r"\\?\hid#vid_03f0&pid_0696&mi_00#instance").is_none());
        assert!(profile_path(r"\\?\hid#vid_046d&pid_0af7&mi_00#instance").is_none());
        let mut identities = std::collections::BTreeSet::new();
        assert!(
            PROFILES
                .iter()
                .all(|p| identities.insert((p.vendor, p.product)))
        );
        Ok(())
    }

    #[test]
    fn direct_logitech_headset_preserves_soc_and_voltage_only_uncertainty() -> Result<()> {
        let mut soc = Script(std::collections::VecDeque::from([
            (hidpp_request(0xff, 0, 1, &[0, 0, 19]), vec![4, 0, 19]),
            (hidpp_request(0xff, 0, 0, &[0x10, 4]), vec![5, 0, 0]),
            (hidpp_request(0xff, 5, 0, &[]), vec![15, 2]),
            (hidpp_request(0xff, 5, 1, &[]), vec![0, 1, 0, 0]),
        ]));
        assert_eq!(
            logitech_headset(&mut soc, 19)?,
            Some(Reading {
                slot: 0,
                kind: Kind::Headset,
                percent: Some(0)
            })
        );
        assert!(soc.0.is_empty());
        let mut adc = Script(std::collections::VecDeque::from([
            (hidpp_request(0xff, 0, 1, &[0, 0, 19]), vec![4, 0, 19]),
            (hidpp_request(0xff, 0, 0, &[0x10, 4]), vec![0, 0, 0]),
            (hidpp_request(0xff, 0, 0, &[0x10, 0]), vec![0, 0, 0]),
            (hidpp_request(0xff, 0, 0, &[0x1f, 0x20]), vec![6, 0, 0]),
            (hidpp_request(0xff, 6, 0, &[]), vec![0x0f, 0xa0, 1]),
        ]));
        assert_eq!(
            logitech_headset(&mut adc, 19)?,
            Some(Reading {
                slot: 0,
                kind: Kind::Headset,
                percent: None
            })
        );
        assert!(adc.0.is_empty());
        Ok(())
    }

    #[test]
    fn g305_uses_its_reported_battery_status_without_unified_battery() -> Result<()> {
        let profile = profile_path(r"\\?\hid#vid_046d&pid_c53f&mi_02#instance")
            .ok_or_else(|| anyhow::anyhow!("missing G305 receiver"))?;
        assert_eq!(profile.slots, 1);
        let mut wire = Script(std::collections::VecDeque::from([
            (hidpp_request(1, 0, 1, &[0, 0, 23]), vec![4, 2, 23]),
            (hidpp_request(1, 0, 0, &[0, 5]), vec![3, 0, 0]),
            (hidpp_request(1, 3, 2, &[]), vec![3]),
            (hidpp_request(1, 0, 0, &[0x10, 4]), vec![0, 0, 0]),
            (hidpp_request(1, 0, 0, &[0x10, 0]), vec![5, 0, 0]),
            (hidpp_request(1, 5, 0, &[]), vec![90, 50, 0]),
        ]));
        assert_eq!(
            logitech(&mut wire, 1, 23)?,
            Some(Reading {
                slot: 1,
                kind: Kind::Mouse,
                percent: Some(90),
            })
        );
        assert!(wire.0.is_empty());
        Ok(())
    }

    #[test]
    fn discrete_unified_levels_are_not_promoted_to_percentages() -> Result<()> {
        let mut wire = Script(std::collections::VecDeque::from([
            (hidpp_request(1, 0, 1, &[0, 0, 19]), vec![3, 0, 19]),
            (hidpp_request(1, 0, 0, &[0, 5]), vec![3, 0, 0]),
            (hidpp_request(1, 3, 2, &[]), vec![3]),
            (hidpp_request(1, 0, 0, &[0x10, 4]), vec![5, 0, 0]),
            (hidpp_request(1, 5, 0, &[]), vec![15, 0]),
            (hidpp_request(1, 5, 1, &[]), vec![0, 8, 0, 0]),
            (hidpp_request(1, 0, 0, &[0x10, 0]), vec![0, 0, 0]),
        ]));
        assert_eq!(logitech(&mut wire, 1, 19)?.and_then(|r| r.percent), None);
        assert!(wire.0.is_empty());
        Ok(())
    }

    #[test]
    fn battery_status_unknown_and_failure_values_stay_unknown() {
        for raw in [
            vec![],
            vec![90],
            vec![90, 50],
            vec![0, 0, 0],
            vec![101, 0, 0],
            vec![255, 0, 0],
            vec![90, 50, 5],
            vec![90, 50, 255],
        ] {
            assert_eq!(battery_status_percent(&raw), None, "{raw:?}");
        }
        assert_eq!(battery_status_percent(&[100, 90, 3]), Some(100));
        assert_eq!(battery_status_percent(&[20, 5, 0]), Some(20));
    }

    #[test]
    fn receiver_collections_share_only_the_same_physical_usb_instance() {
        let short = r"\\?\hid#vid_046d&pid_c53f&mi_02&col01#7&12345678&0&0000#{hid-guid}";
        let long = r"\\?\hid#vid_046d&pid_c53f&mi_02&col02#7&12345678&0&0001#{hid-guid}";
        assert_eq!(receiver_key(short), receiver_key(long));
        assert_ne!(
            receiver_key(short),
            receiver_key(&long.replace("12345678", "87654321"))
        );
        assert_ne!(
            receiver_key(short),
            receiver_key(&long.replace("mi_02", "mi_01"))
        );
        assert_eq!(
            receiver_key(short),
            receiver_key(&short.to_ascii_uppercase())
        );
        assert!(receiver_key(&short.replace("col01", "colzz")).is_none());
    }

    #[test]
    fn malformed_wrong_nonce_and_legacy_protocol_replies_fail_without_panicking() {
        struct Reply(Vec<u8>);
        impl Wire for Reply {
            fn exchange(&mut self, _: &[u8], _: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
                Ok(self.0.clone())
            }
        }
        for reply in [
            vec![],
            vec![0x11],
            vec![0x10, 1, 0, 0x1c, 4, 2, 22],
            vec![0x10, 1, 0x8f, 0, 0x1c, 1, 0],
        ] {
            assert!(logitech(&mut Reply(reply), 1, 23).is_err());
        }
    }
    #[test]
    fn receiver_discovery_sends_only_queries_and_preserves_an_exact_zero() -> Result<()> {
        let mut wire = Script(std::collections::VecDeque::from([
            (hidpp_request(1, 0, 1, &[0, 0, 42]), vec![4, 0, 42]),
            (hidpp_request(1, 0, 0, &[0, 5]), vec![3, 0, 0]),
            (hidpp_request(1, 3, 2, &[]), vec![3]),
            (hidpp_request(1, 0, 0, &[0x10, 4]), vec![5, 0, 0]),
            (hidpp_request(1, 5, 0, &[]), vec![15, 2]),
            (hidpp_request(1, 5, 1, &[]), vec![0, 1, 0, 0]),
        ]));
        let result = logitech(&mut wire, 1, 42)?;
        assert_eq!(
            result,
            Some(Reading {
                slot: 1,
                kind: Kind::Mouse,
                percent: Some(0)
            })
        );
        assert!(wire.0.is_empty());
        Ok(())
    }
    #[test]
    fn unknown_models_and_wrong_interfaces_are_never_probed() {
        assert!(profile_path(r"\\?\hid#vid_1038&pid_2202&mi_03#instance").is_some());
        for path in [
            r"\\?\hid#vid_1038&pid_2202&mi_00#instance",
            r"\\?\hid#vid_046d&pid_ffff&mi_02#instance",
            r"\\server\hid#vid_046d&pid_c52b&mi_02",
            r"C:\data",
        ] {
            assert!(profile_path(path).is_none());
        }
    }
    #[test]
    fn replies_must_belong_to_the_slot_feature_and_software() {
        let request = hidpp_request(1, 3, 1, &[]);
        let mut response = request;
        assert!(hidpp_matches(&request, &response));
        for index in [0, 1, 2, 3] {
            response = request;
            response[index] ^= 1;
            assert!(!hidpp_matches(&request, &response));
        }
        assert!(!hidpp_matches(&request, &request[..6]));
        let mut short = request[..7].to_vec();
        short[0] = 0x10;
        assert!(hidpp_matches(&request, &short));
        short[3] = 0x10; // A device event, not a reply for our software ID.
        assert!(!hidpp_matches(&request, &short));
    }
    #[test]
    fn approximate_levels_errors_and_zero_remain_distinct() {
        assert_eq!(unified_percent(&[15, 2], &[0, 1, 0, 0]), Some(0));
        assert_eq!(unified_percent(&[15, 0], &[0, 8, 0, 0]), None);
        assert_eq!(unified_percent(&[15, 2], &[101, 0, 0, 0]), None);
        assert_eq!(unified_percent(&[15, 2], &[50, 0, 4, 0]), None);
        let raw = [0, 0xb0, 0, 0, 3, 0, 0, 0];
        assert_eq!(
            nova(Model::NovaPercent, &raw).and_then(|r| r.percent),
            Some(0)
        );
        assert_eq!(
            nova(Model::NovaDiscrete, &raw).and_then(|r| r.percent),
            None
        );
        let mut offline = raw;
        offline[4] = 0;
        assert!(nova(Model::NovaPercent, &offline).is_none());
        assert!(nova(Model::NovaPercent, &[0xb0, 3]).is_none());
    }
}
