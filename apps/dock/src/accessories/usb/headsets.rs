//! Small, read-only adapters. VID/PID and HID descriptors are checked before IO.
//! Protocol references and unverified hardware limits are in ACCESSORIES.md.
mod centurion;
#[cfg(test)]
mod tests;

use super::protocol::{self, Model, Reading, Wire};
use anyhow::{Result, ensure};
use nosih_shell::accessories::Kind;

fn reading(percent: Option<u8>) -> Reading {
    Reading {
        slot: 0,
        kind: Kind::Headset,
        percent,
    }
}

fn checked(
    wire: &mut impl Wire,
    request: &[u8],
    accepts: impl Fn(&[u8]) -> bool,
) -> Result<Vec<u8>> {
    let response = wire.exchange(request, &accepts)?;
    ensure!(accepts(&response), "invalid headset response");
    Ok(response)
}

pub fn read(model: Model, wire: &mut impl Wire) -> Result<Option<Reading>> {
    match model {
        Model::NovaDiscrete | Model::NovaPercent | Model::Nova5 => {
            let raw = checked(wire, &[0, 0xb0], |raw| {
                raw.first() == Some(&0xb0) || raw.starts_with(&[0, 0xb0])
            })?;
            Ok(protocol::nova(model, &raw))
        }
        Model::CloudAlpha => alpha(wire),
        Model::Cloud2Kingston => cloud2(wire),
        Model::ProX2 => centurion::read(wire),
        _ => anyhow::bail!("unexpected headset protocol"),
    }
}

fn alpha_command(wire: &mut impl Wire, command: u8) -> Result<Vec<u8>> {
    checked(wire, &[0x21, 0xbb, command], |raw| {
        raw.len() >= 4 && raw[..3] == [0x21, 0xbb, command]
    })
}

fn alpha(wire: &mut impl Wire) -> Result<Option<Reading>> {
    let connection = alpha_command(wire, 3)?;
    match connection[3] {
        1 => return Ok(None),
        0 => {}
        _ => anyhow::bail!("unknown Cloud Alpha connection state"),
    }
    let charge = alpha_command(wire, 0x0c)?;
    ensure!(charge[3] <= 2, "unknown Cloud Alpha charge state");
    if charge[3] == 1 {
        // This firmware does not supply a reliable percentage while charging.
        return Ok(Some(reading(None)));
    }
    let battery = alpha_command(wire, 0x0b)?;
    ensure!(battery[3] <= 100, "invalid Cloud Alpha battery level");
    Ok(Some(reading(Some(battery[3]))))
}

fn cloud2_command(wire: &mut impl Wire, command: u8, minimum: usize) -> Result<Vec<u8>> {
    let mut request = [0; 62];
    request[..11].copy_from_slice(&[6, 0, 2, 0, 0x9a, 0, 0, 0x68, 0x4a, 0x8e, 0x0a]);
    request[14] = 0xbb;
    request[15] = command;
    checked(wire, &request, |raw| {
        raw.len() >= minimum && raw[..4] == [0x0b, 0, 0xbb, command]
    })
}

fn cloud2(wire: &mut impl Wire) -> Result<Option<Reading>> {
    // Non-DTS Kingston only. The HP/DTS connection query can reset mic state;
    // those USB IDs are deliberately excluded instead of sending it there.
    let connection = cloud2_command(wire, 1, 5)?;
    match connection[4] {
        0 | 2 => return Ok(None), // Disconnected or pairing, not an active headset.
        1 | 4 => {}
        _ => anyhow::bail!("unknown Cloud II connection state"),
    }
    let battery = cloud2_command(wire, 2, 8)?;
    ensure!(battery[7] <= 100, "invalid Cloud II battery level");
    Ok(Some(reading(Some(battery[7]))))
}
