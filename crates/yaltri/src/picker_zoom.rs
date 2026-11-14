//! The picker's magnifier, modelled on PowerToys' `ZoomWindowHelper` /
//! `ZoomView` (MIT License, Copyright (c) Microsoft Corporation):
//!
//! - the first scroll-up freezes a 50x50 physical-pixel patch around the
//!   cursor; the square stays centered on that point
//! - four levels, factor 1x/2x/4x/8x, so the square is 50-400 logical px
//! - once a cell is at least 4 px, a pixel grid is drawn whose lines are
//!   black on light pixels, white on dark ones, fading towards the edges
//! - the cell under the pointer is outlined and its color is what gets shown
//!   and picked

use yaltri_color::Rgb;
use yaltri_platform::screen::Screenshot;

/// Captured patch side, in physical pixels.
pub const BASE: usize = 50;
pub const MAX_LEVEL: usize = 4;
/// Grid lines appear once a cell is this many logical pixels wide.
const GRID_MIN_CELL: f32 = 4.;

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

fn is_light(c: Rgb) -> bool {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32 > 140.
}

/// BGRA image of the patch at `physical_side` pixels, nearest-neighbor, with
/// the grid baked in when `grid` is set. Rendered once per zoom level.
pub fn render(capture: &Capture, physical_side: u32, grid: bool) -> Vec<u8> {
    let n = physical_side as usize;
    let cell = physical_side as f32 / BASE as f32;
    let center = physical_side as f32 / 2.;
    let max_dist = (2f32).sqrt() * center;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        let row = ((y as f32 / cell) as usize).min(BASE - 1);
        let on_row_edge = grid && y == (row as f32 * cell).floor() as usize;
        for x in 0..n {
            let col = ((x as f32 / cell) as usize).min(BASE - 1);
            let c = capture.at(col, row);
            let mut px = [c.b as f32, c.g as f32, c.r as f32];
            if grid && (on_row_edge || x == (col as f32 * cell).floor() as usize) {
                let (mx, my) = ((col as f32 + 0.5) * cell, (row as f32 + 0.5) * cell);
                let fade = 1. - ((mx - center).powi(2) + (my - center).powi(2)).sqrt() / max_dist;
                if fade > 0.05 {
                    let alpha = fade.clamp(0., 1.) * 160. / 255.;
                    let line = if is_light(c) { 0. } else { 255. };
                    for ch in &mut px {
                        *ch = *ch * (1. - alpha) + line * alpha;
                    }
                }
            }
            let i = (y * n + x) * 4;
            out[i..i + 4].copy_from_slice(&[px[0] as u8, px[1] as u8, px[2] as u8, 255]);
        }
    }
    out
}

/// Whether the grid is drawn at `level`.
pub fn has_grid(level: usize) -> bool {
    side(level) / BASE as f32 >= GRID_MIN_CELL
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
        assert!(!has_grid(1) && !has_grid(2));
        assert!(has_grid(3) && has_grid(4));
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
    fn render_is_nearest_neighbor_with_grid_on_edges() {
        let c = Capture::from_shot(&shot(), 100, 100);
        let side = 400; // 8 px cells
        let plain = render(&c, side, false);
        let gridded = render(&c, side, true);
        let at = |img: &[u8], x: usize, y: usize| {
            let i = (y * side as usize + x) * 4;
            [img[i + 2], img[i + 1], img[i]]
        };
        // Inside a cell: exactly the source pixel, with or without grid.
        assert_eq!(at(&plain, 203, 203), [100, 100, 0]);
        assert_eq!(at(&gridded, 203, 203), [100, 100, 0]);
        // A cell's top-left edge pixel is tinted by the grid near the center.
        assert_ne!(at(&gridded, 200, 203), at(&plain, 200, 203));
    }

    #[test]
    fn easing_hits_both_ends() {
        assert_eq!(eased(50., 100., 0.), 50.);
        assert_eq!(eased(50., 100., 1.), 100.);
        assert_eq!(eased(400., 200., 1.), 200.);
        assert!(eased(50., 100., 0.5) > 75., "ease-out grows fast first");
    }
}
