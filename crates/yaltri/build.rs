//! Windows: embeds the app icon (rendered from `design/brand/app-icon.svg`)
//! as icon resource 1, which the taskbar, Alt+Tab and GPUI's windows use.

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
    let svg = std::fs::read_to_string(svg_path).expect("app icon svg");
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("valid app icon svg");

    let pngs: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&size| {
            let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
            let scale = size as f32 / tree.size().width();
            resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
            (size, pixmap.encode_png().expect("png"))
        })
        .collect();

    // ICO container holding PNG images.
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, png) in &pngs {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &pngs {
        ico.extend_from_slice(png);
    }
    let ico_path = out.join("yaltri.ico");
    std::fs::write(&ico_path, ico).expect("write ico");

    let rc_path = out.join("yaltri.rc");
    let escaped = ico_path.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc_path, format!("1 ICON \"{escaped}\"\n")).expect("write rc");
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().expect("embedding the icon");
}
