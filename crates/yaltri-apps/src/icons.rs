//! Application icons as image files the UI can load by path.
//!
//! - Windows: the shell renders the icon of a shortcut or Store app
//!   (`IShellItemImageFactory`); it is cached as PNG under `cache_dir`.
//! - Linux: the `Icon=` name is looked up in the freedesktop icon themes and
//!   the PNG/SVG file found there is used directly.

use std::path::{Path, PathBuf};

/// Pixel size requested from the system; the launcher draws icons at 28-32
/// logical pixels, so 64 covers 2x displays.
pub const ICON_SIZE: u32 = 64;

/// Stable cache file name for a shell parsing name (+ file mtime, so an
/// updated shortcut gets a fresh icon).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn cache_key(parsing_name: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    parsing_name.to_lowercase().hash(&mut h);
    if let Ok(modified) = std::fs::metadata(parsing_name).and_then(|m| m.modified()) {
        modified.hash(&mut h);
    }
    format!("{:016x}.png", h.finish())
}

#[cfg(windows)]
pub use win::icon_for;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
    use windows::core::HSTRING;

    thread_local! {
        static COM: () = {
            // SAFETY: initializes COM for this (background) thread once.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        };
    }

    /// Cached PNG for a shell item: a file path (`...\App.lnk`) or a Store app
    /// (`shell:AppsFolder\<AUMID>`). Extracts and caches it on first use.
    pub fn icon_for(parsing_name: &str, cache_dir: &Path) -> Option<PathBuf> {
        let file = cache_dir.join(cache_key(parsing_name));
        if file.is_file() {
            return Some(file);
        }
        let rgba = extract(parsing_name)?;
        std::fs::create_dir_all(cache_dir).ok()?;
        let img = image::RgbaImage::from_raw(ICON_SIZE, ICON_SIZE, rgba)?;
        let tmp = file.with_extension("png.tmp");
        img.save_with_format(&tmp, image::ImageFormat::Png).ok()?;
        std::fs::rename(&tmp, &file).ok()?;
        Some(file)
    }

    fn extract(parsing_name: &str) -> Option<Vec<u8>> {
        COM.with(|_| ());
        // SAFETY: COM is initialized on this thread; every handle created here
        // is released before returning.
        unsafe {
            let factory: IShellItemImageFactory =
                SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None).ok()?;
            let size = SIZE { cx: ICON_SIZE as i32, cy: ICON_SIZE as i32 };
            let bitmap = factory.GetImage(size, SIIGBF_ICONONLY).ok()?;
            let pixels = bitmap_to_rgba(bitmap);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            pixels
        }
    }

    /// Reads a 32-bit HBITMAP (premultiplied BGRA, bottom-up) into straight
    /// RGBA, rescaled to ICON_SIZE if the shell returned another size.
    unsafe fn bitmap_to_rgba(bitmap: HBITMAP) -> Option<Vec<u8>> {
        unsafe {
            let mut info = BITMAP::default();
            if GetObjectW(HGDIOBJ(bitmap.0), size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) == 0 {
                return None;
            }
            let (w, h) = (info.bmWidth, info.bmHeight);
            if w <= 0 || h <= 0 {
                return None;
            }
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let dc = CreateCompatibleDC(None);
            let lines = GetDIBits(dc, bitmap, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
            let _ = DeleteDC(dc);
            if lines == 0 {
                return None;
            }

            // Bitmaps without an alpha channel report alpha 0 everywhere.
            let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
            for px in buf.chunks_exact_mut(4) {
                let (b, g, r, a) = (px[0], px[1], px[2], if has_alpha { px[3] } else { 255 });
                let unpremultiply = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
                px.copy_from_slice(&[unpremultiply(r), unpremultiply(g), unpremultiply(b), a]);
            }
            let img = image::RgbaImage::from_raw(w as u32, h as u32, buf)?;
            let img = if (w as u32, h as u32) == (ICON_SIZE, ICON_SIZE) {
                img
            } else {
                image::imageops::resize(&img, ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Triangle)
            };
            Some(img.into_raw())
        }
    }
}

/// Resolves a freedesktop `Icon=` value to a file, searching the usual themes.
#[cfg_attr(windows, allow(dead_code))]
pub fn find_theme_icon(name: &str, bases: &[PathBuf], themes: &[&str]) -> Option<PathBuf> {
    let as_path = Path::new(name);
    if as_path.is_absolute() {
        return as_path.is_file().then(|| as_path.to_path_buf());
    }
    // Largest bitmap first; scalable SVG is ideal at any size.
    const SIZES: [&str; 9] =
        ["scalable", "256x256", "128x128", "96x96", "72x72", "64x64", "48x48", "36x36", "32x32"];
    for base in bases {
        for theme in themes {
            for size in SIZES {
                for ext in ["svg", "png"] {
                    let p = base.join(theme).join(size).join("apps").join(format!("{name}.{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    for dir in ["/usr/share/pixmaps", "/usr/local/share/pixmaps"] {
        for ext in ["svg", "png"] {
            let p = Path::new(dir).join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `icons` folders to search on Linux, highest priority first.
#[cfg_attr(windows, allow(dead_code))]
pub fn icon_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        bases.push(home.join(".local/share/icons"));
        bases.push(home.join(".icons"));
        bases.push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    bases.extend(data_dirs.split(':').filter(|s| !s.is_empty()).map(|d| Path::new(d).join("icons")));
    bases.push("/var/lib/flatpak/exports/share/icons".into());
    bases.push("/var/lib/snapd/desktop/icons".into());
    bases.dedup();
    bases.retain(|b| b.is_dir());
    bases
}

/// Themes tried in order: the desktop's usual ones, then the hicolor fallback.
#[cfg_attr(windows, allow(dead_code))]
pub const THEMES: [&str; 6] = ["Yaru", "Adwaita", "breeze", "Papirus", "elementary", "hicolor"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_largest_theme_icon_then_pixmaps() {
        let base = std::env::temp_dir().join(format!("yaltri-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let icons = base.join("icons");
        for (size, ext) in [("48x48", "png"), ("128x128", "png")] {
            let dir = icons.join("hicolor").join(size).join("apps");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("firefox.{ext}")), b"").unwrap();
        }
        let found = find_theme_icon("firefox", &[icons.clone()], &THEMES).unwrap();
        assert!(found.ends_with("hicolor/128x128/apps/firefox.png") || found.ends_with(r"hicolor\128x128\apps\firefox.png"));
        assert!(find_theme_icon("nope", &[icons], &THEMES).is_none());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn cache_key_is_stable_and_case_insensitive() {
        assert_eq!(cache_key("shell:AppsFolder\\X!App"), cache_key("SHELL:appsfolder\\x!app"));
        assert!(cache_key("a").ends_with(".png"));
    }

    #[cfg(windows)]
    #[test]
    fn extracts_a_real_windows_icon() {
        let dir = std::env::temp_dir().join(format!("yaltri-iconcache-{}", std::process::id()));
        let notepad = std::env::var("SystemRoot").unwrap() + r"\System32\notepad.exe";
        let png = icon_for(&notepad, &dir).expect("notepad icon");
        let img = image::open(&png).unwrap();
        assert_eq!((img.width(), img.height()), (ICON_SIZE, ICON_SIZE));
        // Second call is served from the cache.
        assert_eq!(icon_for(&notepad, &dir).unwrap(), png);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
