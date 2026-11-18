//! Battery tray icons ("glyph gauge" design): the device's silhouette fills
//! with its charge, bottom to top (a keyboard left to right), above the
//! number in a pixel font. Drawn on a 16 px grid for sizes up to 20 px and a
//! 24 px grid above, each by hand, never one scaled to the other. Shapes sit
//! on whole or half pixels and may anti-alias at curves; digits are 1 px
//! cells drawn with crisp edges.

use crate::battery::DeviceKind;

/// What colors a number or a fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Low,
    Charging,
}

/// One battery's level and color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub percent: u8,
    pub tone: Tone,
}

/// Everything a battery icon shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gauge {
    pub kind: DeviceKind,
    /// The number, or `None` when the level is unknown (the whole glyph
    /// greys out and a dash replaces the number).
    pub number: Option<(u8, Tone)>,
    /// How full the glyph is. Earbuds fill each bud on its own: `fills[0]`
    /// is the left bud, `fills[1]` the right; `None` leaves it empty (a bud
    /// in its case). Other devices use `fills[0]`.
    pub fills: [Option<Fill>; 2],
}

struct Palette {
    track: &'static str,
    ink: &'static str,
    unknown: &'static str,
    low: &'static str,
    charging: &'static str,
}

const DARK: Palette = Palette { track: "#4D4D4D", ink: "#FFFFFF", unknown: "#8A8A8A", low: "#E5484D", charging: "#3DD68C" };
const LIGHT: Palette =
    Palette { track: "#C4C4C4", ink: "#1A1A1A", unknown: "#7A7A7A", low: "#E5484D", charging: "#18794E" };

impl Palette {
    fn tone(&self, tone: Tone) -> &'static str {
        match tone {
            Tone::Normal => self.ink,
            Tone::Low => self.low,
            Tone::Charging => self.charging,
        }
    }
}

/// One drawing size.
struct Grid {
    size: u32,
    /// Height of the glyph band (rows from the top) that fills.
    glyph_h: u32,
    /// Keyboard: where the fill starts and how wide it gets.
    keyboard_x: u32,
    keyboard_w: u32,
    /// First row of the digits.
    digits_y: u32,
    /// The dash shown for an unknown level: x, y, width.
    dash: (u32, u32, u32),
}

const G16: Grid = Grid { size: 16, glyph_h: 9, keyboard_x: 1, keyboard_w: 14, digits_y: 10, dash: (6, 13, 3) };
const G24: Grid = Grid { size: 24, glyph_h: 14, keyboard_x: 1, keyboard_w: 22, digits_y: 15, dash: (9, 19, 4) };

/// The device's shapes in `COLOR`, with their offset on the grid.
fn glyph(kind: DeviceKind, grid: &Grid) -> (&'static str, &'static str) {
    match (kind, grid.size) {
        (DeviceKind::Mouse, 16) => (
            "4 0",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M0 3.5A3.5 3.5 0 0 1 7 3.5V6A3 3 0 0 1 4 9H3A3 3 0 0 1 0 6ZM3 0h1v4h-1z"/>"#,
        ),
        (DeviceKind::Mouse, _) => (
            "6 0",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M0 5.5A5.5 5.5 0 0 1 11 5.5V9.5A4.5 4.5 0 0 1 6.5 14H4.5A4.5 4.5 0 0 1 0 9.5ZM5 0h1v6h-1z"/>"#,
        ),
        (DeviceKind::Headset, 16) => (
            "3 0",
            r#"<path fill="none" stroke="COLOR" stroke-width="1" d="M0.5 6V5A4.5 4.5 0 0 1 9.5 5V6"/><path fill="COLOR" fill-rule="evenodd" d="M1 5H2A1 1 0 0 1 3 6V8A1 1 0 0 1 2 9H1A1 1 0 0 1 0 8V6A1 1 0 0 1 1 5ZM8 5H9A1 1 0 0 1 10 6V8A1 1 0 0 1 9 9H8A1 1 0 0 1 7 8V6A1 1 0 0 1 8 5Z"/>"#,
        ),
        (DeviceKind::Headset, _) => (
            "4 0",
            r#"<path fill="none" stroke="COLOR" stroke-width="2" d="M1 9V8A7 7 0 0 1 15 8V9"/><path fill="COLOR" fill-rule="evenodd" d="M1.5 8H2.5A1.5 1.5 0 0 1 4 9.5V12.5A1.5 1.5 0 0 1 2.5 14H1.5A1.5 1.5 0 0 1 0 12.5V9.5A1.5 1.5 0 0 1 1.5 8ZM13.5 8H14.5A1.5 1.5 0 0 1 16 9.5V12.5A1.5 1.5 0 0 1 14.5 14H13.5A1.5 1.5 0 0 1 12 12.5V9.5A1.5 1.5 0 0 1 13.5 8Z"/>"#,
        ),
        (DeviceKind::Earbuds, 16) => (
            "2 0",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M0 2.5A2.5 2.5 0 1 0 5 2.5A2.5 2.5 0 1 0 0 2.5ZM4 3H4A1 1 0 0 1 5 4V8A1 1 0 0 1 4 9H4A1 1 0 0 1 3 8V4A1 1 0 0 1 4 3Z"/><path fill="COLOR" fill-rule="evenodd" d="M7 2.5A2.5 2.5 0 1 0 12 2.5A2.5 2.5 0 1 0 7 2.5ZM8 3H8A1 1 0 0 1 9 4V8A1 1 0 0 1 8 9H8A1 1 0 0 1 7 8V4A1 1 0 0 1 8 3Z"/>"#,
        ),
        (DeviceKind::Earbuds, _) => (
            "3 0",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M0 3.5A3.5 3.5 0 1 0 7 3.5A3.5 3.5 0 1 0 0 3.5ZM5.5 4H5.5A1.5 1.5 0 0 1 7 5.5V12.5A1.5 1.5 0 0 1 5.5 14H5.5A1.5 1.5 0 0 1 4 12.5V5.5A1.5 1.5 0 0 1 5.5 4Z"/><path fill="COLOR" fill-rule="evenodd" d="M11 3.5A3.5 3.5 0 1 0 18 3.5A3.5 3.5 0 1 0 11 3.5ZM12.5 4H12.5A1.5 1.5 0 0 1 14 5.5V12.5A1.5 1.5 0 0 1 12.5 14H12.5A1.5 1.5 0 0 1 11 12.5V5.5A1.5 1.5 0 0 1 12.5 4Z"/>"#,
        ),
        (DeviceKind::Keyboard, 16) => (
            "1 1",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M1.5 0H12.5A1.5 1.5 0 0 1 14 1.5V5.5A1.5 1.5 0 0 1 12.5 7H1.5A1.5 1.5 0 0 1 0 5.5V1.5A1.5 1.5 0 0 1 1.5 0ZM2 1h1v1h-1zM2 3h1v1h-1zM5 1h1v1h-1zM5 3h1v1h-1zM8 1h1v1h-1zM8 3h1v1h-1zM11 1h1v1h-1zM11 3h1v1h-1zM4 5h6v1h-6z"/>"#,
        ),
        (DeviceKind::Keyboard, _) => (
            "1 1",
            r#"<path fill="COLOR" fill-rule="evenodd" d="M2 0H20A2 2 0 0 1 22 2V10A2 2 0 0 1 20 12H2A2 2 0 0 1 0 10V2A2 2 0 0 1 2 0ZM2 2h2v2h-2zM2 5h2v2h-2zM6 2h2v2h-2zM6 5h2v2h-2zM10 2h2v2h-2zM10 5h2v2h-2zM14 2h2v2h-2zM14 5h2v2h-2zM18 2h2v2h-2zM18 5h2v2h-2zM6 8h10v2h-10z"/>"#,
        ),
    }
}

/// Digits, one string per row (`#` is ink). 16 px: 4×6, 24 px: 5×9; the 1
/// is two columns wide in both.
fn digit_rows(c: char, grid: &Grid) -> Option<&'static [&'static str]> {
    let small = grid.size == 16;
    Some(match (c, small) {
        ('0', true) => &[" ## ", "#  #", "#  #", "#  #", "#  #", " ## "],
        ('1', true) => &[" #", "##", " #", " #", " #", " #"],
        ('2', true) => &[" ## ", "#  #", "   #", "  # ", " #  ", "####"],
        ('3', true) => &["### ", "   #", " ## ", "   #", "   #", "### "],
        ('4', true) => &["#  #", "#  #", "#  #", "####", "   #", "   #"],
        ('5', true) => &["####", "#   ", "### ", "   #", "   #", "### "],
        ('6', true) => &[" ## ", "#   ", "### ", "#  #", "#  #", " ## "],
        ('7', true) => &["####", "   #", "  # ", "  # ", " #  ", " #  "],
        ('8', true) => &[" ## ", "#  #", " ## ", "#  #", "#  #", " ## "],
        ('9', true) => &[" ## ", "#  #", "#  #", " ###", "   #", " ## "],
        ('0', false) => &[" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
        ('1', false) => &[" #", "##", " #", " #", " #", " #", " #", " #", " #"],
        ('2', false) => &[" ### ", "#   #", "    #", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"],
        ('3', false) => &[" ### ", "#   #", "    #", "    #", " ### ", "    #", "    #", "#   #", " ### "],
        ('4', false) => &["#   #", "#   #", "#   #", "#   #", "#####", "    #", "    #", "    #", "    #"],
        ('5', false) => &["#####", "#    ", "#    ", "#### ", "    #", "    #", "    #", "#   #", " ### "],
        ('6', false) => &[" ### ", "#    ", "#    ", "#### ", "#   #", "#   #", "#   #", "#   #", " ### "],
        ('7', false) => &["#####", "    #", "    #", "   # ", "   # ", "  #  ", "  #  ", "  #  ", "  #  "],
        ('8', false) => &[" ### ", "#   #", "#   #", "#   #", " ### ", "#   #", "#   #", "#   #", " ### "],
        ('9', false) => &[" ### ", "#   #", "#   #", "#   #", " ####", "    #", "    #", "    #", " ### "],
        _ => return None,
    })
}

/// `n` as 1 px rects, centered, on the digit rows.
fn number_cells(n: u8, grid: &Grid) -> String {
    let glyphs: Vec<&[&str]> = n.to_string().chars().filter_map(|c| digit_rows(c, grid)).collect();
    let width: usize = glyphs.iter().map(|g| g[0].len()).sum::<usize>() + glyphs.len().saturating_sub(1);
    let mut x = (grid.size as usize).saturating_sub(width) / 2;
    let mut out = String::new();
    for rows in glyphs {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c == '#' {
                    out.push_str(&format!("M{} {}h1v1h-1z", x + dx, grid.digits_y as usize + dy));
                }
            }
        }
        x += rows[0].len() + 1;
    }
    out
}

/// Rows (or keyboard columns) a `percent` fill covers; at least one for any
/// charge above zero.
fn filled(percent: u8, span: u32) -> u32 {
    let n = (u32::from(percent.min(100)) * span + 50) / 100;
    if percent > 0 { n.max(1) } else { 0 }
}

/// The icon as an SVG on its grid.
fn svg(gauge: &Gauge, grid: &Grid, p: &Palette) -> String {
    let (offset, shapes) = glyph(gauge.kind, grid);
    let draw = |color: &str| format!(r#"<g transform="translate({offset})">{}</g>"#, shapes.replace("COLOR", color));
    let size = grid.size;
    let Some((number, tone)) = gauge.number else {
        let (x, y, w) = grid.dash;
        return format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{}<path fill="{}" shape-rendering="crispEdges" d="M{x} {y}h{w}v1h-{w}z"/></svg>"#,
            draw(p.unknown),
            p.unknown,
        );
    };
    // Each fill is the glyph again, clipped to the part that is full.
    let mut defs = String::new();
    let mut fills = String::new();
    let mut clip = |i: usize, (x, y, w, h): (u32, u32, u32, u32), tone: Tone| {
        defs.push_str(&format!(r#"<clipPath id="f{i}"><rect x="{x}" y="{y}" width="{w}" height="{h}"/></clipPath>"#));
        fills.push_str(&format!(r#"<g clip-path="url(#f{i})">{}</g>"#, draw(p.tone(tone))));
    };
    match gauge.kind {
        DeviceKind::Keyboard => {
            if let Some(f) = gauge.fills[0] {
                clip(0, (grid.keyboard_x, 0, filled(f.percent, grid.keyboard_w), size), f.tone);
            }
        }
        DeviceKind::Earbuds => {
            let half = size / 2;
            for (i, f) in gauge.fills.iter().enumerate() {
                if let Some(f) = f {
                    let h = filled(f.percent, grid.glyph_h);
                    clip(i, (i as u32 * half, grid.glyph_h - h, half, h), f.tone);
                }
            }
        }
        _ => {
            if let Some(f) = gauge.fills[0] {
                let h = filled(f.percent, grid.glyph_h);
                clip(0, (0, grid.glyph_h - h, size, h), f.tone);
            }
        }
    }
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}"><defs>{defs}</defs>{}{fills}<path fill="{}" shape-rendering="crispEdges" d="{}"/></svg>"#,
        draw(p.track),
        p.tone(tone),
        number_cells(number, grid),
    )
}

/// The icon at `size` px for a light or dark taskbar, as straight RGBA.
pub fn rgba(gauge: &Gauge, size: u32, light_taskbar: bool) -> Vec<u8> {
    let grid = if size <= 20 { &G16 } else { &G24 };
    let palette = if light_taskbar { &LIGHT } else { &DARK };
    crate::icon::svg_rgba(&svg(gauge, grid, palette), size, None).unwrap_or_else(|| vec![0; (size * size * 4) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(rgba: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        rgba[(y * size + x) * 4..][..4].try_into().unwrap()
    }

    fn single(kind: DeviceKind, percent: u8, tone: Tone) -> Gauge {
        Gauge { kind, number: Some((percent, tone)), fills: [Some(Fill { percent, tone }), None] }
    }

    #[test]
    fn fill_heights_match_the_design() {
        // 16 px: 9 rows; 24 px: 14 rows; keyboard 14 and 22 columns.
        let cases = [(85, 8, 12), (15, 1, 2), (60, 5, 8), (100, 9, 14), (51, 5, 7), (67, 6, 9), (40, 4, 6), (72, 6, 10)];
        for (p, h16, h24) in cases {
            assert_eq!(filled(p, 9), h16, "{p}% at 16");
            assert_eq!(filled(p, 14), h24, "{p}% at 24");
        }
        assert_eq!((filled(85, 14), filled(15, 22), filled(60, 22)), (12, 3, 13));
        assert_eq!((filled(1, 9), filled(0, 9)), (1, 0), "any charge shows");
    }

    #[test]
    fn digits_are_centered_like_the_design() {
        // "85" at 16 px starts at x 3, "15" at x 4, "100" at x 2.
        let first_x = |n: u8, grid: &Grid| {
            let cells = number_cells(n, grid);
            cells.split('M').filter(|c| !c.is_empty()).map(|c| c.split(' ').next().unwrap().parse::<u32>().unwrap()).min()
        };
        assert_eq!(first_x(85, &G16), Some(3));
        assert_eq!(first_x(15, &G16), Some(4));
        assert_eq!(first_x(100, &G16), Some(2));
        assert_eq!(first_x(85, &G24), Some(6));
        assert_eq!(first_x(15, &G24), Some(8));
        assert_eq!(first_x(100, &G24), Some(5));
        for d in 0..10 {
            assert!(digit_rows(char::from(b'0' + d), &G16).is_some_and(|r| r.len() == 6));
            assert!(digit_rows(char::from(b'0' + d), &G24).is_some_and(|r| r.len() == 9));
        }
    }

    /// Mouse at 60% on a dark taskbar (fill rows 4..9): the body above the
    /// fill stays track colored, below it is white, the digits are crisp.
    #[test]
    fn mouse_fills_from_the_bottom() {
        let image = rgba(&single(DeviceKind::Mouse, 60, Tone::Normal), 16, false);
        assert_eq!(px(&image, 16, 5, 2), [0x4D, 0x4D, 0x4D, 255], "above the fill");
        assert_eq!(px(&image, 16, 5, 6), [255, 255, 255, 255], "inside the fill");
        // Top row of the 6 is " ## " from x 3.
        assert_eq!(px(&image, 16, 4, 10), [255, 255, 255, 255]);
        assert_eq!(px(&image, 16, 3, 10)[3], 0);
    }

    #[test]
    fn earbuds_fill_each_bud_and_unknown_greys_out() {
        let buds = Gauge {
            kind: DeviceKind::Earbuds,
            number: Some((15, Tone::Low)),
            fills: [Some(Fill { percent: 15, tone: Tone::Low }), Some(Fill { percent: 40, tone: Tone::Normal })],
        };
        let image = rgba(&buds, 16, true);
        // Right stem (x 9..11) is filled from row 5; the left one only on
        // row 8, so its row 6 is still track. The number is low red: the 1
        // of "15" starts at x 4 with " #".
        assert_eq!(px(&image, 16, 10, 6), [0x1A, 0x1A, 0x1A, 255]);
        assert_eq!(px(&image, 16, 6, 6), [0xC4, 0xC4, 0xC4, 255], "left bud above its fill");
        assert_eq!(px(&image, 16, 5, 10), [0xE5, 0x48, 0x4D, 255]);

        let unknown = Gauge { kind: DeviceKind::Keyboard, number: None, fills: [None, None] };
        let image = rgba(&unknown, 24, false);
        assert_eq!(px(&image, 24, 10, 19), [0x8A, 0x8A, 0x8A, 255], "dash");
        assert_eq!(px(&image, 24, 10, 16)[3], 0, "no number");
    }
}
