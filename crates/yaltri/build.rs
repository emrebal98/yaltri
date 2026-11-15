//! Windows: embeds the app icon (rendered from `design/brand/app-icon.svg`)
//! as icon resource 1, which the taskbar, Alt+Tab, Task Manager and GPUI's
//! windows use, and version info so Windows shows "Yaltri" instead of
//! "yaltri.exe" (Task Manager, startup apps, file properties).

use std::path::PathBuf;

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    let svg_path = "../../design/brand/app-icon.svg";
    println!("cargo:rerun-if-changed={svg_path}");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico_path = out.join("yaltri.ico");
    std::fs::write(&ico_path, render_ico(svg_path)).expect("write ico");

    let rc_path = out.join("yaltri.rc");
    let escaped = ico_path.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc_path, format!("1 ICON \"{escaped}\"\n{}", version_info())).expect("write rc");
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().expect("embedding resources");
}

/// ICO with every size: small ones as classic 32-bit bitmaps (what the
/// shell's small-icon paths read reliably), 256 px as PNG.
fn render_ico(svg_path: &str) -> Vec<u8> {
    let svg = std::fs::read_to_string(svg_path).expect("app icon svg");
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("valid app icon svg");

    let images: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&size| {
            let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
            let scale = size as f32 / tree.size().width();
            resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
            let data = if size >= 256 { pixmap.encode_png().expect("png") } else { bitmap(&pixmap) };
            (size, data)
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        ico.extend_from_slice(data);
    }
    ico
}

/// ICO bitmap entry: BITMAPINFOHEADER (double height), bottom-up BGRA
/// (straight alpha), then a 1-bit AND mask (all zero: alpha decides).
fn bitmap(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mask_row = (w.div_ceil(32) * 4) as usize;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(2 * h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]); // compression, sizes, resolution, colors
    for y in (0..h).rev() {
        for x in 0..w {
            let c = pixmap.pixel(x, y).expect("in bounds").demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
    }
    out.resize(out.len() + mask_row * h as usize, 0);
    out
}

/// VERSIONINFO: the name Windows shows for the process and startup entry.
fn version_info() -> String {
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts: Vec<u32> = version.split(['-', '+']).next().unwrap_or("0").split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let numeric = parts.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    format!(
        r#"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Yaltri"
      VALUE "FileDescription", "Yaltri"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "yaltri"
      VALUE "LegalCopyright", "Copyright (C) 2026 Yaltri contributors. GPL-3.0-or-later"
      VALUE "OriginalFilename", "yaltri.exe"
      VALUE "ProductName", "Yaltri"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
