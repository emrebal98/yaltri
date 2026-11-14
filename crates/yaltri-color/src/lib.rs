//! Color math for the picker: conversions, text formats, shades and the
//! picked-color history. No UI or platform code.

use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#RRGGBB`, `RRGGBB` or `#RGB`.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let hex = |i: usize, n: usize| u8::from_str_radix(s.get(i..i + n)?, 16).ok();
        match s.len() {
            6 => Some(Self::new(hex(0, 2)?, hex(2, 2)?, hex(4, 2)?)),
            3 => {
                let d = |i| hex(i, 1).map(|v| v * 17);
                Some(Self::new(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    fn unit(self) -> (f32, f32, f32) {
        (self.r as f32 / 255., self.g as f32 / 255., self.b as f32 / 255.)
    }

    /// Hue in degrees 0..360, saturation and lightness 0..1.
    pub fn to_hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = self.unit();
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.;
        let d = max - min;
        if d == 0. {
            return (0., 0., l);
        }
        let s = d / (1. - (2. * l - 1.).abs());
        (hue(r, g, b, max, d), s, l)
    }

    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        let s = s.clamp(0., 1.);
        let l = l.clamp(0., 1.);
        let c = (1. - (2. * l - 1.).abs()) * s;
        Self::from_chroma(h, c, l - c / 2.)
    }

    /// Hue in degrees, saturation and value 0..1.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let (r, g, b) = self.unit();
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        let h = if d == 0. { 0. } else { hue(r, g, b, max, d) };
        let s = if max == 0. { 0. } else { d / max };
        (h, s, max)
    }

    /// Cyan, magenta, yellow, key as 0..1.
    pub fn to_cmyk(self) -> (f32, f32, f32, f32) {
        let (r, g, b) = self.unit();
        let k = 1. - r.max(g).max(b);
        if k >= 1. {
            return (0., 0., 0., 1.);
        }
        ((1. - r - k) / (1. - k), (1. - g - k) / (1. - k), (1. - b - k) / (1. - k), k)
    }

    /// Hue in degrees, whiteness and blackness 0..1.
    pub fn to_hwb(self) -> (f32, f32, f32) {
        let (h, s, v) = self.to_hsv();
        (h, (1. - s) * v, 1. - v)
    }

    fn from_chroma(h: f32, c: f32, m: f32) -> Self {
        let h = h.rem_euclid(360.) / 60.;
        let x = c * (1. - (h % 2. - 1.).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.),
            1 => (x, c, 0.),
            2 => (0., c, x),
            3 => (0., x, c),
            4 => (x, 0., c),
            _ => (c, 0., x),
        };
        let to = |v: f32| ((v + m) * 255.).round().clamp(0., 255.) as u8;
        Self::new(to(r), to(g), to(b))
    }

    /// Relative luminance (WCAG), used to pick readable text over a swatch.
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = c as f32 / 255.;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn is_dark(self) -> bool {
        self.luminance() < 0.4
    }

    /// `n` lighter shades (closest first) and `n` darker shades. Steps are
    /// a share of the remaining distance to white/black, so even near-black
    /// or near-white colors get distinct shades.
    pub fn shades(self, n: usize) -> (Vec<Rgb>, Vec<Rgb>) {
        let (h, s, l) = self.to_hsl();
        let step = 1. / (n as f32 + 1.5);
        let lighter = (1..=n).map(|i| Rgb::from_hsl(h, s, l + (1. - l) * step * i as f32)).collect();
        let darker = (1..=n).map(|i| Rgb::from_hsl(h, s, l - l * step * i as f32)).collect();
        (lighter, darker)
    }
}

fn hue(r: f32, g: f32, b: f32, max: f32, d: f32) -> f32 {
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    h * 60.
}

/// Text representations offered for copying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Hex,
    Rgb,
    Hsl,
    Hsv,
    Cmyk,
    Hwb,
    Decimal,
    Float,
}

impl Format {
    pub const ALL: [Format; 8] =
        [Format::Hex, Format::Rgb, Format::Hsl, Format::Hsv, Format::Cmyk, Format::Hwb, Format::Decimal, Format::Float];

    pub fn label(self) -> &'static str {
        match self {
            Format::Hex => "HEX",
            Format::Rgb => "RGB",
            Format::Hsl => "HSL",
            Format::Hsv => "HSV",
            Format::Cmyk => "CMYK",
            Format::Hwb => "HWB",
            Format::Decimal => "Decimal",
            Format::Float => "Float",
        }
    }

    /// Stable name used in settings.
    pub fn key(self) -> &'static str {
        match self {
            Format::Hex => "hex",
            Format::Rgb => "rgb",
            Format::Hsl => "hsl",
            Format::Hsv => "hsv",
            Format::Cmyk => "cmyk",
            Format::Hwb => "hwb",
            Format::Decimal => "decimal",
            Format::Float => "float",
        }
    }

    pub fn format(self, c: Rgb) -> String {
        let pct = |v: f32| (v * 100.).round() as i32;
        let deg = |v: f32| v.round() as i32 % 360;
        match self {
            Format::Hex => c.to_hex(),
            Format::Rgb => format!("rgb({}, {}, {})", c.r, c.g, c.b),
            Format::Hsl => {
                let (h, s, l) = c.to_hsl();
                format!("hsl({}, {}%, {}%)", deg(h), pct(s), pct(l))
            }
            Format::Hsv => {
                let (h, s, v) = c.to_hsv();
                format!("hsv({}, {}%, {}%)", deg(h), pct(s), pct(v))
            }
            Format::Cmyk => {
                let (cy, m, y, k) = c.to_cmyk();
                format!("cmyk({}%, {}%, {}%, {}%)", pct(cy), pct(m), pct(y), pct(k))
            }
            Format::Hwb => {
                let (h, w, b) = c.to_hwb();
                format!("hwb({}, {}%, {}%)", deg(h), pct(w), pct(b))
            }
            Format::Decimal => ((c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32).to_string(),
            Format::Float => {
                let (r, g, b) = c.unit();
                format!("({r:.3}, {g:.3}, {b:.3})")
            }
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Format::ALL
            .into_iter()
            .find(|f| f.key().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown color format: {s}"))
    }
}

/// Picked colors, most recent first, persisted as hex strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorHistory {
    colors: Vec<Rgb>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    colors: Vec<String>,
}

impl ColorHistory {
    pub const MAX: usize = 30;

    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<OnDisk>(&text) {
            Ok(d) => Self { colors: d.colors.iter().filter_map(|h| Rgb::from_hex(h)).take(Self::MAX).collect() },
            Err(e) => {
                tracing::warn!(%e, "color history unreadable, starting fresh");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let disk = OnDisk { colors: self.colors.iter().map(|c| c.to_hex()).collect() };
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string(&disk).context("serializing colors")?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Adds `c` at the front (moving it if already present).
    pub fn push(&mut self, c: Rgb) {
        self.colors.retain(|x| *x != c);
        self.colors.insert(0, c);
        self.colors.truncate(Self::MAX);
    }

    pub fn remove(&mut self, c: Rgb) {
        self.colors.retain(|x| *x != c);
    }

    pub fn colors(&self) -> &[Rgb] {
        &self.colors
    }

    pub fn latest(&self) -> Option<Rgb> {
        self.colors.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DODGER: Rgb = Rgb::new(30, 144, 255);

    #[test]
    fn hex_round_trip() {
        assert_eq!(DODGER.to_hex(), "#1E90FF");
        assert_eq!(Rgb::from_hex("#1e90ff"), Some(DODGER));
        assert_eq!(Rgb::from_hex("1E90FF"), Some(DODGER));
        assert_eq!(Rgb::from_hex("#fff"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::from_hex("#12345"), None);
        assert_eq!(Rgb::from_hex("zzzzzz"), None);
    }

    #[test]
    fn formats_match_reference_values() {
        assert_eq!(Format::Rgb.format(DODGER), "rgb(30, 144, 255)");
        assert_eq!(Format::Hsl.format(DODGER), "hsl(210, 100%, 56%)");
        assert_eq!(Format::Hsv.format(DODGER), "hsv(210, 88%, 100%)");
        assert_eq!(Format::Cmyk.format(DODGER), "cmyk(88%, 44%, 0%, 0%)");
        assert_eq!(Format::Hwb.format(DODGER), "hwb(210, 12%, 0%)");
        assert_eq!(Format::Decimal.format(DODGER), "2003199");
        assert_eq!(Format::Float.format(DODGER), "(0.118, 0.565, 1.000)");
        assert_eq!(Format::Cmyk.format(Rgb::new(0, 0, 0)), "cmyk(0%, 0%, 0%, 100%)");
    }

    #[test]
    fn hsl_round_trips_all_hues() {
        for c in [DODGER, Rgb::new(255, 0, 0), Rgb::new(12, 200, 90), Rgb::new(128, 128, 128), Rgb::new(250, 240, 10)] {
            let (h, s, l) = c.to_hsl();
            let back = Rgb::from_hsl(h, s, l);
            for (a, b) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                assert!(a.abs_diff(b) <= 1, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn shades_get_lighter_and_darker() {
        let (light, dark) = DODGER.shades(2);
        let l = |c: Rgb| c.to_hsl().2;
        assert!(l(light[0]) > l(DODGER) && l(light[1]) > l(light[0]));
        assert!(l(dark[0]) < l(DODGER) && l(dark[1]) < l(dark[0]));
    }

    #[test]
    fn shades_are_distinct_even_near_black_and_white() {
        for c in [Rgb::new(12, 12, 12), Rgb::new(245, 245, 245)] {
            let (light, dark) = c.shades(2);
            let all: std::collections::HashSet<Rgb> = light.iter().chain(&dark).copied().chain([c]).collect();
            assert_eq!(all.len(), 5, "{c:?}: {light:?} {dark:?}");
        }
    }

    #[test]
    fn format_keys_parse() {
        for f in Format::ALL {
            assert_eq!(f.key().parse::<Format>().unwrap(), f);
        }
        assert!("nope".parse::<Format>().is_err());
    }

    #[test]
    fn readable_text_choice() {
        assert!(Rgb::new(10, 10, 40).is_dark());
        assert!(!Rgb::new(250, 250, 200).is_dark());
    }

    #[test]
    fn history_moves_duplicates_to_front_and_persists() {
        let mut h = ColorHistory::default();
        h.push(Rgb::new(1, 2, 3));
        h.push(DODGER);
        h.push(Rgb::new(1, 2, 3));
        assert_eq!(h.colors(), &[Rgb::new(1, 2, 3), DODGER]);
        for i in 0..40 {
            h.push(Rgb::new(i, 0, 0));
        }
        assert_eq!(h.colors().len(), ColorHistory::MAX);

        let path = std::env::temp_dir().join(format!("yaltri-colors-{}.toml", std::process::id()));
        h.save(&path).unwrap();
        assert_eq!(ColorHistory::load(&path), h);
        fs::remove_file(path).unwrap();
    }
}
