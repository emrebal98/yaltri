//! Approximate color names ("light_blue") for the picker's color box.
//!
//! Ported from PowerToys' `ColorNameHelper` (MIT License, Copyright (c)
//! Microsoft Corporation): the same hue tables, bucketed by saturation and
//! lightness on a 0-255 scale, with keys instead of names.

use crate::Rgb;

/// Every key `Rgb::name_key` returns; the app translates `color.<key>`.
pub const NAME_KEYS: &[&str] = &["coral", "rose", "light_orange", "tan", "light_yellow", "light_green", "lime", "aqua", "sky_blue", "light_turquoise", "pale_blue", "light_blue", "ice_blue", "periwinkle", "lavender", "pink", "red", "orange", "brown", "gold", "yellow", "olive_green", "green", "bright_green", "teal", "turquoise", "blue", "blue_gray", "indigo", "purple", "dark_red", "dark_yellow", "dark_green", "dark_teal", "dark_blue", "dark_purple", "plum", "white", "black", "light_gray", "gray", "dark_gray"];

const HUE_LIMITS_SAT1: [u16; 23] = [8, 0, 0, 44, 0, 0, 0, 63, 0, 0, 122, 0, 134, 0, 0, 0, 0, 166, 176, 241, 0, 256, 0];
const HUE_LIMITS_SAT2: [u16; 23] = [0, 10, 0, 32, 46, 0, 0, 0, 61, 0, 106, 0, 136, 144, 0, 0, 0, 158, 166, 241, 0, 0, 256];
const HUE_LIMITS_SAT3: [u16; 23] = [0, 8, 0, 0, 39, 46, 0, 0, 0, 71, 120, 0, 131, 144, 0, 0, 163, 0, 177, 211, 249, 0, 256];
const HUE_LIMITS_SAT4: [u16; 23] = [0, 11, 26, 0, 0, 38, 45, 0, 0, 56, 100, 121, 129, 0, 140, 0, 180, 0, 0, 224, 241, 0, 256];
const HUE_LIMITS_SAT5: [u16; 23] = [0, 13, 27, 0, 0, 36, 45, 0, 0, 59, 118, 0, 127, 136, 142, 0, 185, 0, 0, 216, 239, 0, 256];

const LUM_LOW: [u16; 23] = [130, 100, 115, 100, 100, 100, 110, 75, 100, 90, 100, 100, 100, 100, 80, 100, 100, 100, 100, 100, 100, 100, 100];
const LUM_HIGH: [u16; 23] = [170, 170, 170, 155, 170, 170, 170, 170, 170, 115, 170, 170, 170, 170, 170, 170, 170, 170, 150, 150, 170, 140, 165];

const LIGHT: [&str; 23] = [
    "coral", "rose", "light_orange", "tan", "tan", "light_yellow", "light_yellow", "tan",
    "light_green", "lime", "light_green", "light_green", "aqua", "sky_blue", "light_turquoise",
    "pale_blue", "light_blue", "ice_blue", "periwinkle", "lavender", "pink", "tan", "rose",
];
const MID: [&str; 23] = [
    "coral", "red", "orange", "brown", "tan", "gold", "yellow", "olive_green",
    "olive_green", "green", "green", "bright_green", "teal", "aqua", "turquoise",
    "pale_blue", "blue", "blue_gray", "indigo", "purple", "pink", "brown", "red",
];
const DARK: [&str; 23] = [
    "brown", "dark_red", "brown", "brown", "brown", "dark_yellow", "dark_yellow",
    "brown", "dark_green", "dark_green", "dark_green", "dark_green", "dark_teal", "dark_teal",
    "dark_teal", "dark_blue", "dark_blue", "blue_gray", "indigo", "dark_purple", "plum", "brown",
    "dark_red",
];

impl Rgb {
    /// Key of a short, human name for the color, as PowerToys shows under
    /// the code: `light_blue` for "Light blue".
    pub fn name_key(self) -> &'static str {
        let (h, s, l) = self.to_hsl();
        let hue = h / 360. * 255.;
        let sat = s * 255.;
        let lum = l * 255.;

        if lum > 240. {
            return "white";
        }
        if lum < 20. {
            return "black";
        }
        if sat <= 20. {
            return if lum > 170. {
                "light_gray"
            } else if lum > 100. {
                "gray"
            } else {
                "dark_gray"
            };
        }
        let limits = if sat <= 75. {
            &HUE_LIMITS_SAT1
        } else if sat <= 115. {
            &HUE_LIMITS_SAT2
        } else if sat <= 150. {
            &HUE_LIMITS_SAT3
        } else if sat <= 240. {
            &HUE_LIMITS_SAT4
        } else {
            &HUE_LIMITS_SAT5
        };
        let Some(i) = limits.iter().position(|&limit| hue < limit as f32) else {
            return "";
        };
        if lum > LUM_HIGH[i] as f32 {
            LIGHT[i]
        } else if lum < LUM_LOW[i] as f32 {
            DARK[i]
        } else {
            MID[i]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(hex: &str) -> &'static str {
        Rgb::from_hex(hex).unwrap().name_key()
    }

    #[test]
    fn matches_powertoys_examples() {
                assert_eq!(name("#89b4fa"), "light_blue");
        assert_eq!(name("#ffffff"), "white");
        assert_eq!(name("#000000"), "black");
        assert_eq!(name("#808080"), "gray");
    }

    #[test]
    fn primary_colors_are_named_sensibly() {
        assert_eq!(name("#ff0000"), "red");
        assert_eq!(name("#00ff00"), "green");
        assert_eq!(name("#00a000"), "dark_green");
        assert_eq!(name("#0000ff"), "blue");
        assert_eq!(name("#ffff00"), "yellow");
        assert_eq!(name("#ff8000"), "orange");
    }

    #[test]
    fn every_color_gets_a_known_key() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    let key = Rgb::new(r, g, b).name_key();
                    assert!(NAME_KEYS.contains(&key), "{r},{g},{b}: {key:?}");
                }
            }
        }
    }
}
