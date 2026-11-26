use std::{collections::BTreeMap, mem::size_of};

use anyhow::{Context, Result, ensure};
use windows::{
    Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                DIGCF_ALLCLASSES, DIGCF_PRESENT, GUID_DEVCLASS_MEDIA, GUID_DEVCLASS_MOUSE,
                HDEVINFO, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
                SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
            },
            Properties::{
                DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_ContainerId,
                DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName, DEVPROP_TYPE_BOOLEAN,
                DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING, DEVPROPTYPE,
            },
        },
        Foundation::{DEVPROPKEY, ERROR_NO_MORE_ITEMS},
    },
    core::{GUID, PCWSTR, w},
};

use super::model::Node;

// Compatibility probes only: these cached Bluetooth keys are not a public
// battery contract. No HID handles, feature reports or GATT reads are issued.
const BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};
const CONNECTED: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29),
    pid: 15,
};
const MAX_NODES: usize = 256;

struct Devices(HDEVINFO);
impl Drop for Devices {
    fn drop(&mut self) {
        // SAFETY: This set was opened by this object and is closed once.
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

impl Devices {
    fn property(&self, node: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<(DEVPROPTYPE, Vec<u8>)> {
        let mut bytes = [0u8; 4096];
        let mut kind = DEVPROPTYPE::default();
        let mut size = 0;
        // SAFETY: The node belongs to this set; all buffers live for the call.
        unsafe {
            SetupDiGetDevicePropertyW(
                self.0,
                node,
                key,
                &mut kind,
                Some(&mut bytes),
                Some(&mut size),
                0,
            )
        }
        .ok()?;
        let data = bytes.get(..size as usize)?;
        Some((kind, data.to_vec()))
    }

    fn text(&self, node: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
        let (kind, bytes) = self.property(node, key)?;
        if kind != DEVPROP_TYPE_STRING || bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        let text = String::from_utf16_lossy(&units);
        (!text.is_empty()).then_some(text)
    }

    fn container(&self, node: &SP_DEVINFO_DATA) -> Option<String> {
        let (kind, bytes) = self.property(node, &DEVPKEY_Device_ContainerId)?;
        if kind != DEVPROP_TYPE_GUID || bytes.len() != size_of::<GUID>() {
            return None;
        }
        // SAFETY: A GUID has no invalid bit patterns; the length is checked.
        let guid = unsafe { bytes.as_ptr().cast::<GUID>().read_unaligned() };
        (guid != GUID::zeroed()).then(|| format!("{guid:?}"))
    }

    fn byte(&self, node: &SP_DEVINFO_DATA, key: &DEVPROPKEY, expected: DEVPROPTYPE) -> Option<u8> {
        let (kind, bytes) = self.property(node, key)?;
        (kind == expected && bytes.len() == 1).then(|| bytes[0])
    }
}

pub fn snapshot() -> Result<Vec<Node>> {
    let mut found = BTreeMap::new();
    for (class, enumerator, transport, kind) in [
        (None, w!("BTHENUM"), "Bluetooth", "other"),
        (None, w!("BTHLE"), "Bluetooth LE", "other"),
        (None, w!("BTHLEDEVICE"), "Bluetooth LE", "other"),
        (
            Some(&GUID_DEVCLASS_MOUSE),
            PCWSTR::null(),
            "HID (connection unknown)",
            "mouse",
        ),
        (
            Some(&GUID_DEVCLASS_MEDIA),
            PCWSTR::null(),
            "USB audio (connection unknown)",
            "headset candidate",
        ),
    ] {
        let flags = DIGCF_PRESENT
            | if class.is_none() {
                DIGCF_ALLCLASSES
            } else {
                Default::default()
            };
        // SAFETY: Class and enumerator are valid for this read-only enumeration.
        let devices = Devices(
            unsafe { SetupDiGetClassDevsW(class.map(std::ptr::from_ref), enumerator, None, flags) }
                .context("enumerating cached device properties")?,
        );
        for index in 0..=MAX_NODES as u32 {
            let mut node = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            // SAFETY: A live set and correctly sized output structure.
            match unsafe { SetupDiEnumDeviceInfo(devices.0, index, &mut node) } {
                Ok(()) => {}
                Err(error) if error.code() == ERROR_NO_MORE_ITEMS.to_hresult() => break,
                Err(error) => return Err(error).context("reading a device node"),
            }
            ensure!(
                index < MAX_NODES as u32,
                "device enumeration exceeded {MAX_NODES} nodes per scope"
            );
            let mut instance = [0u16; 512];
            // SAFETY: The output buffer lives through this call.
            unsafe { SetupDiGetDeviceInstanceIdW(devices.0, &node, Some(&mut instance), None) }?;
            let mut instance = String::from_utf16_lossy(
                &instance[..instance
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(instance.len())],
            );
            instance.make_ascii_uppercase();
            if kind == "headset candidate" && !instance.starts_with("USB\\") {
                continue;
            }
            let name = devices
                .text(&node, &DEVPKEY_Device_FriendlyName)
                .or_else(|| devices.text(&node, &DEVPKEY_Device_BusReportedDeviceDesc))
                .or_else(|| devices.text(&node, &DEVPKEY_Device_DeviceDesc))
                .unwrap_or_default();
            let identity = devices.container(&node).unwrap_or_else(|| instance.clone());
            let percent = devices
                .byte(&node, &BATTERY, DEVPROP_TYPE_BYTE)
                .filter(|value| *value <= 100);
            // A connected HID/USB node may only be a receiver. Its presence
            // does not establish the wireless peripheral's connection.
            let connected = if class.is_none() {
                devices
                    .byte(&node, &CONNECTED, DEVPROP_TYPE_BOOLEAN)
                    .map(|value| value != 0)
            } else {
                None
            };
            found.entry(instance).or_insert(Node {
                identity,
                name,
                kind,
                transport,
                percent,
                connected,
            });
        }
    }
    Ok(found.into_values().collect())
}
