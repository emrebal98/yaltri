//! The picker's magnifier, modelled on PowerToys v0.99 `ZoomWindowHelper`,
//! `GridEffectBehavior` and `GridShader.fx` (MIT License, Copyright (c)
//! Microsoft Corporation):
//!
//! - the first scroll-up freezes a 50x50 physical-pixel patch around the
//!   cursor; the square stays centered on that point
//! - four levels, factor 1x/2x/4x/8x, so the square is 50-400 logical px
//! - from 4x on (levels 3 and 4) the pointed pixel gets a thin outline and
//!   a grid shows in a tiny circle around the pointer (radius 4% of the
//!   square): grid pixels blend towards white, or black on near-white
//!   pixels, strongest at the pointer
//! - the pixel under the pointer is what gets shown and picked

use yaltri_color::Rgb;
use yaltri_platform::screen::Screenshot;

/// Captured patch side, in physical pixels.
pub const BASE: usize = 50;
pub const MAX_LEVEL: usize = 4;
/// The grid and outline appear from this magnification on.
const OVERLAY_MIN_FACTOR: f32 = 4.;
/// Grid circle radius as a fraction of the square's side (shader `radius`).
const GRID_RADIUS_FRACTION: f32 = 0.04;

/// Magnification at `level` (1..=MAX_LEVEL).
pub fn factor(level: usize) -> f32 {
    (1u32 << (level.clamp(1, MAX_LEVEL) - 1)) as f32
}

/// Logical side length of the square at `level`.
pub fn side(level: usize) -> f32 {
    BASE as f32 * factor(level)
}

/// The frozen patch.
pub struct Capture {
    pixels: Vec<Rgb>,
}

impl Capture {
    /// `BASE`x`BASE` pixels centered on desktop pixel (`cx`, `cy`); outside
    /// the screen reads as black.
    pub fn from_shot(shot: &Screenshot, cx: i32, cy: i32) -> Self {
        let half = (BASE / 2) as i32;
        let mut pixels = Vec::with_capacity(BASE * BASE);
        for y in 0..BASE as i32 {
            for x in 0..BASE as i32 {
                let [r, g, b] = shot.pixel(cx - half + x, cy - half + y).unwrap_or([0, 0, 0]);
                pixels.push(Rgb::new(r, g, b));
            }
        }
        Self { pixels }
    }

    pub fn at(&self, col: usize, row: usize) -> Rgb {
        self.pixels[row.min(BASE - 1) * BASE + col.min(BASE - 1)]
    }
}

/// Cell under a point given relative to the square's top-left, if inside.
pub fn cell_at(x: f32, y: f32, side: f32) -> Option<(usize, usize)> {
    if x < 0. || y < 0. || x >= side || y >= side {
        return None;
    }
    let cell = side / BASE as f32;
    Some((((x / cell) as usize).min(BASE - 1), ((y / cell) as usize).min(BASE - 1)))
}

/// BGRA image of the patch at `physical_side` pixels, nearest-neighbor.
/// Rendered once per zoom level.
pub fn render(capture: &Capture, physical_side: u32) -> Vec<u8> {
    let n = physical_side as usize;
    let cell = physical_side as f32 / BASE as f32;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        let row = ((y as f32 / cell) as usize).min(BASE - 1);
        for x in 0..n {
            let col = ((x as f32 / cell) as usize).min(BASE - 1);
            let c = capture.at(col, row);
            let i = (y * n + x) * 4;
            out[i..i + 4].copy_from_slice(&[c.b, c.g, c.r, 255]);
        }
    }
    out
}

/// One grid pixel: square-relative logical position, line color and how
/// strongly it covers the image pixel (1 at the pointer, 0 at the radius).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridPixel {
    pub x: i32,
    pub y: i32,
    /// White line, or black on near-white image pixels.
    pub white: bool,
    pub strength: f32,
}

/// Outline around the pointed pixel: top-left, outer side, color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub x: i32,
    pub y: i32,
    pub side: i32,
    pub white: bool,
}

/// What to draw over the magnified image for a pointer at `pointer`
/// (square-relative logical px). `None` below 4x or outside the square.
pub fn pointer_overlay(capture: &Capture, level: usize, pointer: (f32, f32)) -> Option<(Outline, Vec<GridPixel>)> {
    let factor = factor(level);
    let side = BASE as f32 * factor;
    if factor < OVERLAY_MIN_FACTOR || pointer.0 < 0. || pointer.1 < 0. || pointer.0 >= side || pointer.1 >= side {
        return None;
    }
    let square = factor as i32; // cell size in px
    let near_white = |x: i32, y: i32| {
        let c = capture.at((x / square).clamp(0, BASE as i32 - 1) as usize, (y / square).clamp(0, BASE as i32 - 1) as usize);
        c.r > 204 && c.g > 204 && c.b > 204
    };
    let (mx, my) = (pointer.0 as i32, pointer.1 as i32);
    // Shader: topLeftRectangle = mouse - mouse % squareSize - 1.
    let (tx, ty) = (mx - mx % square - 1, my - my % square - 1);
    let outline = Outline { x: tx, y: ty, side: square + 3, white: !near_white(mx, my) };

    // The ring between the outline and the cell keeps the image as is.
    let in_inner_ring = |x: i32, y: i32| {
        let (a, b) = (tx + 1, tx + square + 1);
        let (c, d) = (ty + 1, ty + square + 1);
        ((a..=b).contains(&x) && (y == c || y == d)) || ((c..=d).contains(&y) && (x == a || x == b))
    };
    let on_outline = |x: i32, y: i32| {
        let (a, b) = (tx, tx + square + 2);
        let (c, d) = (ty, ty + square + 2);
        ((a..=b).contains(&x) && (y == c || y == d)) || ((c..=d).contains(&y) && (x == a || x == b))
    };

    let radius = GRID_RADIUS_FRACTION * side;
    let reach = radius.ceil() as i32;
    let mut grid = Vec::new();
    for y in (my - reach).max(0)..=(my + reach).min(side as i32 - 1) {
        for x in (mx - reach).max(0)..=(mx + reach).min(side as i32 - 1) {
            if x % square != 0 && y % square != 0 {
                continue;
            }
            if in_inner_ring(x, y) || on_outline(x, y) {
                continue;
            }
            let d = (((x - mx) * (x - mx) + (y - my) * (y - my)) as f32).sqrt();
            if d > radius {
                continue;
            }
            grid.push(GridPixel { x, y, white: !near_white(x, y), strength: 1. - d / radius });
        }
    }
    Some((outline, grid))
}

/// Eased size during the 200 ms resize between levels: sine ease-out when
/// growing, quadratic ease-in when shrinking (as PowerToys does).
pub fn eased(from: f32, to: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let k = if to >= from { (t * std::f32::consts::FRAC_PI_2).sin() } else { t * t };
    from + (to - from) * k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> Screenshot {
        // 200x200 desktop; red = x, green = y, so every pixel is unique.
        let mut bgra = Vec::new();
        for y in 0..200u32 {
            for x in 0..200u32 {
                bgra.extend_from_slice(&[0, y as u8, x as u8, 255]);
            }
        }
        Screenshot { x: 0, y: 0, width: 200, height: 200, bgra }
    }

    #[test]
    fn levels_match_powertoys() {
        assert_eq!([side(1), side(2), side(3), side(4)], [50., 100., 200., 400.]);
    }

    #[test]
    fn capture_is_centered_on_the_point() {
        let c = Capture::from_shot(&shot(), 100, 80);
        // Cell (25, 25) is the center pixel.
        assert_eq!(c.at(25, 25), Rgb::new(100, 80, 0));
        assert_eq!(c.at(0, 0), Rgb::new(75, 55, 0));
    }

    #[test]
    fn pointer_maps_to_cells() {
        let s = side(4); // 8 px cells
        assert_eq!(cell_at(0., 0., s), Some((0, 0)));
        assert_eq!(cell_at(15.9, 8., s), Some((1, 1)));
        assert_eq!(cell_at(399.9, 399.9, s), Some((49, 49)));
        assert_eq!(cell_at(400., 10., s), None);
        assert_eq!(cell_at(-1., 10., s), None);
    }

    #[test]
    fn render_is_nearest_neighbor() {
        let c = Capture::from_shot(&shot(), 100, 100);
        let side = 400; // 8 px cells
        let img = render(&c, side);
        let at = |x: usize, y: usize| {
            let i = (y * side as usize + x) * 4;
            [img[i + 2], img[i + 1], img[i]]
        };
        assert_eq!(at(200, 200), [100, 100, 0]);
        assert_eq!(at(207, 207), [100, 100, 0], "same cell");
        assert_eq!(at(208, 200), [101, 100, 0], "next cell");
    }

    #[test]
    fn nothing_drawn_below_4x() {
        let c = Capture::from_shot(&shot(), 100, 100);
        assert!(pointer_overlay(&c, 1, (20., 20.)).is_none());
        assert!(pointer_overlay(&c, 2, (20., 20.)).is_none());
        assert!(pointer_overlay(&c, 3, (20., 20.)).is_some());
        assert!(pointer_overlay(&c, 4, (20., 20.)).is_some());
        assert!(pointer_overlay(&c, 4, (400., 20.)).is_none(), "outside the square");
    }

    #[test]
    fn outline_frames_the_pointed_cell_one_px_out() {
        let c = Capture::from_shot(&shot(), 100, 100);
        // Level 4: 8 px cells; pointer inside cell (2, 3).
        let (outline, _) = pointer_overlay(&c, 4, (21., 29.)).unwrap();
        assert_eq!((outline.x, outline.y, outline.side), (15, 23, 11));
        assert!(outline.white, "dark pixel -> white outline");
    }

    #[test]
    fn grid_is_a_tiny_circle_strongest_at_the_pointer() {
        let c = Capture::from_shot(&shot(), 100, 100);
        let pointer = (204., 196.);
        let (_, grid) = pointer_overlay(&c, 4, pointer).unwrap();
        let radius = 0.04 * 400.; // 16 px
        assert!(!grid.is_empty());
        for g in &grid {
            let d = (((g.x as f32 - pointer.0).powi(2)) + ((g.y as f32 - pointer.1).powi(2))).sqrt();
            assert!(d <= radius + 0.01, "{g:?} outside radius");
            assert!(g.x % 8 == 0 || g.y % 8 == 0, "{g:?} not on a grid line");
        }
        // The outline and the ring inside it stay free of grid lines.
        let (outline, _) = pointer_overlay(&c, 4, pointer).unwrap();
        let inside = |g: &GridPixel| {
            (outline.x..outline.x + outline.side).contains(&g.x) && (outline.y..outline.y + outline.side).contains(&g.y)
        };
        assert!(grid.iter().all(|g| !inside(g)), "grid inside the outline box");
        // Fades out: strength drops with distance.
        let near = grid.iter().map(|g| g.strength).fold(0., f32::max);
        let far = grid.iter().map(|g| g.strength).fold(1., f32::min);
        assert!(near > far);
        // Level 3: half the radius.
        let (_, small) = pointer_overlay(&c, 3, (102., 98.)).unwrap();
        assert!(small.iter().all(|g| ((g.x - 102).pow(2) + (g.y - 98).pow(2)) as f32 <= 64.01));
    }

    #[test]
    fn easing_hits_both_ends() {
        assert_eq!(eased(50., 100., 0.), 50.);
        assert_eq!(eased(50., 100., 1.), 100.);
        assert_eq!(eased(400., 200., 1.), 200.);
        assert!(eased(50., 100., 0.5) > 75., "ease-out grows fast first");
    }
}
