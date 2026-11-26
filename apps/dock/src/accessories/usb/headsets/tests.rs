use super::*;
use std::collections::VecDeque;

struct Script(VecDeque<(Vec<u8>, Vec<Vec<u8>>)>);
impl Wire for Script {
    fn exchange(&mut self, request: &[u8], accepts: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
        let (expected, responses) = self
            .0
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("unexpected headset command"))?;
        assert_eq!(request, expected);
        responses
            .into_iter()
            .find(|response| accepts(response))
            .ok_or_else(|| anyhow::anyhow!("no matching headset response"))
    }
}
fn script(rows: Vec<(Vec<u8>, Vec<u8>)>) -> Script {
    Script(
        rows.into_iter()
            .map(|(request, response)| (request, vec![response]))
            .collect(),
    )
}
fn cloud_request(command: u8) -> Vec<u8> {
    let mut request = vec![0; 62];
    request[..11].copy_from_slice(&[6, 0, 2, 0, 0x9a, 0, 0, 0x68, 0x4a, 0x8e, 0x0a]);
    request[14] = 0xbb;
    request[15] = command;
    request
}

#[test]
fn alpha_checks_connection_and_never_reads_a_charging_percentage() -> Result<()> {
    for level in [0, 58, 100] {
        let mut wire = script(vec![
            (vec![0x21, 0xbb, 3], vec![0x21, 0xbb, 3, 0]),
            (vec![0x21, 0xbb, 0x0c], vec![0x21, 0xbb, 0x0c, 0]),
            (vec![0x21, 0xbb, 0x0b], vec![0x21, 0xbb, 0x0b, level]),
        ]);
        assert_eq!(
            read(Model::CloudAlpha, &mut wire)?,
            Some(reading(Some(level)))
        );
        assert!(wire.0.is_empty());
    }
    let mut off = script(vec![(vec![0x21, 0xbb, 3], vec![0x21, 0xbb, 3, 1])]);
    assert_eq!(read(Model::CloudAlpha, &mut off)?, None);
    assert!(off.0.is_empty());
    let mut charging = script(vec![
        (vec![0x21, 0xbb, 3], vec![0x21, 0xbb, 3, 0]),
        (vec![0x21, 0xbb, 0x0c], vec![0x21, 0xbb, 0x0c, 1]),
    ]);
    assert_eq!(read(Model::CloudAlpha, &mut charging)?, Some(reading(None)));
    assert!(charging.0.is_empty());
    Ok(())
}

#[test]
fn kingston_connection_and_pairing_gate_the_battery_read() -> Result<()> {
    for state in [0, 1, 2, 4] {
        let mut rows = vec![(cloud_request(1), vec![0x0b, 0, 0xbb, 1, state])];
        let expected = if matches!(state, 1 | 4) {
            rows.push((cloud_request(2), vec![0x0b, 0, 0xbb, 2, 0, 0, 0, 0]));
            Some(reading(Some(0)))
        } else {
            None
        };
        let mut wire = script(rows);
        assert_eq!(read(Model::Cloud2Kingston, &mut wire)?, expected);
        assert!(wire.0.is_empty());
    }
    Ok(())
}

#[test]
fn nova_variants_keep_their_own_connection_and_battery_layouts() -> Result<()> {
    for model in [Model::Nova5, Model::NovaDiscrete, Model::NovaPercent] {
        for level in [0, 4, 100, 255] {
            let mut raw = vec![0; 16];
            raw[0] = 0xb0;
            if model == Model::Nova5 {
                raw[1] = 1;
                raw[3] = level;
            } else {
                raw[2] = level;
                raw[3] = 3;
            }
            let valid = level <= if model == Model::NovaDiscrete { 4 } else { 100 };
            let expected = valid.then(|| reading((model != Model::NovaDiscrete).then_some(level)));
            let mut wire = script(vec![(vec![0, 0xb0], raw.clone())]);
            assert_eq!(read(model, &mut wire)?, expected);
            raw[if model == Model::Nova5 { 1 } else { 3 }] =
                if model == Model::Nova5 { 2 } else { 0 };
            let mut wire = script(vec![(vec![0, 0xb0], raw)]);
            assert_eq!(read(model, &mut wire)?, None);
        }
    }
    Ok(())
}

#[test]
fn truncated_unrelated_and_out_of_range_reports_are_rejected() {
    for raw in [
        vec![],
        vec![0x21],
        vec![0x21, 0xbb, 3],
        vec![0x21, 0, 3, 0],
        vec![0x21, 0xbb, 0x0b, 50],
        vec![0x21, 0xbb, 3, 255],
    ] {
        let mut wire = script(vec![(vec![0x21, 0xbb, 3], raw)]);
        assert!(read(Model::CloudAlpha, &mut wire).is_err());
    }
    for level in [101, 255] {
        let mut wire = script(vec![
            (cloud_request(1), vec![0x0b, 0, 0xbb, 1, 1]),
            (cloud_request(2), vec![0x0b, 0, 0xbb, 2, 0, 0, 0, level]),
        ]);
        assert!(read(Model::Cloud2Kingston, &mut wire).is_err());
    }
}

fn cpl(payload: &[u8]) -> Vec<u8> {
    let mut raw = vec![0; 64];
    raw[0] = 0x51;
    raw[1] = (payload.len() + 1) as u8;
    raw[3..3 + payload.len()].copy_from_slice(payload);
    raw
}
fn bridge_request(feature: u8, params: &[u8]) -> Vec<u8> {
    let mut data = vec![3, 0x1c, 0, (params.len() + 3) as u8, 0, feature, 0x0c];
    data.extend_from_slice(params);
    cpl(&data)
}
fn bridge_reply(feature: u8, params: &[u8]) -> Vec<u8> {
    let mut data = vec![3, 0x10, 0, (params.len() + 3) as u8, 0, feature, 0x0c];
    data.extend_from_slice(params);
    cpl(&data)
}

#[test]
fn centurion_discovers_only_bridge_and_soc_while_ignoring_ack_and_foreign_replies() -> Result<()> {
    for level in [0, 73, 100] {
        let mut wire = Script(VecDeque::from([
            (
                cpl(&[0, 0x0c, 0, 3]),
                vec![cpl(&[0, 1, 99]), cpl(&[0, 0x0c, 3, 0, 0])],
            ),
            (cpl(&[3, 0x0c]), vec![cpl(&[3, 0x0c, 3, 0xe8, 0, 1, 1])]),
            (
                bridge_request(0, &[1, 4]),
                vec![cpl(&[3, 0x1c]), bridge_reply(0, &[6, 0, 0])],
            ),
            (
                bridge_request(6, &[]),
                vec![bridge_reply(5, &[90]), bridge_reply(6, &[level, 0, 0])],
            ),
        ]));
        assert_eq!(read(Model::ProX2, &mut wire)?, Some(reading(Some(level))));
        assert!(wire.0.is_empty());
    }
    Ok(())
}

#[test]
fn centurion_disconnected_link_never_reads_soc() -> Result<()> {
    let mut wire = script(vec![
        (cpl(&[0, 0x0c, 0, 3]), cpl(&[0, 0x0c, 3, 0, 0])),
        (cpl(&[3, 0x0c]), cpl(&[3, 0x0c, 3, 0xe8, 0, 0, 1])),
    ]);
    assert_eq!(read(Model::ProX2, &mut wire)?, None);
    assert!(wire.0.is_empty());
    Ok(())
}

#[test]
fn centurion_checks_lengths_nested_errors_and_radio_disconnects() {
    for response in [
        cpl(&[3, 0, 0, 0]),
        bridge_reply(6, &[255]),
        cpl(&[3, 0x10, 0, 8, 0, 6, 0x0c, 20]),
        cpl(&[3, 0x10, 0, 4, 0, 0xff, 6, 0x0c]),
    ] {
        let disconnected = response[4] == 0;
        let mut wire = script(vec![
            (cpl(&[0, 0x0c, 0, 3]), cpl(&[0, 0x0c, 3, 0, 0])),
            (cpl(&[3, 0x0c]), cpl(&[3, 0x0c, 3, 0xe8, 0, 1, 1])),
            (bridge_request(0, &[1, 4]), bridge_reply(0, &[6, 0, 0])),
            (bridge_request(6, &[]), response),
        ]);
        let result = read(Model::ProX2, &mut wire);
        if disconnected {
            assert!(matches!(result, Ok(None)));
        } else {
            assert!(result.is_err());
        }
    }
}
