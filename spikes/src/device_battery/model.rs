use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Node {
    pub identity: String,
    pub name: String,
    pub kind: &'static str,
    pub transport: &'static str,
    pub percent: Option<u8>,
    pub connected: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Device {
    pub name: String,
    pub kind: &'static str,
    pub transports: BTreeSet<&'static str>,
    pub interfaces: usize,
    pub connected: Option<bool>,
    pub percent: Option<u8>,
    pub conflicting_levels: bool,
    pub source: &'static str,
}

pub fn connection(states: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let (mut known, mut unknown) = (false, false);
    for state in states {
        match state {
            Some(true) => return Some(true),
            Some(false) => known = true,
            None => unknown = true,
        }
    }
    (known && !unknown).then_some(false)
}

/// Cache reads can disagree across service nodes. Keep uncertainty instead of
/// choosing an arbitrary interface or averaging separate batteries.
pub fn merge(nodes: Vec<Node>) -> Vec<Device> {
    let mut groups: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for node in nodes {
        groups.entry(node.identity.clone()).or_default().push(node);
    }
    groups
        .into_values()
        .map(|nodes| {
            let levels: BTreeSet<u8> = nodes
                .iter()
                .filter_map(|node| node.percent)
                .filter(|percent| *percent <= 100)
                .collect();
            let connected = connection(nodes.iter().map(|node| node.connected));
            let name = nodes
                .iter()
                .map(|node| &node.name)
                .filter(|name| !name.is_empty())
                .min()
                .cloned()
                .unwrap_or_default();
            let kind = nodes
                .iter()
                .map(|node| node.kind)
                .filter(|kind| *kind != "other")
                .min()
                .unwrap_or("other");
            Device {
                name,
                kind,
                transports: nodes.iter().map(|node| node.transport).collect(),
                interfaces: nodes.len(),
                connected,
                percent: (levels.len() == 1)
                    .then(|| levels.first().copied())
                    .flatten(),
                conflicting_levels: levels.len() > 1,
                source: if levels.is_empty() {
                    "unavailable"
                } else {
                    "windows-device-cache (undocumented property)"
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(identity: &str, percent: Option<u8>, connected: Option<bool>) -> Node {
        Node {
            identity: identity.into(),
            name: "Same name".into(),
            kind: "mouse",
            transport: "Bluetooth",
            percent,
            connected,
        }
    }

    #[test]
    fn zero_is_a_reading_and_absence_is_not_zero() {
        let rows = merge(vec![
            node("one", Some(0), Some(true)),
            node("two", None, None),
        ]);
        assert_eq!(rows[0].percent, Some(0));
        assert_eq!(rows[1].percent, None);
    }

    #[test]
    fn identical_names_do_not_merge_distinct_physical_devices() {
        assert_eq!(
            merge(vec![
                node("one", Some(70), None),
                node("two", Some(30), None)
            ])
            .len(),
            2
        );
    }

    #[test]
    fn duplicate_transports_do_not_change_the_result_with_enumeration_order()
    -> serde_json::Result<()> {
        let a = node("one", Some(70), Some(false));
        let mut b = node("one", Some(70), Some(true));
        b.transport = "Bluetooth LE";
        let first = merge(vec![a.clone(), b.clone()]);
        let second = merge(vec![b, a]);
        assert_eq!(
            serde_json::to_string(&first)?,
            serde_json::to_string(&second)?
        );
        assert_eq!(first[0].interfaces, 2);
        assert_eq!(first[0].connected, Some(true));
        Ok(())
    }

    #[test]
    fn conflicting_levels_are_not_an_invented_global_percentage() {
        let rows = merge(vec![
            node("one", Some(40), None),
            node("one", Some(80), None),
        ]);
        assert_eq!(rows[0].percent, None);
        assert!(rows[0].conflicting_levels);
    }

    #[test]
    fn an_unknown_connection_does_not_prove_disconnection() {
        assert_eq!(
            merge(vec![
                node("one", None, Some(false)),
                node("one", None, None)
            ])[0]
                .connected,
            None
        );
        assert_eq!(
            merge(vec![node("one", None, Some(false))])[0].connected,
            Some(false)
        );
    }

    #[test]
    fn invalid_levels_are_not_clamped_into_apparently_valid_data() {
        assert_eq!(merge(vec![node("one", Some(255), None)])[0].percent, None);
    }

    #[test]
    fn diagnostic_output_omits_physical_identifiers() -> serde_json::Result<()> {
        let rows = merge(vec![node(
            "private-container-or-address",
            Some(100),
            Some(true),
        )]);
        let output = serde_json::to_string(&rows)?;
        assert!(!output.contains("private-container-or-address"));
        assert!(output.contains("100"));
        Ok(())
    }

    #[test]
    fn unknown_endpoints_do_not_disappear_when_combining_transports() {
        assert_eq!(connection([]), None);
        assert_eq!(connection([None, Some(false)]), None);
        assert_eq!(connection([Some(false), None]), None);
        assert_eq!(connection([None, Some(true)]), Some(true));
    }
}
