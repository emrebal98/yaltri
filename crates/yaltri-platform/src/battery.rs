//! Battery levels of wireless mice and earbuds that talk to the computer
//! through their own USB dongle. Windows and Linux only know the battery of
//! Bluetooth devices, so each supported dongle is asked in its own HID
//! protocol.
//!
//! - Mice on Compx/Kysona dongles (ATK, Pulsar, Kysona; vendor `373b`,
//!   `3554`): a 17 byte report `08 <cmd> 0… <checksum>`, where the checksum
//!   makes the bytes add up to `0x55`. Command `03` answers whether the
//!   mouse is on (`[6]`), `04` its battery (`[6]` percent, `[7]` charging).
//!   The protocol of Linux's `hid-kysona` driver.
//! - SteelSeries Arctis GameBuds: `00 b0` on the `0xffc0` interface answers
//!   each bud's state (`[3]`, `[4]`: 3 out of the case, 2 docked) and
//!   percent (`[5]`, `[6]`). From HeadsetControl.
//!
//! Linux needs read access to `/dev/hidraw*` (a udev rule) for this.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use hidapi::{HidApi, HidDevice};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceKind {
    Mouse,
    Earbuds,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 2] = [DeviceKind::Mouse, DeviceKind::Earbuds];

    /// Stable machine name (tray icon ids).
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Mouse => "mouse",
            DeviceKind::Earbuds => "earbuds",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// One battery.
    Single { percent: u8, charging: bool },
    /// Earbuds: each bud's percent, `None` while it is in the case (a docked
    /// bud is off and reports nothing useful).
    Pair { left: Option<u8>, right: Option<u8> },
}

impl Level {
    /// The level to show in one number: the lower bud of a pair. `None` when
    /// both buds are in the case.
    pub fn headline(self) -> Option<u8> {
        match self {
            Level::Single { percent, .. } => Some(percent),
            Level::Pair { left, right } => match (left, right) {
                (Some(l), Some(r)) => Some(l.min(r)),
                (l, r) => l.or(r),
            },
        }
    }

    pub fn charging(self) -> bool {
        matches!(self, Level::Single { charging: true, .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub kind: DeviceKind,
    /// Product name, as the dongle reports it.
    pub name: String,
    pub level: Level,
}

struct Driver {
    vendor: u16,
    /// `None`: every product of the vendor.
    product: Option<u16>,
    interface: i32,
    usage_page: u16,
    kind: DeviceKind,
    /// `Ok(None)`: the dongle is there but its device is off.
    read: fn(&HidDevice) -> Result<Option<Level>>,
}

const DRIVERS: &[Driver] = &[
    // ATK / Pulsar mice share Compx's dongle firmware.
    Driver { vendor: 0x373b, product: None, interface: 1, usage_page: 0xff02, kind: DeviceKind::Mouse, read: read_kysona },
    Driver { vendor: 0x3554, product: None, interface: 1, usage_page: 0xff02, kind: DeviceKind::Mouse, read: read_kysona },
    Driver {
        vendor: 0x1038,
        product: Some(0x230a),
        interface: 3,
        usage_page: 0xffc0,
        kind: DeviceKind::Earbuds,
        read: read_gamebuds,
    },
];

const TIMEOUT: Duration = Duration::from_millis(500);

/// Every supported device that is on, one per kind (the first found).
/// Devices that fail to answer are logged and skipped.
pub fn read_all() -> Vec<Reading> {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => {
            tracing::warn!("listing HID devices failed: {e}");
            return Vec::new();
        }
    };
    let mut readings: Vec<Reading> = Vec::new();
    for info in api.device_list() {
        let Some(driver) = DRIVERS.iter().find(|d| {
            d.vendor == info.vendor_id()
                && d.product.is_none_or(|p| p == info.product_id())
                && d.interface == info.interface_number()
                // Linux hidraw has one node per interface and no usage page.
                && (info.usage_page() == d.usage_page || info.usage_page() == 0)
        }) else {
            continue;
        };
        if readings.iter().any(|r| r.kind == driver.kind) {
            continue;
        }
        let name = info.product_string().unwrap_or("").trim().to_owned();
        let level = info.open_device(&api).context("opening").and_then(|dev| (driver.read)(&dev));
        match level {
            Ok(Some(level)) => readings.push(Reading { kind: driver.kind, name, level }),
            Ok(None) => {}
            Err(e) => tracing::debug!(device = name, "reading battery failed: {e:#}"),
        }
    }
    readings.sort_by_key(|r| r.kind);
    readings
}

/// Writes `query` and returns the first answer `accept` takes.
fn ask(dev: &HidDevice, query: &[u8], accept: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
    dev.write(query).context("writing")?;
    let mut buf = [0u8; 128];
    // Other reports (button presses) can come first.
    for _ in 0..8 {
        let n = dev.read_timeout(&mut buf, TIMEOUT.as_millis() as i32).context("reading")?;
        if n == 0 {
            break;
        }
        if accept(&buf[..n]) {
            return Ok(buf[..n].to_vec());
        }
    }
    bail!("no answer")
}

const KYSONA_REPORT: u8 = 0x08;
const KYSONA_ONLINE: u8 = 0x03;
const KYSONA_BATTERY: u8 = 0x04;

pub(crate) fn kysona_query(cmd: u8) -> [u8; 17] {
    let mut q = [0u8; 17];
    q[0] = KYSONA_REPORT;
    q[1] = cmd;
    let sum = q[..16].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    q[16] = 0x55u8.wrapping_sub(sum);
    q
}

fn is_kysona_answer(cmd: u8) -> impl Fn(&[u8]) -> bool {
    move |r| r.len() >= 8 && r[0] == KYSONA_REPORT && r[1] == cmd
}

pub(crate) fn parse_kysona_online(r: &[u8]) -> bool {
    r.get(6).is_some_and(|b| *b != 0)
}

pub(crate) fn parse_kysona_battery(r: &[u8]) -> Option<Level> {
    Some(Level::Single { percent: (*r.get(6)?).min(100), charging: *r.get(7)? != 0 })
}

fn read_kysona(dev: &HidDevice) -> Result<Option<Level>> {
    let online = ask(dev, &kysona_query(KYSONA_ONLINE), is_kysona_answer(KYSONA_ONLINE))?;
    if !parse_kysona_online(&online) {
        return Ok(None);
    }
    let battery = ask(dev, &kysona_query(KYSONA_BATTERY), is_kysona_answer(KYSONA_BATTERY))?;
    parse_kysona_battery(&battery).context("short answer").map(Some)
}

const GAMEBUDS_STATUS: u8 = 0xb0;
const BUD_OUT: u8 = 0x03;

pub(crate) fn parse_gamebuds(r: &[u8]) -> Option<Level> {
    if r.len() < 7 || r[0] != GAMEBUDS_STATUS {
        return None;
    }
    let bud = |state: u8, percent: u8| (state == BUD_OUT).then_some(percent.min(100));
    Some(Level::Pair { left: bud(r[3], r[5]), right: bud(r[4], r[6]) })
}

fn read_gamebuds(dev: &HidDevice) -> Result<Option<Level>> {
    let mut query = [0u8; 65];
    query[1] = GAMEBUDS_STATUS;
    let answer = ask(dev, &query, |r| r.first() == Some(&GAMEBUDS_STATUS))?;
    let level = parse_gamebuds(&answer).context("short answer")?;
    // Both buds in the case: nothing to show.
    Ok(level.headline().map(|_| level))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kysona_queries_add_up_to_0x55() {
        let q = kysona_query(KYSONA_BATTERY);
        assert_eq!(&q[..2], &[0x08, 0x04]);
        assert_eq!(q[16], 0x49);
        assert_eq!(q.iter().fold(0u8, |a, b| a.wrapping_add(*b)), 0x55);
    }

    /// Answers an ATK A9 Ultra gave while charging at 35%.
    #[test]
    fn reads_an_atk_mouse() {
        let online = [0x08, 0x03, 0, 0, 0, 0x01, 0x01, 0x65, 0x40, 0x5b, 0, 0, 0, 0, 0, 0, 0x48];
        let battery = [0x08, 0x04, 0, 0, 0, 0x02, 0x23, 0x01, 0x0e, 0xb2, 0, 0, 0, 0, 0, 0, 0x63];
        assert!(is_kysona_answer(KYSONA_ONLINE)(&online));
        assert!(!is_kysona_answer(KYSONA_BATTERY)(&online));
        assert!(parse_kysona_online(&online));
        assert_eq!(parse_kysona_battery(&battery), Some(Level::Single { percent: 35, charging: true }));
    }

    /// Answer of Arctis GameBuds with both buds out at 60% and 74%.
    #[test]
    fn reads_gamebuds_per_bud() {
        let mut r = vec![0xb0, 0x01, 0x01, 0x03, 0x03, 0x3c, 0x4a, 0x0a, 0x03];
        assert_eq!(parse_gamebuds(&r), Some(Level::Pair { left: Some(60), right: Some(74) }));
        assert_eq!(parse_gamebuds(&r).unwrap().headline(), Some(60));
        r[3] = 0x02;
        assert_eq!(parse_gamebuds(&r), Some(Level::Pair { left: None, right: Some(74) }));
        r[4] = 0x02;
        assert_eq!(parse_gamebuds(&r).unwrap().headline(), None);
        assert_eq!(parse_gamebuds(&[0xb1, 0, 0, 3, 3, 1, 1]), None);
    }
}
