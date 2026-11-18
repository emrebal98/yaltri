//! Linux: peripheral batteries UPower knows: Bluetooth devices (BlueZ),
//! Logitech receivers (the kernel's HID++ driver) and others the kernel
//! exposes under /sys/class/power_supply. The laptop's own battery and
//! power supplies are left out.

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use super::{DeviceKind, Level, Reading};

const UPOWER: &str = "org.freedesktop.UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";

/// `org.freedesktop.UPower.Device` `Type`.
fn kind_of_type(t: u32) -> Option<DeviceKind> {
    match t {
        5 => Some(DeviceKind::Mouse),
        6 => Some(DeviceKind::Keyboard),
        // Headset, headphones.
        17 | 19 => Some(DeviceKind::Headset),
        _ => None,
    }
}

/// `State` 1: charging, 4: fully charged while plugged in.
fn is_charging(state: u32) -> bool {
    matches!(state, 1 | 4)
}

/// The properties of one UPower device that matter here.
#[derive(Debug, Default)]
pub(crate) struct Device {
    pub kind: u32,
    pub percent: f64,
    pub state: u32,
    pub model: String,
    pub present: bool,
    pub power_supply: bool,
}

pub(crate) fn readings(devices: &[Device]) -> Vec<Reading> {
    devices
        .iter()
        .filter(|d| d.present && !d.power_supply)
        .filter_map(|d| {
            Some(Reading {
                kind: kind_of_type(d.kind)?,
                name: d.model.clone(),
                level: Level::Single { percent: d.percent.round().clamp(0., 100.) as u8, charging: is_charging(d.state) },
            })
        })
        .collect()
}

pub(crate) fn read() -> Vec<Reading> {
    match devices() {
        Ok(devices) => readings(&devices),
        Err(e) => {
            tracing::debug!("reading UPower failed: {e:#}");
            Vec::new()
        }
    }
}

fn devices() -> zbus::Result<Vec<Device>> {
    let bus = Connection::system()?;
    let upower =
        zbus::blocking::Proxy::new(&bus, UPOWER, "/org/freedesktop/UPower", "org.freedesktop.UPower")?;
    let paths: Vec<OwnedObjectPath> = upower.call("EnumerateDevices", &())?;
    let mut devices = Vec::new();
    for path in paths {
        let props = zbus::blocking::fdo::PropertiesProxy::builder(&bus).destination(UPOWER)?.path(path)?.build()?;
        let all: HashMap<String, OwnedValue> = props.get_all(DEVICE.try_into()?)?.into_iter().collect();
        let get = |key: &str| all.get(key).and_then(|v| v.try_clone().ok());
        devices.push(Device {
            kind: get("Type").and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
            percent: get("Percentage").and_then(|v| f64::try_from(v).ok()).unwrap_or(0.),
            state: get("State").and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
            model: get("Model").and_then(|v| String::try_from(v).ok()).unwrap_or_default(),
            present: get("IsPresent").and_then(|v| bool::try_from(v).ok()).unwrap_or(false),
            power_supply: get("PowerSupply").and_then(|v| bool::try_from(v).ok()).unwrap_or(false),
        });
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_peripherals_and_drops_the_laptop_battery() {
        let devices = [
            Device { kind: 2, percent: 71., state: 2, model: "BAT0".into(), present: true, power_supply: true },
            Device { kind: 5, percent: 54.4, state: 2, model: "MX Master 3".into(), present: true, power_supply: false },
            Device { kind: 6, percent: 100., state: 4, model: "K380".into(), present: true, power_supply: false },
            Device { kind: 17, percent: 30., state: 2, model: "WH-1000XM4".into(), present: false, power_supply: false },
            Device { kind: 19, percent: 8., state: 1, model: "Buds".into(), present: true, power_supply: false },
        ];
        let got: Vec<_> = readings(&devices).into_iter().map(|r| (r.kind, r.name, r.level)).collect();
        assert_eq!(
            got,
            [
                (DeviceKind::Mouse, "MX Master 3".into(), Level::Single { percent: 54, charging: false }),
                (DeviceKind::Keyboard, "K380".into(), Level::Single { percent: 100, charging: true }),
                (DeviceKind::Headset, "Buds".into(), Level::Single { percent: 8, charging: true }),
            ]
        );
    }
}
