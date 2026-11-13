//! Procedural tray icon (a four-pointed sparkle, for *yaltrı-* "to glitter")
//! so the binary needs no image assets yet.

pub const SIZE: u32 = 32;

/// Straight (non-premultiplied) RGBA, row-major.
pub fn rgba() -> Vec<u8> {
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let c = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 - c).abs() / c;
            let dy = (y as f32 - c).abs() / c;
            // Astroid-like star: |x|^0.5 + |y|^0.5 <= 1, plus a soft core.
            let star = dx.sqrt() + dy.sqrt();
            let core = (dx * dx + dy * dy).sqrt();
            let alpha = if star <= 1.0 {
                1.0
            } else if star <= 1.12 {
                1.0 - (star - 1.0) / 0.12
            } else {
                0.0
            }
            .max(if core < 0.35 { 1.0 - core / 0.35 } else { 0.0 });
            // Warm gold fading to white at the center.
            let w = (1.0 - core).clamp(0.0, 1.0);
            let r = 255.0;
            let g = 196.0 + 59.0 * w;
            let b = 64.0 + 191.0 * w * w;
            out.extend_from_slice(&[r as u8, g as u8, b as u8, (alpha * 255.0) as u8]);
        }
    }
    out
}

/// ARGB32 in network byte order, as StatusNotifierItem (ksni) expects.
#[cfg(target_os = "linux")]
pub fn argb() -> Vec<u8> {
    let mut data = rgba();
    for px in data.chunks_exact_mut(4) {
        px.rotate_right(1);
    }
    data
}
