//! Drawing on a screenshot: arrows, boxes, pen, highlighter, text, number
//! badges and pixelation.
//!
//! Shapes live in logical px relative to the selection's top-left corner.
//! The same renderer makes the on-screen preview and the copied / saved
//! image: pixelated areas are applied to the pixels first, then every other
//! shape is written as SVG and drawn over them with resvg.

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};
use yaltri_platform::screen::Screenshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Rect,
    Pen,
    Marker,
    Text,
    Number,
    Pixelate,
}

impl Tool {
    pub const ALL: [Tool; 7] =
        [Tool::Arrow, Tool::Rect, Tool::Pen, Tool::Marker, Tool::Text, Tool::Number, Tool::Pixelate];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Arrow => "move-up-right",
            Tool::Rect => "square",
            Tool::Pen => "pencil",
            Tool::Marker => "highlighter",
            Tool::Text => "type",
            Tool::Number => "list-ordered",
            Tool::Pixelate => "grid-2x2",
        }
    }
}

/// Red, yellow, green, blue, black, white.
pub const COLORS: [u32; 6] = [0xE5303A, 0xF5C400, 0x2EAD5B, 0x2F7BF5, 0x111111, 0xFFFFFF];
/// Thin, medium, thick (logical px).
pub const WIDTHS: [f32; 3] = [2., 4., 8.];

pub type Pt = (f32, f32);

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Arrow { from: Pt, to: Pt, color: u32, width: f32 },
    Rect { from: Pt, to: Pt, color: u32, width: f32 },
    Pen { points: Vec<Pt>, color: u32, width: f32 },
    Marker { points: Vec<Pt>, color: u32, width: f32 },
    Text { at: Pt, text: String, color: u32, width: f32 },
    Number { at: Pt, n: u32, color: u32, width: f32 },
    Pixelate { from: Pt, to: Pt },
}

impl Shape {
    /// A new shape for `tool` started at `at`.
    pub fn start(tool: Tool, at: Pt, color: u32, width: f32, next_number: u32) -> Shape {
        match tool {
            Tool::Arrow => Shape::Arrow { from: at, to: at, color, width },
            Tool::Rect => Shape::Rect { from: at, to: at, color, width },
            Tool::Pen => Shape::Pen { points: vec![at], color, width },
            Tool::Marker => Shape::Marker { points: vec![at], color, width },
            Tool::Text => Shape::Text { at, text: String::new(), color, width },
            Tool::Number => Shape::Number { at, n: next_number, color, width },
            Tool::Pixelate => Shape::Pixelate { from: at, to: at },
        }
    }

    /// Follows the pointer while dragging. `straight` (Shift) keeps arrows
    /// at 45° steps and boxes square.
    pub fn drag_to(&mut self, to: Pt, straight: bool) {
        match self {
            Shape::Arrow { from, to: end, .. } => *end = if straight { snap_45(*from, to) } else { to },
            Shape::Rect { from, to: end, .. } => *end = if straight { square(*from, to) } else { to },
            Shape::Pixelate { from, to: end } => *end = if straight { square(*from, to) } else { to },
            Shape::Pen { points, .. } | Shape::Marker { points, .. } => {
                if points.last().is_none_or(|p| dist(*p, to) >= 1.) {
                    points.push(to);
                }
            }
            Shape::Text { .. } | Shape::Number { .. } => {}
        }
    }

    /// Too small to keep (a click with a drag tool).
    pub fn is_empty(&self) -> bool {
        match self {
            Shape::Arrow { from, to, .. } => dist(*from, *to) < 3.,
            Shape::Rect { from, to, .. } | Shape::Pixelate { from, to } => {
                (from.0 - to.0).abs() < 3. || (from.1 - to.1).abs() < 3.
            }
            Shape::Pen { points, .. } | Shape::Marker { points, .. } => points.len() < 2,
            Shape::Text { text, .. } => text.trim().is_empty(),
            Shape::Number { .. } => false,
        }
    }
}

fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// `to` moved onto the nearest 45° ray from `from`.
fn snap_45(from: Pt, to: Pt) -> Pt {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt();
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    (from.0 + len * angle.cos(), from.1 + len * angle.sin())
}

/// `to` moved so the box from `from` is square (the longer side wins).
fn square(from: Pt, to: Pt) -> Pt {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let side = dx.abs().max(dy.abs());
    (from.0 + side.copysign(dx), from.1 + side.copysign(dy))
}

fn text_size(width: f32) -> f32 {
    14. + width * 2.
}

fn badge_radius(width: f32) -> f32 {
    10. + width * 1.5
}

fn hex(color: u32) -> String {
    format!("#{color:06X}")
}

/// Dark text on light badges, white on the rest.
fn on_color(color: u32) -> &'static str {
    let [_, r, g, b] = color.to_be_bytes();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 160. { "#111111" } else { "#FFFFFF" }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

const FONTS: &str = "'Segoe UI', Ubuntu, Cantarell, 'Noto Sans', 'DejaVu Sans', sans-serif";

/// Every shape but pixelation as an SVG of the selection's logical size.
/// `caret` adds a cursor after the last text (while typing).
pub fn to_svg(shapes: &[Shape], width: f32, height: f32, caret: bool) -> String {
    let mut s = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#);
    let points = |pts: &[Pt]| pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect::<Vec<_>>().join(" ");
    let last = shapes.len().saturating_sub(1);
    for (i, shape) in shapes.iter().enumerate() {
        match shape {
            Shape::Arrow { from, to, color, width } => {
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                let (ux, uy) = (dx / len, dy / len);
                let head = (width * 4.).max(12.).min(len);
                let half = head * 0.55;
                let base = (to.0 - ux * head, to.1 - uy * head);
                let (lx, ly) = (base.0 - uy * half, base.1 + ux * half);
                let (rx, ry) = (base.0 + uy * half, base.1 - ux * half);
                let c = hex(*color);
                let _ = write!(
                    s,
                    r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{c}" stroke-width="{width}" stroke-linecap="round"/><polygon points="{:.1},{:.1} {lx:.1},{ly:.1} {rx:.1},{ry:.1}" fill="{c}" stroke="{c}" stroke-width="{}" stroke-linejoin="round"/>"#,
                    from.0,
                    from.1,
                    base.0 + ux * 0.5,
                    base.1 + uy * 0.5,
                    to.0,
                    to.1,
                    width / 2.,
                );
            }
            Shape::Rect { from, to, color, width } => {
                let _ = write!(
                    s,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="{}" stroke-width="{width}" stroke-linejoin="round"/>"#,
                    from.0.min(to.0),
                    from.1.min(to.1),
                    (to.0 - from.0).abs(),
                    (to.1 - from.1).abs(),
                    hex(*color),
                );
            }
            Shape::Pen { points: pts, color, width } => {
                let _ = write!(
                    s,
                    r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>"#,
                    points(pts),
                    hex(*color),
                );
            }
            Shape::Marker { points: pts, color, width } => {
                let _ = write!(
                    s,
                    r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="0.4" stroke-width="{}" stroke-linecap="square" stroke-linejoin="round"/>"#,
                    points(pts),
                    hex(*color),
                    width * 4. + 8.,
                );
            }
            Shape::Text { at, text, color, width } => {
                let size = text_size(*width);
                let shown = if caret && i == last { format!("{text}|") } else { text.clone() };
                let _ = write!(
                    s,
                    r#"<text x="{:.1}" y="{:.1}" font-family="{FONTS}" font-size="{size}" font-weight="600" fill="{}" xml:space="preserve">{}</text>"#,
                    at.0,
                    at.1 + size * 0.8,
                    hex(*color),
                    escape(&shown),
                );
            }
            Shape::Number { at, n, color, width } => {
                let r = badge_radius(*width);
                let _ = write!(
                    s,
                    r#"<circle cx="{:.1}" cy="{:.1}" r="{r}" fill="{}"/><text x="{:.1}" y="{:.1}" text-anchor="middle" font-family="{FONTS}" font-size="{}" font-weight="700" fill="{}">{n}</text>"#,
                    at.0,
                    at.1,
                    hex(*color),
                    at.0,
                    at.1 + r * 0.36,
                    r,
                    on_color(*color),
                );
            }
            Shape::Pixelate { .. } => {}
        }
    }
    s.push_str("</svg>");
    s
}

/// Averages `block`-sized squares inside the rectangle (physical px).
pub fn pixelate(rgba: &mut [u8], width: u32, height: u32, rect: (u32, u32, u32, u32), block: u32) {
    let (x0, y0) = (rect.0.min(width), rect.1.min(height));
    let (x1, y1) = ((rect.0 + rect.2).min(width), (rect.1 + rect.3).min(height));
    let block = block.max(1);
    let w = width as usize;
    let mut by = y0;
    while by < y1 {
        let bh = block.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = block.min(x1 - bx);
            let mut sum = [0u32; 3];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = (y as usize * w + x as usize) * 4;
                    for c in 0..3 {
                        sum[c] += rgba[i + c] as u32;
                    }
                }
            }
            let n = bw * bh;
            let avg = sum.map(|v| (v / n) as u8);
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = (y as usize * w + x as usize) * 4;
                    rgba[i..i + 3].copy_from_slice(&avg);
                }
            }
            bx += bw;
        }
        by += bh;
    }
}

fn fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// `base` with the shapes drawn on it. `scale` is physical px per logical px.
pub fn render(base: &Screenshot, shapes: &[Shape], scale: f32, caret: bool) -> Screenshot {
    let (w, h) = (base.width, base.height);
    let mut rgba = yaltri_platform::clipboard::bgra_to_rgba(&base.bgra);
    for shape in shapes {
        if let Shape::Pixelate { from, to } = shape {
            let p = |v: f32| (v.max(0.) * scale).round() as u32;
            let (x, y) = (p(from.0.min(to.0)), p(from.1.min(to.1)));
            let rect = (x, y, p(from.0.max(to.0)).saturating_sub(x), p(from.1.max(to.1)).saturating_sub(y));
            pixelate(&mut rgba, w, h, rect, (10. * scale).round() as u32);
        }
    }
    if shapes.iter().any(|s| !matches!(s, Shape::Pixelate { .. })) {
        let svg = to_svg(shapes, w as f32 / scale, h as f32 / scale, caret);
        let options = usvg::Options { fontdb: fonts(), ..Default::default() };
        match usvg::Tree::from_str(&svg, &options) {
            Ok(tree) => {
                // The capture is opaque, so straight and premultiplied RGBA agree.
                if let Some(mut pixmap) = tiny_skia::IntSize::from_wh(w, h).and_then(|size| tiny_skia::Pixmap::from_vec(rgba.clone(), size)) {
                    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
                    rgba = pixmap.take();
                }
            }
            Err(e) => tracing::warn!("drawing the markup failed: {e}"),
        }
    }
    for p in rgba.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    Screenshot { x: base.x, y: base.y, width: w, height: h, bgra: rgba }
}

/// The number the next badge gets.
pub fn next_number(shapes: &[Shape]) -> u32 {
    shapes.iter().filter_map(|s| if let Shape::Number { n, .. } = s { Some(*n) } else { None }).max().unwrap_or(0) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_keeps_arrows_on_45_degree_steps_and_boxes_square() {
        let (x, y) = snap_45((0., 0.), (10., 1.));
        assert!((x - 10.05).abs() < 0.01 && y.abs() < 0.01);
        let (x, y) = snap_45((0., 0.), (10., 9.));
        assert!((x - y).abs() < 0.01);
        assert_eq!(square((10., 10.), (0., 40.)), (-20., 40.));
    }

    #[test]
    fn clicks_with_drag_tools_draw_nothing() {
        let mut s = Shape::start(Tool::Rect, (5., 5.), 0, 2., 1);
        assert!(s.is_empty());
        s.drag_to((50., 40.), false);
        assert!(!s.is_empty());
        assert!(Shape::start(Tool::Pen, (1., 1.), 0, 2., 1).is_empty());
        assert!(!Shape::start(Tool::Number, (1., 1.), 0, 2., 1).is_empty());
    }

    #[test]
    fn numbers_count_up_from_the_highest() {
        let shapes = vec![
            Shape::start(Tool::Number, (0., 0.), 0, 2., 1),
            Shape::start(Tool::Arrow, (0., 0.), 0, 2., 0),
            Shape::start(Tool::Number, (0., 0.), 0, 2., 4),
        ];
        assert_eq!(next_number(&shapes), 5);
        assert_eq!(next_number(&[]), 1);
    }

    #[test]
    fn svg_escapes_text_and_skips_pixelation() {
        let shapes = vec![
            Shape::Pixelate { from: (0., 0.), to: (5., 5.) },
            Shape::Text { at: (1., 1.), text: "a<b & c".into(), color: 0xE5303A, width: 2. },
        ];
        let svg = to_svg(&shapes, 100., 50., true);
        assert!(svg.contains("a&lt;b &amp; c|"));
        assert!(svg.contains("#E5303A"));
        assert!(usvg::Tree::from_str(&svg, &usvg::Options::default()).is_ok());
    }

    #[test]
    fn pixelation_averages_blocks_inside_the_rect_only() {
        // 4x1: two blocks of 2 inside x 0..4, alpha untouched.
        let mut rgba = vec![0, 0, 0, 255, 100, 100, 100, 255, 10, 20, 30, 255, 30, 40, 50, 255];
        pixelate(&mut rgba, 4, 1, (0, 0, 2, 1), 2);
        assert_eq!(&rgba[..8], &[50, 50, 50, 255, 50, 50, 50, 255]);
        assert_eq!(&rgba[8..], &[10, 20, 30, 255, 30, 40, 50, 255]);
    }

    #[test]
    fn renders_shapes_onto_the_pixels() {
        let base = Screenshot { x: 0, y: 0, width: 20, height: 20, bgra: [255, 255, 255, 255].repeat(400) };
        let shapes = vec![Shape::Rect { from: (2., 2.), to: (18., 18.), color: 0xE5303A, width: 2. }];
        let out = render(&base, &shapes, 1., false);
        assert_eq!((out.width, out.height, out.bgra.len()), (20, 20, 1600));
        // On the left edge of the box: red (BGRA).
        let i = (10 * 20 + 2) * 4;
        assert_eq!(&out.bgra[i..i + 3], &[0x3A, 0x30, 0xE5]);
        // Inside: still white.
        let i = (10 * 20 + 10) * 4;
        assert_eq!(&out.bgra[i..i + 3], &[255, 255, 255]);
    }
}
