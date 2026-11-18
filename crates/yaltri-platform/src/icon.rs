//! Tray and menu icons, rasterized from the brand / Lucide SVGs.
//!
//! The tray mark is monochrome: white on dark taskbars, #1A1A1A on light
//! ones. The 16 px size has its own drawing; larger sizes scale the 24 px one.

use resvg::{tiny_skia, usvg};

use crate::battery::DeviceKind;

const TRAY_16_WHITE: &str = include_str!("../../../design/brand/tray-16-white.svg");
const TRAY_16_DARK: &str = include_str!("../../../design/brand/tray-16-dark.svg");
const TRAY_24_WHITE: &str = include_str!("../../../design/brand/tray-24-white.svg");
const TRAY_24_DARK: &str = include_str!("../../../design/brand/tray-24-dark.svg");

/// Straight (non-premultiplied) RGBA of `svg` drawn into a `size`² square,
/// with `currentColor` replaced by `color` (`#RRGGBB`) when given.
pub fn svg_rgba(svg: &str, size: u32, color: Option<&str>) -> Option<Vec<u8>> {
    let svg = match color {
        Some(c) => svg.replace("currentColor", c),
        None => svg.to_owned(),
    };
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(
        pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect(),
    )
}

/// PNG of `svg_rgba`, for D-Bus menus that take encoded images.
#[cfg(target_os = "linux")]
pub fn svg_png(svg: &str, size: u32, color: Option<&str>) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(size, size, svg_rgba(svg, size, color)?)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// The tray mark at `size` px for a light or dark taskbar.
pub fn tray_rgba(size: u32, light_taskbar: bool) -> Vec<u8> {
    let svg = match (size <= 20, light_taskbar) {
        (true, false) => TRAY_16_WHITE,
        (true, true) => TRAY_16_DARK,
        (false, false) => TRAY_24_WHITE,
        (false, true) => TRAY_24_DARK,
    };
    svg_rgba(svg, size, None).unwrap_or_else(|| vec![0; (size * size * 4) as usize])
}

/// How a battery number is colored in the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Low,
    Charging,
}

/// Digits in a 4×9 pixel font, one string per row (`#` is ink).
fn digit_rows(c: char) -> Option<[&'static str; 9]> {
    Some(match c {
        '0' => [" ## ", "#  #", "#  #", "#  #", "#  #", "#  #", "#  #", "#  #", " ## "],
        '1' => ["  # ", " ## ", "  # ", "  # ", "  # ", "  # ", "  # ", "  # ", " ###"],
        '2' => [" ## ", "#  #", "   #", "   #", "  # ", " #  ", "#   ", "#   ", "####"],
        '3' => [" ## ", "#  #", "   #", "   #", " ## ", "   #", "   #", "#  #", " ## "],
        '4' => ["#  #", "#  #", "#  #", "#  #", "####", "   #", "   #", "   #", "   #"],
        '5' => ["####", "#   ", "#   ", "### ", "   #", "   #", "   #", "#  #", " ## "],
        '6' => [" ## ", "#   ", "#   ", "### ", "#  #", "#  #", "#  #", "#  #", " ## "],
        '7' => ["####", "   #", "   #", "  # ", "  # ", " #  ", " #  ", " #  ", " #  "],
        '8' => [" ## ", "#  #", "#  #", "#  #", " ## ", "#  #", "#  #", "#  #", " ## "],
        '9' => [" ## ", "#  #", "#  #", "#  #", " ###", "   #", "   #", "   #", " ## "],
        '-' => ["    ", "    ", "    ", "    ", "####", "    ", "    ", "    ", "    "],
        _ => return None,
    })
}

/// Device glyphs drawn for 16 px, where a scaled-down outline icon blurs.
fn device_rows(kind: DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::Mouse => &[" ### ", "# # #", "# # #", "#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
        DeviceKind::Earbuds => &[" #### ", "#    #", "#    #", "#    #", "##  ##", "##  ##", "##  ##", "##  ##"],
    }
}

/// `rows` as 1 px rects at (`x0`, `y0`) of the 16 px grid.
fn pixel_rects(out: &mut String, rows: &[&str], x0: usize, y0: usize) {
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                out.push_str(&format!(r#"<rect x="{}" y="{}" width="1" height="1"/>"#, x0 + x, y0 + y));
            }
        }
    }
}

/// A tray icon for a battery on a 16 px grid, drawn at `size` px: the
/// device on the left, `text` (digits or `-`) on the right. Three digits
/// (100) take the whole width, without the device.
pub fn battery_rgba(kind: DeviceKind, text: &str, tone: Tone, size: u32, light_taskbar: bool) -> Vec<u8> {
    let ink = if light_taskbar { "#1A1A1A" } else { "#FFFFFF" };
    let digit_ink = match tone {
        Tone::Normal => ink,
        Tone::Low => "#E5484D",
        Tone::Charging => if light_taskbar { "#18794E" } else { "#3DD68C" },
    };
    let digits: Vec<[&str; 9]> = text.chars().filter_map(digit_rows).collect();
    let width = (digits.len() * 5).saturating_sub(1);
    let mut device = String::new();
    // Right of the device: x 7..16, 9 px.
    let left = if digits.len() > 2 {
        16usize.saturating_sub(width) / 2
    } else {
        let rows = device_rows(kind);
        pixel_rects(&mut device, rows, (6 - rows[0].len()) / 2, (16 - rows.len()) / 2);
        7 + (9 - width) / 2
    };
    let mut number = String::new();
    for (i, rows) in digits.iter().enumerate() {
        pixel_rects(&mut number, rows, left + i * 5, 3);
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" shape-rendering="crispEdges"><g fill="{ink}">{device}</g><g fill="{digit_ink}">{number}</g></svg>"#
    );
    svg_rgba(&svg, size, None).unwrap_or_else(|| vec![0; (size * size * 4) as usize])
}

/// Whether the taskbar uses the light theme (Windows "system" mode, which
/// can differ from the apps mode). Linux panels are assumed dark.
pub fn taskbar_is_light() -> bool {
    #[cfg(windows)]
    {
        read_personalize_dword("SystemUsesLightTheme").is_some_and(|v| v != 0)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Whether apps (and so native menus) use the light theme.
pub fn apps_use_light_theme() -> bool {
    #[cfg(windows)]
    {
        read_personalize_dword("AppsUseLightTheme").is_none_or(|v| v != 0)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn read_personalize_dword(name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide(name);
    let mut data: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: valid NUL-terminated strings and a DWORD-sized out buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut len,
        )
    };
    (status == 0).then_some(data)
}

/// ARGB32 in network byte order, as StatusNotifierItem (ksni) expects.
#[cfg(target_os = "linux")]
pub fn argb(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.chunks_exact_mut(4) {
        px.rotate_right(1);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icons_rasterize_with_opaque_pixels() {
        for (size, light) in [(16, false), (16, true), (32, false), (32, true)] {
            let rgba = tray_rgba(size, light);
            assert_eq!(rgba.len(), (size * size * 4) as usize);
            let opaque = rgba.chunks_exact(4).filter(|p| p[3] > 200).count();
            assert!(opaque > (size * size / 20) as usize, "{size}px light={light}: mark is drawn");
            let expected = if light { 0x1A } else { 0xFF };
            assert!(rgba.chunks_exact(4).filter(|p| p[3] == 255).all(|p| p[0] == expected));
        }
    }

    #[test]
    fn battery_icon_puts_the_device_left_of_crisp_digits() {
        let rgba = battery_rgba(DeviceKind::Mouse, "35", Tone::Low, 16, false);
        let px = |x: usize, y: usize| &rgba[(y * 16 + x) * 4..][..4];
        // The mouse's top edge, in the taskbar ink.
        assert_eq!(px(1, 3), &[255, 255, 255, 255]);
        // "35" is 9 px wide from x = 7; the top row of "3" is " ## ".
        assert_eq!(px(8, 3), &[0xE5, 0x48, 0x4D, 255]);
        assert_eq!(px(7, 3)[3], 0);
        // One digit sits in the middle of the number's space.
        let one = battery_rgba(DeviceKind::Earbuds, "7", Tone::Normal, 16, true);
        assert_eq!(&one[(3 * 16 + 9) * 4..][..4], &[0x1A, 0x1A, 0x1A, 255], "top of the 7");
        // "100" is 14 px wide from x = 1, with no device; the 1's top at x = 3.
        let full = battery_rgba(DeviceKind::Mouse, "100", Tone::Normal, 16, true);
        assert_eq!(&full[(3 * 16 + 3) * 4..][..4], &[0x1A, 0x1A, 0x1A, 255], "the 1's top");
        assert_eq!(full[(3 * 16 + 1) * 4 + 3], 0, "no mouse");
        assert_eq!(battery_rgba(DeviceKind::Mouse, "-", Tone::Normal, 24, false).len(), 24 * 24 * 4);
    }

    #[test]
    fn recolors_current_color() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="currentColor"/></svg>"#;
        let rgba = svg_rgba(svg, 4, Some("#FF0000")).unwrap();
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    }
}
