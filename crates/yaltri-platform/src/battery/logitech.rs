//! Windows: Logitech mice, keyboards and headsets over HID++ 2.0, on a
//! Unifying, Lightspeed or Bolt receiver (devices 1–6) or connected
//! directly (device 0xFF). Linux leaves this to the kernel's
//! `hid-logitech-hidpp`, which UPower reports.
//!
//! A request is a long report `11 <device> <feature index> <function|sw>
//! <params…>`, 20 bytes. The answer echoes the first four bytes; an error
//! comes back as `11 <device> FF <feature index> <function|sw> <code>`, or
//! from a receiver for a device that isn't there as the short HID++ 1.0
//! `10 <device> 8F …`. Features are found by id through the root feature
//! (index 0). Battery, in order of preference: 0x1004 Unified Battery,
//! 0x1000 Battery Status, 0x1001 Battery Voltage (an approximate percent
//! from the voltage). The kind comes from 0x0005 Device Name & Type.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use hidapi::{HidApi, HidDevice};

use super::{DeviceKind, Level, Reading};

const LOGITECH: u16 = 0x046d;
/// Unifying, Nano, Lightspeed and Bolt receivers: devices sit at 1–6.
const RECEIVERS: &[u16] =
    &[0xc52b, 0xc532, 0xc534, 0xc539, 0xc53a, 0xc53d, 0xc53f, 0xc541, 0xc545, 0xc547, 0xc548];
/// HID++ collections: short (7 byte) and long (20 byte) reports.
const USAGE_PAGE: u16 = 0xff00;
const USAGE_SHORT: u16 = 0x0001;
const USAGE_LONG: u16 = 0x0002;
const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
/// Our software id, so answers to others (Logitech's own app) are ignored.
const SW_ID: u8 = 0x0b;
/// A device that is off answers quickly with an error; a missing slot or
/// a device that doesn't speak HID++ 2.0 may not answer at all.
const TIMEOUT: Duration = Duration::from_millis(300);

const ROOT: u8 = 0x00;
const DEVICE_NAME: u16 = 0x0005;
const UNIFIED_BATTERY: u16 = 0x1004;
const BATTERY_STATUS: u16 = 0x1000;
const BATTERY_VOLTAGE: u16 = 0x1001;

/// What one input report means for a request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The 16 parameter bytes of the matching answer.
    Data([u8; 16]),
    /// HID++ error code.
    Error(u8),
    /// Not ours.
    Other,
}

pub(crate) fn request(device: u8, feature: u8, function: u8, params: &[u8]) -> [u8; 20] {
    let mut q = [0u8; 20];
    q[0] = LONG;
    q[1] = device;
    q[2] = feature;
    q[3] = (function << 4) | SW_ID;
    q[4..4 + params.len().min(16)].copy_from_slice(&params[..params.len().min(16)]);
    q
}

pub(crate) fn classify(r: &[u8], device: u8, feature: u8, function: u8) -> Answer {
    let id = (function << 4) | SW_ID;
    if r.len() < 6 || r[1] != device {
        return Answer::Other;
    }
    match (r[0], r[2]) {
        (LONG, f) if f == feature && r[3] == id => {
            let mut data = [0u8; 16];
            let tail = &r[4..r.len().min(20)];
            data[..tail.len()].copy_from_slice(tail);
            Answer::Data(data)
        }
        (LONG, 0xff) | (SHORT, 0x8f) if r[3] == feature && r[4] == id => Answer::Error(r[5]),
        _ => Answer::Other,
    }
}

/// 0x1004 `get_status`: state of charge, level flags, charging status.
/// Devices without a percentage report 0 and only the level flags.
pub(crate) fn parse_unified(d: &[u8; 16]) -> Level {
    let percent = match (d[0], d[1]) {
        (p, _) if p > 0 => p.min(100),
        (_, l) if l & 0x08 != 0 => 100,
        (_, l) if l & 0x04 != 0 => 50,
        (_, l) if l & 0x02 != 0 => 20,
        _ => 5,
    };
    // 1 charging, 2 slow charging, 3 complete; 0 discharging, 4 error.
    Level::Single { percent, charging: matches!(d[2], 1..=3) }
}

/// 0x1000 `getBatteryLevelStatus`: level, next level, status.
pub(crate) fn parse_status(d: &[u8; 16]) -> Level {
    // 1 recharging, 2 almost full, 3 charged, 4 slow recharge.
    let charging = matches!(d[2], 1..=4);
    let percent = if d[0] == 0 && d[2] == 3 { 100 } else { d[0].min(100) };
    Level::Single { percent, charging }
}

/// 0x1001 `getBatteryInfo`: millivolts (big endian) and flags; bit 7 of
/// the flags is set while charging.
pub(crate) fn parse_voltage(d: &[u8; 16]) -> Level {
    let mv = u16::from_be_bytes([d[0], d[1]]);
    Level::Single { percent: voltage_percent(mv), charging: d[2] & 0x80 != 0 }
}

/// A single Li-ion cell's charge from its voltage, roughly (the curve
/// Solaar uses), interpolated between points.
pub(crate) fn voltage_percent(mv: u16) -> u8 {
    const CURVE: [(u16, u8); 13] = [
        (4186, 100),
        (4067, 90),
        (3989, 80),
        (3922, 70),
        (3859, 60),
        (3811, 50),
        (3778, 40),
        (3751, 30),
        (3717, 20),
        (3671, 10),
        (3646, 5),
        (3579, 2),
        (3500, 0),
    ];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for pair in CURVE.windows(2) {
        let ((hi_mv, hi_p), (lo_mv, lo_p)) = (pair[0], pair[1]);
        if mv >= lo_mv {
            let span = f32::from(hi_mv - lo_mv);
            let share = f32::from(mv - lo_mv) / span;
            return (f32::from(lo_p) + share * f32::from(hi_p - lo_p)).round() as u8;
        }
    }
    0
}

/// 0x0005 `getDeviceType`.
pub(crate) fn kind_of_type(t: u8) -> Option<DeviceKind> {
    match t {
        // Keyboard, numpad.
        0 | 2 => Some(DeviceKind::Keyboard),
        // Mouse, trackpad, trackball.
        3..=5 => Some(DeviceKind::Mouse),
        8 => Some(DeviceKind::Headset),
        _ => None,
    }
}

/// The HID++ collections of one receiver or device.
struct Channel {
    long: HidDevice,
    short: Option<HidDevice>,
}

impl Channel {
    fn call(&self, device: u8, feature: u8, function: u8, params: &[u8]) -> Result<[u8; 16]> {
        self.long.write(&request(device, feature, function, params))?;
        let deadline = Instant::now() + TIMEOUT;
        let mut buf = [0u8; 64];
        while Instant::now() < deadline {
            for handle in std::iter::once(&self.long).chain(self.short.as_ref()) {
                let n = handle.read_timeout(&mut buf, 5)?;
                match classify(&buf[..n], device, feature, function) {
                    Answer::Data(d) => return Ok(d),
                    Answer::Error(code) => bail!("HID++ error {code:#04x}"),
                    Answer::Other => {}
                }
            }
        }
        bail!("no answer")
    }

    /// Index of feature `id`, `None` when the device lacks it.
    fn feature(&self, device: u8, id: u16) -> Result<Option<u8>> {
        let [hi, lo] = id.to_be_bytes();
        let d = self.call(device, ROOT, 0, &[hi, lo])?;
        Ok((d[0] != 0).then_some(d[0]))
    }

    fn name(&self, device: u8, index: u8) -> Result<String> {
        let len = usize::from(self.call(device, index, 0, &[])?[0]);
        let mut name = Vec::new();
        while name.len() < len {
            let chunk = self.call(device, index, 1, &[name.len() as u8])?;
            let take = (len - name.len()).min(16);
            if chunk[..take].iter().all(|b| *b == 0) {
                break;
            }
            name.extend_from_slice(&chunk[..take]);
        }
        Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').trim().to_owned())
    }

    fn reading(&self, device: u8) -> Result<Option<Reading>> {
        let Some(names) = self.feature(device, DEVICE_NAME)? else { return Ok(None) };
        let Some(kind) = kind_of_type(self.call(device, names, 2, &[])?[0]) else { return Ok(None) };
        let level = if let Some(i) = self.feature(device, UNIFIED_BATTERY)? {
            parse_unified(&self.call(device, i, 1, &[])?)
        } else if let Some(i) = self.feature(device, BATTERY_STATUS)? {
            parse_status(&self.call(device, i, 0, &[])?)
        } else if let Some(i) = self.feature(device, BATTERY_VOLTAGE)? {
            parse_voltage(&self.call(device, i, 0, &[])?)
        } else {
            return Ok(None);
        };
        let name = self.name(device, names).unwrap_or_default();
        Ok(Some(Reading { kind, name, level }))
    }
}

/// Every Logitech device that answers, over each receiver's or device's
/// HID++ collections.
pub(crate) fn read(api: &HidApi) -> Vec<Reading> {
    let mut readings = Vec::new();
    let hidpp = |usage: u16| {
        api.device_list().filter(move |i| i.vendor_id() == LOGITECH && i.usage_page() == USAGE_PAGE && i.usage() == usage)
    };
    for long in hidpp(USAGE_LONG) {
        let short = hidpp(USAGE_SHORT)
            .find(|s| s.product_id() == long.product_id() && s.interface_number() == long.interface_number());
        let Ok(long_dev) = long.open_device(api) else { continue };
        let channel = Channel { long: long_dev, short: short.and_then(|s| s.open_device(api).ok()) };
        let devices: Vec<u8> = if RECEIVERS.contains(&long.product_id()) { (1..=6).collect() } else { vec![0xff] };
        for device in devices {
            match channel.reading(device) {
                Ok(Some(reading)) => readings.push(reading),
                Ok(None) => {}
                Err(e) => tracing::debug!(product = long.product_id(), device, "Logitech battery: {e:#}"),
            }
        }
    }
    readings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(bytes: &[u8]) -> [u8; 16] {
        let mut d = [0u8; 16];
        d[..bytes.len()].copy_from_slice(bytes);
        d
    }

    #[test]
    fn builds_long_requests() {
        // Root getFeature(0x1004) for device 2.
        let q = request(2, ROOT, 0, &[0x10, 0x04]);
        assert_eq!(&q[..6], &[0x11, 0x02, 0x00, 0x0b, 0x10, 0x04]);
        assert_eq!(q.len(), 20);
    }

    #[test]
    fn matches_answers_and_errors() {
        let mut answer = vec![0x11, 0x02, 0x00, 0x0b, 0x07, 0x00, 0x02];
        answer.resize(20, 0);
        assert_eq!(classify(&answer, 2, 0, 0), Answer::Data(data(&[0x07, 0x00, 0x02])));
        assert_eq!(classify(&answer, 3, 0, 0), Answer::Other, "another device");
        assert_eq!(classify(&answer, 2, 0, 1), Answer::Other, "another function");
        let mut other_app = answer.clone();
        other_app[3] = 0x01;
        assert_eq!(classify(&other_app, 2, 0, 0), Answer::Other, "another software id");
        // HID++ 2.0 error: unsupported function.
        assert_eq!(classify(&[0x11, 0x02, 0xff, 0x07, 0x1b, 0x07, 0, 0], 2, 7, 1), Answer::Error(0x07));
        // HID++ 1.0 error from the receiver: no device in slot 4.
        assert_eq!(classify(&[0x10, 0x04, 0x8f, 0x00, 0x0b, 0x09, 0x00], 4, 0, 0), Answer::Error(0x09));
    }

    #[test]
    fn reads_the_three_battery_features() {
        assert_eq!(parse_unified(&data(&[73, 0x04, 0x00])), Level::Single { percent: 73, charging: false });
        assert_eq!(parse_unified(&data(&[0, 0x02, 0x01])), Level::Single { percent: 20, charging: true }, "levels only");
        assert_eq!(parse_status(&data(&[50, 20, 0])), Level::Single { percent: 50, charging: false });
        assert_eq!(parse_status(&data(&[0, 0, 3])), Level::Single { percent: 100, charging: true }, "charged");
        // 3.90 V, charging.
        assert_eq!(parse_voltage(&data(&[0x0f, 0x3c, 0x80])), Level::Single { percent: 67, charging: true });
    }

    #[test]
    fn maps_voltage_along_the_curve() {
        assert_eq!(voltage_percent(4200), 100);
        assert_eq!(voltage_percent(3811), 50);
        assert_eq!(voltage_percent(3835), 55);
        assert_eq!(voltage_percent(3500), 0);
        assert_eq!(voltage_percent(3000), 0);
    }

    #[test]
    fn types_devices() {
        assert_eq!(kind_of_type(0), Some(DeviceKind::Keyboard));
        assert_eq!(kind_of_type(3), Some(DeviceKind::Mouse));
        assert_eq!(kind_of_type(8), Some(DeviceKind::Headset));
        assert_eq!(kind_of_type(7), None, "the receiver itself");
    }
}
