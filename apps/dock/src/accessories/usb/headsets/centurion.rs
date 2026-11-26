//! G PRO X 2 (046D:0AF7): discover only Bridge and Battery SoC, never enumerate
//! every feature or send initialization, audio, pairing or firmware commands.
use super::{checked, reading};
use crate::accessories::usb::protocol::{Reading, Wire};
use anyhow::{Result, ensure};

const SOFTWARE: u8 = 0x0c;

fn frame(data: &[u8]) -> [u8; 64] {
    let mut raw = [0; 64];
    raw[0] = 0x51;
    raw[1] = (data.len() + 1) as u8;
    raw[3..3 + data.len()].copy_from_slice(data);
    raw
}

fn payload(raw: &[u8]) -> Option<&[u8]> {
    if raw.len() > 64 || raw.len() < 5 || raw[0] != 0x51 || raw[2] != 0 {
        return None;
    }
    let end = usize::from(raw[1]) + 2;
    (end >= 5).then(|| raw.get(3..end)).flatten()
}

fn direct(wire: &mut impl Wire, feature: u8, params: &[u8]) -> Result<Vec<u8>> {
    let mut data = vec![feature, SOFTWARE];
    data.extend_from_slice(params);
    let raw = checked(wire, &frame(&data), |raw| {
        payload(raw).is_some_and(|p| {
            p.starts_with(&[feature, SOFTWARE]) || p.starts_with(&[0xff, feature, SOFTWARE])
        })
    })?;
    let p = payload(&raw).ok_or_else(|| anyhow::anyhow!("invalid Centurion frame"))?;
    ensure!(p[0] == feature, "Centurion query rejected");
    Ok(p[2..].to_vec())
}

fn bridged(
    wire: &mut impl Wire,
    bridge: u8,
    feature: u8,
    params: &[u8],
) -> Result<Option<Vec<u8>>> {
    let mut data = vec![
        bridge,
        0x10 | SOFTWARE,
        0,
        (params.len() + 3) as u8,
        0,
        feature,
        SOFTWARE,
    ];
    data.extend_from_slice(params);
    let raw = checked(wire, &frame(&data), |raw| {
        payload(raw).is_some_and(|p| {
            let reply = p.len() >= 7
                && p[0] == bridge
                && p[1] == 0x10
                && p[4] == 0
                && (p[5..].starts_with(&[feature, SOFTWARE])
                    || p[5..].starts_with(&[0xff, feature, SOFTWARE]));
            // ACKs and events for other software are never battery answers.
            p.starts_with(&[bridge, 0, 0, 0])
                || reply
                || p.starts_with(&[0xff, bridge, 0x10 | SOFTWARE])
        })
    })?;
    let p = payload(&raw).ok_or_else(|| anyhow::anyhow!("invalid Centurion bridge frame"))?;
    if p.starts_with(&[bridge, 0, 0, 0]) {
        return Ok(None);
    }
    ensure!(
        p[0] == bridge && p.len() >= 7 && p[5] != 0xff,
        "Centurion bridge rejected query"
    );
    let size = (usize::from(p[2] & 0x0f) << 8) | usize::from(p[3]);
    ensure!(
        p[2] & 0xf0 == 0 && size >= 3 && size == p.len() - 4,
        "invalid Centurion bridge length"
    );
    Ok(Some(p[7..].to_vec()))
}

pub(super) fn read(wire: &mut impl Wire) -> Result<Option<Reading>> {
    let root = direct(wire, 0, &[0, 3])?;
    let bridge = *root
        .first()
        .filter(|index| **index != 0 && **index != 0xff)
        .ok_or_else(|| anyhow::anyhow!("Centurion bridge unavailable"))?;
    let connection = direct(wire, bridge, &[])?;
    ensure!(connection.len() >= 4, "short Centurion link status");
    if connection[2..4] == [0, 0] {
        return Ok(None);
    }
    ensure!(connection[2..4] == [0, 1], "unknown Centurion link status");
    let Some(info) = bridged(wire, bridge, 0, &[1, 4])? else {
        return Ok(None);
    };
    let soc = *info
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing Centurion battery feature"))?;
    if soc == 0 {
        return Ok(Some(reading(None)));
    }
    ensure!(soc != 0xff, "invalid Centurion feature index");
    let Some(battery) = bridged(wire, bridge, soc, &[])? else {
        return Ok(None);
    };
    let percent = *battery
        .first()
        .filter(|value| **value <= 100)
        .ok_or_else(|| anyhow::anyhow!("invalid Centurion battery level"))?;
    Ok(Some(reading(Some(percent))))
}
