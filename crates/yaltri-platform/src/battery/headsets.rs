//! Wireless headsets on their own USB dongle, in the protocols HeadsetControl
//! (github.com/Sapd/HeadsetControl) worked out. Only families that answer a
//! plain request are here; ones that need a session (Corsair Bragi, Logitech
//! Centurion, Razer) or only talk when spoken to by a button (Sony) are not.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use hidapi::HidDevice;

use super::{DeviceKind, Driver, Level, ask_within};

const HEADSET: DeviceKind = DeviceKind::Headset;
/// Headsets wake their radio slowly; HeadsetControl waits up to 5 s.
const WAIT: Duration = Duration::from_millis(1500);

/// `c + (clamp(x, a, b) - a) * (d - c) / (b - a)`, truncating, as
/// HeadsetControl maps raw levels.
fn map(x: u8, a: u8, b: u8) -> u8 {
    let x = x.clamp(a, b);
    ((u32::from(x - a) * 100) / u32::from(b - a)) as u8
}

fn single(percent: u8, charging: bool) -> Option<Level> {
    Some(Level::Single { percent: percent.min(100), charging })
}

pub(super) const DRIVERS: &[Driver] = &[
    // SteelSeries Arctis 1, 1 Xbox, 7X, 7P.
    Driver {
        vendor: 0x1038,
        products: &[0x12b3, 0x12b6, 0x12d7, 0x12d5],
        usage_page: Some(0xff43),
        usage: Some(0x0202),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| Ok(parse_arctis_1(&ask_within(dev, &[0x06, 0x12], WAIT, |r| r.len() >= 4)?)),
    },
    // SteelSeries Arctis 7, 7 (2019), Pro (2019), Pro GameDAC.
    Driver {
        vendor: 0x1038,
        products: &[0x1260, 0x12ad, 0x1252, 0x1280],
        usage_page: Some(0xff43),
        usage: Some(0x0202),
        interface: Some(5),
        kind: HEADSET,
        read: |dev, _| Ok(parse_arctis_7(&ask_within(dev, &[0x06, 0x18], WAIT, |r| r.len() >= 3)?)),
    },
    // SteelSeries Arctis 9.
    Driver {
        vendor: 0x1038,
        products: &[0x12c2, 0x12c4],
        usage_page: None,
        usage: None,
        interface: None,
        kind: HEADSET,
        read: |dev, _| Ok(parse_arctis_9(&ask_within(dev, &[0x00, 0x20], WAIT, |r| r.len() >= 5)?)),
    },
    // SteelSeries Arctis Pro Wireless.
    Driver {
        vendor: 0x1038,
        products: &[0x1290],
        usage_page: None,
        usage: None,
        interface: None,
        kind: HEADSET,
        read: read_arctis_pro_wireless,
    },
    // SteelSeries Arctis 7+, Nova 7 / 7P, Nova 3P / 3X, Nova 5 / 5X.
    Driver {
        vendor: 0x1038,
        products: &[
            0x220e, 0x2212, 0x2216, 0x2236, 0x2202, 0x2206, 0x220a, 0x223a, 0x227a, 0x22a4, 0x22a1, 0x227e, 0x2258,
            0x229e, 0x22ad, 0x22a9, 0x22a5, 0x22a7, 0x2298, 0x2269, 0x226d, 0x2232, 0x2253,
        ],
        usage_page: Some(0xffc0),
        usage: Some(0x0001),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, product| Ok(parse_nova(product, &ask_within(dev, &[0x00, 0xb0], WAIT, |r| r.len() >= 5)?)),
    },
    // SteelSeries Arctis Nova Pro Wireless (base station).
    Driver {
        vendor: 0x1038,
        products: &[0x12e0, 0x12e5],
        usage_page: None,
        usage: None,
        interface: Some(4),
        kind: HEADSET,
        read: |dev, _| {
            let mut q = [0u8; 31];
            q[..2].copy_from_slice(&[0x06, 0xb0]);
            Ok(parse_nova_pro(&ask_within(dev, &q, WAIT, |r| r.len() >= 16)?))
        },
    },
    // Corsair Void family and other older Corsair wireless headsets.
    Driver {
        vendor: 0x1b1c,
        products: &[
            0x1b27, 0x0a14, 0x0a16, 0x0a17, 0x0a1d, 0x0a1a, 0x1b2a, 0x1b23, 0x1b29, 0x0a55, 0x0a51, 0x0a52, 0x0a38,
            0x0a4f, 0x0a2b, 0x0a75, 0x0a56, 0x1b1c,
        ],
        usage_page: Some(0xffc5),
        usage: Some(0x0001),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| parse_corsair_void(&ask_within(dev, &[0xc9, 0x64], WAIT, |r| r.len() >= 5 && r[0] == 100)?),
    },
    // HyperX Cloud II Wireless (HP).
    Driver {
        vendor: 0x03f0,
        products: &[0x0696, 0x018b],
        usage_page: Some(0xff90),
        usage: Some(0x0303),
        interface: Some(0),
        kind: HEADSET,
        read: read_cloud_2,
    },
    // HyperX Cloud Flight Wireless.
    Driver {
        vendor: 0x0951,
        products: &[0x16c4, 0x1723],
        usage_page: None,
        usage: None,
        interface: None,
        kind: HEADSET,
        read: |dev, _| {
            let mut q = [0u8; 20];
            q[..3].copy_from_slice(&[0x21, 0xff, 0x05]);
            Ok(parse_cloud_flight(&ask_within(dev, &q, WAIT, |r| r.len() == 15 || r.len() == 20)?))
        },
    },
    // HyperX Cloud Alpha Wireless.
    Driver {
        vendor: 0x03f0,
        products: &[0x098d],
        usage_page: None,
        usage: None,
        interface: None,
        kind: HEADSET,
        read: read_cloud_alpha,
    },
    // Logitech HID++ headsets with fixed feature indexes and a voltage table.
    Driver {
        vendor: 0x046d,
        products: &[0x0a66],
        usage_page: Some(0xff43),
        usage: Some(0x0202),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_logitech_voltage(dev, [0x07, 0x01], &G533),
    },
    Driver {
        vendor: 0x046d,
        products: &[0x0ac4],
        usage_page: Some(0x000c),
        usage: Some(0x0001),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_logitech_voltage(dev, [0x05, 0x0d], &G535),
    },
    Driver {
        vendor: 0x046d,
        products: &[0x0a5c, 0x0a89, 0x0a5b, 0x0a87, 0x0ab5, 0x0afe, 0x0b1f],
        usage_page: Some(0xff43),
        usage: Some(0x0202),
        interface: None,
        kind: HEADSET,
        read: |dev, _| read_logitech_voltage(dev, [0x08, 0x0a], &G633),
    },
    Driver {
        vendor: 0x046d,
        products: &[0x0aa7, 0x0aaa, 0x0aba, 0x0afb, 0x0afc],
        usage_page: Some(0xff43),
        usage: Some(0x0202),
        interface: None,
        kind: HEADSET,
        read: |dev, _| read_logitech_voltage(dev, [0x06, 0x0d], &G_PRO),
    },
    // Jabra (GN protocol): Link 380 / 390 dongles and Evolve2 65 Flex,
    // Evolve3 75 over USB. The headset's address differs.
    Driver {
        vendor: 0x0b0e,
        products: &[0x24c7, 0x24c8, 0x24c9, 0x24ca],
        usage_page: Some(0xff00),
        usage: Some(0x0001),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_jabra(dev, 0x04),
    },
    Driver {
        vendor: 0x0b0e,
        products: &[0x2e50, 0x2e51, 0x2e56, 0x2e57],
        usage_page: Some(0xff00),
        usage: Some(0x0005),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_jabra(dev, 0x04),
    },
    Driver {
        vendor: 0x0b0e,
        products: &[0x2517, 0x2518, 0x2519],
        usage_page: Some(0xff00),
        usage: Some(0x0001),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_jabra(dev, 0x08),
    },
    Driver {
        vendor: 0x0b0e,
        products: &[0x2546, 0x2547, 0x2548],
        usage_page: Some(0xff00),
        usage: Some(0x0005),
        interface: Some(3),
        kind: HEADSET,
        read: |dev, _| read_jabra(dev, 0x08),
    },
    // FIFINE HT6 Wireless.
    Driver {
        vendor: 0x3142,
        products: &[0x00c3],
        usage_page: Some(0xff90),
        usage: Some(0x0001),
        interface: Some(0),
        kind: HEADSET,
        read: |dev, _| {
            let accept = |r: &[u8]| r.len() >= 4 && r[0] == 0x55 && (r[1] == 0x65 || r[1] == 0x0a || r[3] == 0x1a);
            Ok(parse_fifine(&ask_within(dev, &report_55(), WAIT, accept)?))
        },
    },
    // MCHOSE X9 and V9 Pro Wireless.
    Driver {
        vendor: 0x3837,
        products: &[0x6045],
        usage_page: Some(0xff90),
        usage: Some(0x0001),
        interface: Some(0),
        kind: HEADSET,
        read: read_mchose,
    },
    Driver {
        vendor: 0x291d,
        products: &[0x385d],
        usage_page: Some(0xff90),
        usage: Some(0x0001),
        interface: Some(0),
        kind: HEADSET,
        read: read_mchose,
    },
    // Lenovo Wireless VoIP Headset.
    Driver {
        vendor: 0x17ef,
        products: &[0xa07d],
        usage_page: Some(0xff07),
        usage: Some(0x0222),
        interface: Some(3),
        kind: HEADSET,
        read: read_lenovo,
    },
];

/// Arctis 1 / 7X / 7P: `[2]` 1 = off, `[3]` percent. Never says charging.
pub(crate) fn parse_arctis_1(r: &[u8]) -> Option<Level> {
    if r[2] == 0x01 {
        return None;
    }
    single(map(r[3], 0, 100), false)
}

/// Arctis 7 / Pro: `[2]` percent.
pub(crate) fn parse_arctis_7(r: &[u8]) -> Option<Level> {
    single(map(r[2], 0, 100), false)
}

/// Arctis 9: `[4]` 0xFF = off, 1 = charging; `[3]` raw 0x64–0x9A.
pub(crate) fn parse_arctis_9(r: &[u8]) -> Option<Level> {
    if r[4] == 0xff {
        return None;
    }
    single(map(r[3], 0x64, 0x9a), r[4] == 0x01)
}

fn read_arctis_pro_wireless(dev: &HidDevice, _: u16) -> Result<Option<Level>> {
    let mut q = [0u8; 31];
    q[..2].copy_from_slice(&[0x41, 0xaa]);
    if ask_within(dev, &q, WAIT, |r| !r.is_empty())?[0] == 0x02 {
        return Ok(None);
    }
    q[0] = 0x40;
    let level = ask_within(dev, &q, WAIT, |r| !r.is_empty())?[0];
    Ok(single(map(level, 0, 4), false))
}

/// Nova 7 models that report 0–4 instead of a percent.
const NOVA_DISCRETE: &[u16] = &[0x2202, 0x2206, 0x220a, 0x223a, 0x227a, 0x22a4];

/// The `00 b0` status of the Nova family, by model.
pub(crate) fn parse_nova(product: u16, r: &[u8]) -> Option<Level> {
    match product {
        // Arctis 7+.
        0x220e | 0x2212 | 0x2216 | 0x2236 => (r[1] != 0x01).then(|| single(map(r[2], 0, 4), r[3] == 0x01))?,
        // Nova 3P / 3X, Nova 5 / 5X.
        0x2269 | 0x226d | 0x2232 | 0x2253 => (r[1] != 0x02).then(|| single(r[3], r[4] == 0x01))?,
        // Nova 7 / 7P.
        _ => {
            if r[3] == 0x00 {
                return None;
            }
            let percent = if NOVA_DISCRETE.contains(&product) { map(r[2], 0, 4) } else { r[2] };
            single(percent, matches!(r[3], 0x01 | 0x02))
        }
    }
}

/// Nova Pro Wireless: `[15]` 1 = off, 2 = charging; `[6]` 0–8.
pub(crate) fn parse_nova_pro(r: &[u8]) -> Option<Level> {
    if r[15] == 0x01 {
        return None;
    }
    single(map(r[6], 0, 8), r[15] == 0x02)
}

/// Corsair Void: `[4]` 0 = off, 1–2 on battery, 4–5 charging; `[2]` percent
/// with the mic boom in bit 7.
pub(crate) fn parse_corsair_void(r: &[u8]) -> Result<Option<Level>> {
    let charging = match r[4] {
        0 => return Ok(None),
        1 | 2 => false,
        4 | 5 => true,
        other => bail!("unknown Corsair status {other}"),
    };
    Ok(single(r[2] & 0x7f, charging))
}

fn read_cloud_2(dev: &HidDevice, _: u16) -> Result<Option<Level>> {
    let cmd = |c: u8| -> Result<Vec<u8>> {
        let mut q = [0u8; 52];
        q[..4].copy_from_slice(&[0x06, 0xff, 0xbb, c]);
        ask_within(dev, &q, WAIT, |r| r.len() >= 8 && r[..4] == [0x06, 0xff, 0xbb, c])
    };
    let level = cmd(0x02)?[7];
    let charging = cmd(0x03).is_ok_and(|r| r[4] == 1);
    Ok(single(level, charging))
}

/// Cloud Flight: millivolts in `[3..5]`; above 4107 mV it is charging.
pub(crate) fn parse_cloud_flight(r: &[u8]) -> Option<Level> {
    let mv = f64::from(u16::from_be_bytes([r[3], r[4]]));
    let percent = if mv <= 3648. {
        0.00125 * mv
    } else if mv > 3975. {
        100.
    } else {
        2.547505e-8 * mv.powi(4) - 3.900299e-4 * mv.powi(3) + 2.238321 * mv.powi(2) - 5706.256 * mv + 5452299.0
    };
    single(percent.round().clamp(0., 100.) as u8, mv > 4107.)
}

fn read_cloud_alpha(dev: &HidDevice, _: u16) -> Result<Option<Level>> {
    let cmd = |c: u8| -> Result<u8> {
        let mut q = [0u8; 31];
        q[..3].copy_from_slice(&[0x21, 0xbb, c]);
        Ok(ask_within(dev, &q, WAIT, |r| r.len() >= 4)?[3])
    };
    if cmd(0x03)? == 0x01 {
        return Ok(None);
    }
    let charging = cmd(0x0c)? == 0x01;
    Ok(single(cmd(0x0b)?, charging))
}

/// Logitech headset voltage curves, `(millivolts, percent)` from full.
const G533: [(u16, u8); 6] = [(4200, 100), (3850, 50), (3790, 30), (3750, 20), (3680, 5), (3330, 0)];
const G535: [(u16, u8); 6] = [(4175, 100), (3817, 50), (3766, 30), (3730, 20), (3664, 5), (3310, 0)];
const G633: [(u16, u8); 8] =
    [(4100, 100), (3950, 80), (3850, 60), (3750, 40), (3650, 20), (3500, 10), (3300, 5), (3150, 0)];
const G_PRO: [(u16, u8); 6] = [(4150, 100), (3830, 50), (3780, 30), (3740, 20), (3670, 5), (3320, 0)];

/// HeadsetControl's `spline_battery_level`: linear between points,
/// truncating.
pub(crate) fn voltage_percent(mv: u16, curve: &[(u16, u8)]) -> u8 {
    for (i, &(v, p)) in curve.iter().enumerate() {
        if mv >= v {
            return p;
        }
        if let Some(&(v_lo, p_lo)) = curve.get(i + 1)
            && mv >= v_lo
        {
            return p_lo + (f32::from(mv - v_lo) / f32::from(v - v_lo) * f32::from(p - p_lo)) as u8;
        }
    }
    0
}

/// HID++ long report to device 0xFF; the answer is `11 FF <feature> <fn> <mV
/// big endian> <state>`, `[2]` 0xFF for an error (off), state 3 charging.
pub(crate) fn parse_logitech_voltage(r: &[u8], curve: &[(u16, u8)]) -> Option<Level> {
    if r[2] == 0xff {
        return None;
    }
    let mv = u16::from_be_bytes([r[4], r[5]]);
    // Below the lowest point the reading is not trusted.
    if mv < curve.last()?.0 {
        return None;
    }
    single(voltage_percent(mv, curve), r[6] == 0x03)
}

fn read_logitech_voltage(dev: &HidDevice, feature: [u8; 2], curve: &[(u16, u8)]) -> Result<Option<Level>> {
    let mut q = [0u8; 20];
    q[..4].copy_from_slice(&[0x11, 0xff, feature[0], feature[1]]);
    let accept = |r: &[u8]| r.len() >= 7 && r[0] == 0x11 && r[1] == 0xff && (r[2] == feature[0] || r[2] == 0xff);
    Ok(parse_logitech_voltage(&ask_within(dev, &q, WAIT, accept)?, curve))
}

/// Jabra GN protocol: GET battery (`12 02`) to `address`, sequence 1.
pub(crate) fn jabra_request(address: u8) -> [u8; 64] {
    let mut q = [0u8; 64];
    q[..7].copy_from_slice(&[0x05, address, 0x00, 0x01, 0x46, 0x12, 0x02]);
    q
}

/// A response (`[4]` top bits 3) to sequence 1: `[7]` flags (bit 0
/// charging), `[8]` percent; `[5]` 0xFE is a refusal, F3–F5 meaning off.
pub(crate) fn parse_jabra(r: &[u8]) -> Result<Option<Level>> {
    if r[5] == 0xfe {
        return match r[6] {
            0xf3..=0xf5 => Ok(None),
            code => bail!("Jabra refused: {code:#04x}"),
        };
    }
    if (r[4] & 0x3f) < 8 {
        bail!("short Jabra answer");
    }
    Ok(single(r[8], r[7] & 0x01 != 0))
}

fn read_jabra(dev: &HidDevice, address: u8) -> Result<Option<Level>> {
    let accept = |r: &[u8]| r.len() >= 9 && r[0] == 0x05 && r[4] >> 6 == 3 && r[3] == 0x01;
    parse_jabra(&ask_within(dev, &jabra_request(address), WAIT, accept)?)
}

fn report_55() -> [u8; 64] {
    let mut q = [0u8; 64];
    q[..3].copy_from_slice(&[0x55, 0x65, 0x01]);
    q
}

/// FIFINE HT6: `55 0A …` or `[3]` 0x1A = off; `[2]` percent, `[3]` 3 charging.
pub(crate) fn parse_fifine(r: &[u8]) -> Option<Level> {
    if r[1] == 0x0a || r[3] == 0x1a {
        return None;
    }
    single(r[2], r[3] == 0x03)
}

fn read_mchose(dev: &HidDevice, _: u16) -> Result<Option<Level>> {
    let accept = |r: &[u8]| r.len() >= 4 && r[0] == 0x55 && r[1] == 0x65 && r[3] == 0x02;
    Ok(single(ask_within(dev, &report_55(), WAIT, accept)?[2], false))
}

/// Lenovo: feature report `24 01`, answer `27 01 <off> … [7] percent`.
fn read_lenovo(dev: &HidDevice, _: u16) -> Result<Option<Level>> {
    let mut q = [0u8; 61];
    q[..2].copy_from_slice(&[0x24, 0x01]);
    dev.send_feature_report(&q).context("feature report")?;
    let mut r = [0u8; 61];
    let n = dev.read_timeout(&mut r, WAIT.as_millis() as i32)?;
    if n != 61 || r[0] != 0x27 || r[1] != 0x01 {
        bail!("no Lenovo answer");
    }
    Ok(if r[2] != 0 { None } else { single(r[7], false) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pct(level: Option<Level>) -> Option<(u8, bool)> {
        match level? {
            Level::Single { percent, charging } => Some((percent, charging)),
            Level::Pair { .. } => None,
        }
    }

    #[test]
    fn maps_like_headsetcontrol() {
        assert_eq!(map(2, 0, 4), 50);
        assert_eq!(map(3, 0, 8), 37, "truncates");
        assert_eq!(map(0x7f, 0x64, 0x9a), 50);
        assert_eq!(map(0x50, 0x64, 0x9a), 0, "clamped");
    }

    #[test]
    fn reads_steelseries() {
        assert_eq!(pct(parse_arctis_1(&[0, 0, 0x02, 64])), Some((64, false)));
        assert_eq!(pct(parse_arctis_1(&[0, 0, 0x01, 64])), None, "off");
        assert_eq!(pct(parse_arctis_9(&[0, 0, 0, 0x9a, 0x01])), Some((100, true)));
        assert_eq!(pct(parse_arctis_9(&[0, 0, 0, 0x9a, 0xff])), None);
        // Arctis 7+: 3 of 4, charging.
        assert_eq!(pct(parse_nova(0x220e, &[0xb0, 0x03, 3, 0x01, 0])), Some((75, true)));
        // Nova 7 discrete vs percent models.
        assert_eq!(pct(parse_nova(0x2202, &[0xb0, 0, 2, 0x02, 0])), Some((50, true)));
        assert_eq!(pct(parse_nova(0x2258, &[0xb0, 0, 64, 0x03, 0])), Some((64, false)));
        assert_eq!(pct(parse_nova(0x2258, &[0xb0, 0, 64, 0x00, 0])), None, "off");
        // Nova 5: off when [1] is 2.
        assert_eq!(pct(parse_nova(0x2232, &[0xb0, 0x03, 0, 81, 0x01])), Some((81, true)));
        assert_eq!(pct(parse_nova(0x2232, &[0xb0, 0x02, 0, 81, 0x01])), None);
        let mut pro = [0u8; 16];
        pro[6] = 6;
        pro[15] = 0x02;
        assert_eq!(pct(parse_nova_pro(&pro)), Some((75, true)));
    }

    #[test]
    fn reads_corsair_and_hyperx() {
        assert_eq!(pct(parse_corsair_void(&[100, 0, 0x80 | 42, 177, 1]).unwrap()), Some((42, false)), "mic bit");
        assert_eq!(pct(parse_corsair_void(&[100, 0, 42, 177, 5]).unwrap()), Some((42, true)));
        assert_eq!(pct(parse_corsair_void(&[100, 0, 42, 177, 0]).unwrap()), None);
        assert!(parse_corsair_void(&[100, 0, 42, 177, 9]).is_err());
        // 3.80 V → about 50%, 4.2 V charging.
        let flight = |mv: u16| {
            let [hi, lo] = mv.to_be_bytes();
            pct(parse_cloud_flight(&[0, 0, 0, hi, lo]))
        };
        assert_eq!(flight(4200), Some((100, true)));
        assert_eq!(flight(3600), Some((5, false)));
        let mid = flight(3800).unwrap().0;
        assert!((30..=70).contains(&mid), "{mid}");
    }

    #[test]
    fn reads_logitech_headset_voltage() {
        assert_eq!(voltage_percent(4200, &G533), 100);
        // Halfway between (3850, 50) and (3790, 30): 40.
        assert_eq!(voltage_percent(3820, &G533), 40);
        assert_eq!(voltage_percent(3200, &G633), 1, "truncates");
        let answer = [0x11, 0xff, 0x08, 0x0a, 0x0f, 0x3c, 0x03];
        // 3.90 V, halfway between (3950, 80) and (3850, 60).
        assert_eq!(pct(parse_logitech_voltage(&answer, &G633)), Some((70, true)));
        assert_eq!(pct(parse_logitech_voltage(&[0x11, 0xff, 0xff, 0x08, 0, 0, 0], &G633)), None, "off");
        assert_eq!(pct(parse_logitech_voltage(&[0x11, 0xff, 0x08, 0x0a, 0x0c, 0x00, 1], &G633)), None, "too low");
    }

    #[test]
    fn reads_jabra_and_report_55() {
        assert_eq!(&jabra_request(0x04)[..7], &[0x05, 0x04, 0x00, 0x01, 0x46, 0x12, 0x02]);
        // Response, length 8 (6 + 2 payload bytes), charging at 77%.
        let ok = [0x05, 0x00, 0x04, 0x01, 0xc8, 0x12, 0x02, 0x01, 77];
        assert_eq!(pct(parse_jabra(&ok).unwrap()), Some((77, true)));
        let off = [0x05, 0x00, 0x04, 0x01, 0xc8, 0xfe, 0xf4, 0, 0];
        assert_eq!(pct(parse_jabra(&off).unwrap()), None);
        assert_eq!(pct(parse_fifine(&[0x55, 0x65, 50, 0x03])), Some((50, true)));
        assert_eq!(pct(parse_fifine(&[0x55, 0x0a, 0, 0])), None);
    }
}
