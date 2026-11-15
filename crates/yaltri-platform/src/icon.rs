//! Tray and menu icons, rasterized from the brand / Lucide SVGs.
//!
//! The tray mark is monochrome: white on dark taskbars, #1A1A1A on light
//! ones. The 16 px size has its own drawing; larger sizes scale the 24 px one.

use resvg::{tiny_skia, usvg};

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
    fn recolors_current_color() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="currentColor"/></svg>"#;
        let rgba = svg_rgba(svg, 4, Some("#FF0000")).unwrap();
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    }
}
