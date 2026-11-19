//! X11 window fixes GPUI does not make.
//!
//! - Focus: GPUI maps `PopUp` windows override-redirect. The window manager
//!   never focuses those and ignores `_NET_ACTIVE_WINDOW` for them, so the
//!   launcher got no key events. We set the input focus ourselves, like
//!   dmenu and rofi do.
//! - Rounded corners: per-pixel transparency needs the GPU driver to offer a
//!   premultiplied-alpha swapchain. NVIDIA's X11 Vulkan driver offers only
//!   opaque, so the corners outside our rounded frame showed black. Shaping
//!   the window to a rounded rectangle cuts them away on every driver.

use std::time::Duration;

use anyhow::{Context, Result};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ClipOrdering, ConnectionExt as _, InputFocus, MapState, Rectangle, Window};

/// Gives keyboard focus to `window` once it is mapped. Runs on its own
/// thread: the window may not be viewable yet when this is called.
pub fn focus_window(window: u32) {
    std::thread::spawn(move || {
        if let Err(e) = focus_when_mapped(window) {
            tracing::warn!("focusing X11 window failed: {e:#}");
        }
    });
}

fn focus_when_mapped(window: Window) -> Result<()> {
    let (conn, _) = x11rb::connect(None).context("connecting to X server")?;
    for _ in 0..100 {
        let attrs = conn.get_window_attributes(window)?.reply().context("reading window state")?;
        if attrs.map_state == MapState::VIEWABLE {
            conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)?.check()?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    anyhow::bail!("window never became viewable")
}

/// Clips `window` (`width`×`height` physical px) to a rounded rectangle.
pub fn round_window(window: u32, width: u16, height: u16, radius: u16) -> Result<()> {
    let (conn, _) = x11rb::connect(None).context("connecting to X server")?;
    conn.shape_rectangles(
        shape::SO::SET,
        shape::SK::BOUNDING,
        ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &rounded_rect(width, height, radius),
    )?
    .check()
    .context("shaping window")?;
    Ok(())
}

/// Rectangles covering a `width`×`height` rounded rectangle, one row per
/// corner scanline plus one for the straight middle.
pub(crate) fn rounded_rect(width: u16, height: u16, radius: u16) -> Vec<Rectangle> {
    let r = radius.min(width / 2).min(height / 2);
    let mut rects = Vec::with_capacity(2 * r as usize + 1);
    for row in 0..r {
        // Inset where the arc crosses this row's pixel centers.
        let dy = r as f32 - row as f32 - 0.5;
        let inset = (r as f32 - (r as f32 * r as f32 - dy * dy).sqrt()).round() as u16;
        let width = width - 2 * inset;
        rects.push(Rectangle { x: inset as i16, y: row as i16, width, height: 1 });
        rects.push(Rectangle { x: inset as i16, y: (height - 1 - row) as i16, width, height: 1 });
    }
    rects.push(Rectangle { x: 0, y: r as i16, width, height: height - 2 * r });
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(rects: &[Rectangle], x: i16, y: i16) -> bool {
        rects.iter().any(|r| x >= r.x && x < r.x + r.width as i16 && y >= r.y && y < r.y + r.height as i16)
    }

    #[test]
    fn cuts_the_corners_and_keeps_the_rest() {
        let rects = rounded_rect(100, 60, 10);
        for (x, y) in [(0, 0), (99, 0), (0, 59), (99, 59), (1, 1)] {
            assert!(!covers(&rects, x, y), "corner pixel ({x},{y}) kept");
        }
        for (x, y) in [(50, 0), (0, 30), (99, 30), (50, 59), (10, 10), (89, 49)] {
            assert!(covers(&rects, x, y), "pixel ({x},{y}) cut");
        }
    }

    #[test]
    fn is_symmetric() {
        let rects = rounded_rect(80, 40, 8);
        for y in 0..40 {
            for x in 0..80 {
                assert_eq!(covers(&rects, x, y), covers(&rects, 79 - x, y), "x-mirror at ({x},{y})");
                assert_eq!(covers(&rects, x, y), covers(&rects, x, 39 - y), "y-mirror at ({x},{y})");
            }
        }
    }

    #[test]
    fn zero_radius_is_the_whole_window() {
        let rects = rounded_rect(30, 20, 0);
        assert!((0..20).all(|y| (0..30).all(|x| covers(&rects, x, y))));
    }

    #[test]
    fn radius_is_clamped_to_half_the_short_side() {
        let rects = rounded_rect(40, 10, 50);
        assert!(covers(&rects, 20, 5));
        assert!(!covers(&rects, 0, 0));
    }
}

/// Where the pointer is and the part of its monitor windows may use, in
/// physical px of the X screen (which spans every monitor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerArea {
    pub pointer: (i32, i32),
    /// x, y, width, height: the monitor under the pointer minus the panels
    /// (`_NET_WORKAREA`).
    pub area: (i32, i32, i32, i32),
    /// Width of the whole X screen, to turn these into GPUI's logical px.
    pub screen_width: u16,
}

/// The pointer and its monitor's work area: a tray popup opens there, by
/// the panel the user just clicked.
pub fn pointer_area() -> Result<PointerArea> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::AtomEnum;

    let (conn, screen) = x11rb::connect(None).context("connecting to X server")?;
    let root = &conn.setup().roots[screen];
    let (root_win, screen_width, screen_height) = (root.root, root.width_in_pixels, root.height_in_pixels);
    let ptr = conn.query_pointer(root_win)?.reply().context("reading the pointer")?;
    let pointer = (i32::from(ptr.root_x), i32::from(ptr.root_y));

    let monitors = conn.randr_get_monitors(root_win, true)?.reply().context("listing monitors")?.monitors;
    let monitor = monitors
        .iter()
        .map(|m| (i32::from(m.x), i32::from(m.y), i32::from(m.width), i32::from(m.height)))
        .find(|&(x, y, w, h)| (x..x + w).contains(&pointer.0) && (y..y + h).contains(&pointer.1))
        .unwrap_or((0, 0, i32::from(screen_width), i32::from(screen_height)));

    // The window manager's work area of the first desktop; it spans every
    // monitor, so cut it to this one.
    let atom = conn.intern_atom(false, b"_NET_WORKAREA")?.reply()?.atom;
    let work = conn
        .get_property(false, root_win, atom, AtomEnum::CARDINAL, 0, 4)?
        .reply()
        .ok()
        .and_then(|r| r.value32().map(|v| v.map(|n| n as i32).collect::<Vec<_>>()))
        .filter(|v| v.len() == 4);
    let area = match work {
        Some(w) => intersect(monitor, (w[0], w[1], w[2], w[3])).unwrap_or(monitor),
        None => monitor,
    };
    Ok(PointerArea { pointer, area, screen_width })
}

fn intersect(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}
