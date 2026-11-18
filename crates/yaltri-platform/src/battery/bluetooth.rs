//! Windows: the battery Windows itself shows for Bluetooth devices (classic
//! hands-free headsets and Bluetooth LE mice, keyboards and earbuds), kept
//! as a property on the device's Bluetooth node.
//!
//! The node does not say what the device is, so the kind comes from the
//! other nodes in its device container: a mouse, a keyboard or an audio
//! endpoint. A container with none of those present is a paired device
//! that is not connected; its last level would be stale, so it is skipped.

use std::collections::HashMap;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDevicePropertyW,
};
use windows_sys::Win32::Devices::Properties::{
    DEVPKEY_Device_Class, DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPKEY_NAME,
};
use windows_sys::Win32::Foundation::{DEVPROPKEY, INVALID_HANDLE_VALUE};
use windows_sys::core::GUID;

use super::{DeviceKind, Level, Reading};

/// `DEVPKEY_Bluetooth_Battery`: the level in percent, one byte.
const BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

/// What one device node tells.
#[derive(Debug, Default)]
pub(crate) struct Node {
    pub battery: Option<u8>,
    pub container: Option<u128>,
    /// Setup class name: `Mouse`, `Keyboard`, `AudioEndpoint`, `Bluetooth`…
    pub class: String,
    pub name: String,
}

fn kind_of_class(class: &str) -> Option<DeviceKind> {
    match class {
        "Mouse" => Some(DeviceKind::Mouse),
        "Keyboard" => Some(DeviceKind::Keyboard),
        "AudioEndpoint" | "MEDIA" => Some(DeviceKind::Headset),
        _ => None,
    }
}

/// Readings for the nodes that carry a battery level, typed by their
/// container's other nodes. A mouse or keyboard wins over audio (some mice
/// carry a microphone driver, never the other way round).
pub(crate) fn readings(nodes: &[Node]) -> Vec<Reading> {
    let mut kinds: HashMap<u128, DeviceKind> = HashMap::new();
    for node in nodes {
        if let (Some(container), Some(kind)) = (node.container, kind_of_class(&node.class)) {
            let entry = kinds.entry(container).or_insert(kind);
            if *entry == DeviceKind::Headset {
                *entry = kind;
            }
        }
    }
    nodes
        .iter()
        .filter_map(|node| {
            let percent = node.battery?;
            let kind = *kinds.get(&node.container?)?;
            Some(Reading {
                kind,
                name: node.name.clone(),
                level: Level::Single { percent: percent.min(100), charging: false },
            })
        })
        .collect()
}

pub(crate) fn read() -> Vec<Reading> {
    readings(&nodes())
}

/// Every present device node, with the properties `readings` needs.
fn nodes() -> Vec<Node> {
    // SAFETY: a fresh device info set, walked with a correctly sized
    // SP_DEVINFO_DATA and destroyed before returning.
    unsafe {
        let set = SetupDiGetClassDevsW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), DIGCF_ALLCLASSES | DIGCF_PRESENT);
        if set as isize == INVALID_HANDLE_VALUE as isize {
            tracing::debug!("listing devices failed");
            return Vec::new();
        }
        let mut nodes = Vec::new();
        let mut data: SP_DEVINFO_DATA = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        let mut index = 0;
        while SetupDiEnumDeviceInfo(set, index, &mut data) != 0 {
            index += 1;
            let get = |key: &DEVPROPKEY| -> Option<Vec<u8>> {
                let mut kind = 0u32;
                let mut buf = vec![0u8; 512];
                let mut needed = 0u32;
                let ok = SetupDiGetDevicePropertyW(
                    set,
                    &data,
                    key,
                    &mut kind,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut needed,
                    0,
                );
                (ok != 0).then(|| {
                    buf.truncate(needed as usize);
                    buf
                })
            };
            let text = |key: &DEVPROPKEY| get(key).map(|b| utf16(&b)).unwrap_or_default();
            let name = Some(text(&DEVPKEY_Device_FriendlyName)).filter(|n| !n.is_empty()).unwrap_or_else(|| text(&DEVPKEY_NAME));
            nodes.push(Node {
                battery: get(&BATTERY).and_then(|b| b.first().copied()),
                container: get(&DEVPKEY_Device_ContainerId)
                    .and_then(|b| <[u8; 16]>::try_from(b.as_slice()).ok())
                    .map(u128::from_le_bytes),
                class: text(&DEVPKEY_Device_Class),
                name,
            });
        }
        SetupDiDestroyDeviceInfoList(set);
        nodes
    }
}

/// A NUL-terminated UTF-16 property value.
fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(class: &str, container: u128, battery: Option<u8>, name: &str) -> Node {
        Node { battery, container: Some(container), class: class.into(), name: name.into() }
    }

    #[test]
    fn types_a_battery_by_its_container() {
        let nodes = [
            node("Bluetooth", 1, Some(80), "MX Anywhere"),
            node("HIDClass", 1, None, "HID"),
            node("Mouse", 1, None, "HID-compliant mouse"),
            node("Bluetooth", 2, Some(55), "WH-1000XM4"),
            node("AudioEndpoint", 2, None, "Headphones"),
            node("Bluetooth", 3, Some(140), "K380"),
            node("Keyboard", 3, None, "HID Keyboard Device"),
            node("MEDIA", 3, None, "odd audio driver"),
        ];
        let got = readings(&nodes);
        let summary: Vec<_> = got.iter().map(|r| (r.kind, r.name.as_str(), r.level.headline())).collect();
        assert_eq!(
            summary,
            [
                (DeviceKind::Mouse, "MX Anywhere", Some(80)),
                (DeviceKind::Headset, "WH-1000XM4", Some(55)),
                (DeviceKind::Keyboard, "K380", Some(100)),
            ]
        );
    }

    #[test]
    fn skips_paired_devices_that_are_not_connected() {
        // Only the Bluetooth node is present: nothing tells what it is,
        // and its level is the last one seen.
        let nodes = [node("Bluetooth", 7, Some(40), "Old mouse"), node("Mouse", 8, None, "USB mouse")];
        assert!(readings(&nodes).is_empty());
    }

    #[test]
    fn reads_utf16_names() {
        let bytes: Vec<u8> = "Mouse\0junk".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(utf16(&bytes), "Mouse");
    }
}
