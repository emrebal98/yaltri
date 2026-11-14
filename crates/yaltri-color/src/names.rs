//! Approximate color names ("Açık mavi") for the picker's color box.
//!
//! Ported from PowerToys' `ColorNameHelper` (MIT License, Copyright (c)
//! Microsoft Corporation): the same hue tables, bucketed by saturation and
//! lightness on a 0-255 scale, with Turkish names.

use crate::Rgb;

const HUE_LIMITS_SAT1: [u16; 23] = [8, 0, 0, 44, 0, 0, 0, 63, 0, 0, 122, 0, 134, 0, 0, 0, 0, 166, 176, 241, 0, 256, 0];
const HUE_LIMITS_SAT2: [u16; 23] = [0, 10, 0, 32, 46, 0, 0, 0, 61, 0, 106, 0, 136, 144, 0, 0, 0, 158, 166, 241, 0, 0, 256];
const HUE_LIMITS_SAT3: [u16; 23] = [0, 8, 0, 0, 39, 46, 0, 0, 0, 71, 120, 0, 131, 144, 0, 0, 163, 0, 177, 211, 249, 0, 256];
const HUE_LIMITS_SAT4: [u16; 23] = [0, 11, 26, 0, 0, 38, 45, 0, 0, 56, 100, 121, 129, 0, 140, 0, 180, 0, 0, 224, 241, 0, 256];
const HUE_LIMITS_SAT5: [u16; 23] = [0, 13, 27, 0, 0, 36, 45, 0, 0, 59, 118, 0, 127, 136, 142, 0, 185, 0, 0, 216, 239, 0, 256];

const LUM_LOW: [u16; 23] = [130, 100, 115, 100, 100, 100, 110, 75, 100, 90, 100, 100, 100, 100, 80, 100, 100, 100, 100, 100, 100, 100, 100];
const LUM_HIGH: [u16; 23] = [170, 170, 170, 155, 170, 170, 170, 170, 170, 115, 170, 170, 170, 170, 170, 170, 170, 170, 150, 150, 170, 140, 165];

const LIGHT: [&str; 23] = [
    "Mercan", "Gül", "Açık turuncu", "Ten rengi", "Ten rengi", "Açık sarı", "Açık sarı", "Ten rengi",
    "Açık yeşil", "Limon yeşili", "Açık yeşil", "Açık yeşil", "Su yeşili", "Gök mavisi", "Açık turkuaz",
    "Soluk mavi", "Açık mavi", "Buz mavisi", "Lavanta mavisi", "Lavanta", "Pembe", "Ten rengi", "Gül",
];
const MID: [&str; 23] = [
    "Mercan", "Kırmızı", "Turuncu", "Kahverengi", "Ten rengi", "Altın", "Sarı", "Zeytin yeşili",
    "Zeytin yeşili", "Yeşil", "Yeşil", "Parlak yeşil", "Petrol mavisi", "Su yeşili", "Turkuaz",
    "Soluk mavi", "Mavi", "Mavi gri", "Çivit", "Mor", "Pembe", "Kahverengi", "Kırmızı",
];
const DARK: [&str; 23] = [
    "Kahverengi", "Koyu kırmızı", "Kahverengi", "Kahverengi", "Kahverengi", "Koyu sarı", "Koyu sarı",
    "Kahverengi", "Koyu yeşil", "Koyu yeşil", "Koyu yeşil", "Koyu yeşil", "Koyu petrol", "Koyu petrol",
    "Koyu petrol", "Koyu mavi", "Koyu mavi", "Mavi gri", "Çivit", "Koyu mor", "Erik moru", "Kahverengi",
    "Koyu kırmızı",
];

impl Rgb {
    /// A short, human name for the color, as PowerToys shows under the code.
    pub fn name(self) -> &'static str {
        let (h, s, l) = self.to_hsl();
        let hue = h / 360. * 255.;
        let sat = s * 255.;
        let lum = l * 255.;

        if lum > 240. {
            return "Beyaz";
        }
        if lum < 20. {
            return "Siyah";
        }
        if sat <= 20. {
            return if lum > 170. {
                "Açık gri"
            } else if lum > 100. {
                "Gri"
            } else {
                "Koyu gri"
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
        Rgb::from_hex(hex).unwrap().name()
    }

    #[test]
    fn matches_powertoys_examples() {
        // PowerToys shows "Light blue" for this one.
        assert_eq!(name("#89b4fa"), "Açık mavi");
        assert_eq!(name("#ffffff"), "Beyaz");
        assert_eq!(name("#000000"), "Siyah");
        assert_eq!(name("#808080"), "Gri");
    }

    #[test]
    fn primary_colors_are_named_sensibly() {
        assert_eq!(name("#ff0000"), "Kırmızı");
        assert_eq!(name("#00ff00"), "Yeşil");
        assert_eq!(name("#00a000"), "Koyu yeşil"); // PowerToys: "Dark green"
        assert_eq!(name("#0000ff"), "Mavi");
        assert_eq!(name("#ffff00"), "Sarı");
        assert_eq!(name("#ff8000"), "Turuncu");
    }

    #[test]
    fn every_color_gets_a_name() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    assert!(!Rgb::new(r, g, b).name().is_empty(), "{r},{g},{b}");
                }
            }
        }
    }
}
