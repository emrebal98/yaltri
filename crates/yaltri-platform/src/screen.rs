//! Screen pixels for the color picker.
//!
//! - Windows / X11: one screenshot of the whole virtual desktop, in physical
//!   pixels; the picker shows it frozen and samples it.
//! - Wayland: clients may not read the screen. The compositor's own picker is
//!   used through the `PickColor` desktop portal instead.

use anyhow::Result;

/// BGRA pixels of a rectangle of the desktop, in physical pixels.
#[derive(Clone)]
pub struct Screenshot {
    /// Top-left corner in desktop coordinates (may be negative on Windows).
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Screenshot {
    /// `[r, g, b]` at desktop coordinates, if inside.
    pub fn pixel(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        let (lx, ly) = (x - self.x, y - self.y);
        if lx < 0 || ly < 0 || lx >= self.width as i32 || ly >= self.height as i32 {
            return None;
        }
        let i = (ly as usize * self.width as usize + lx as usize) * 4;
        let p = &self.bgra[i..i + 4];
        Some([p[2], p[1], p[0]])
    }

    /// Copy of the part inside the given desktop rectangle (clamped).
    pub fn crop(&self, x: i32, y: i32, width: u32, height: u32) -> Screenshot {
        let x0 = x.clamp(self.x, self.x + self.width as i32);
        let y0 = y.clamp(self.y, self.y + self.height as i32);
        let x1 = (x + width as i32).clamp(x0, self.x + self.width as i32);
        let y1 = (y + height as i32).clamp(y0, self.y + self.height as i32);
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for row in y0..y1 {
            let start = ((row - self.y) as usize * self.width as usize + (x0 - self.x) as usize) * 4;
            bgra.extend_from_slice(&self.bgra[start..start + w as usize * 4]);
        }
        Screenshot { x: x0, y: y0, width: w, height: h, bgra }
    }
}

/// Physical desktop position of a window's content area (`hwnd`'s client
/// origin). Window placement can differ from what was requested.
#[cfg(windows)]
pub fn client_origin(hwnd: isize) -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: `hwnd` is a live window handle owned by this process.
    let ok = unsafe { ClientToScreen(hwnd as _, &mut point) };
    (ok != 0).then_some((point.x, point.y))
}

/// Lets the system draw a popup's frame: rounded corners (with the system
/// shadow) and a 1 px border in `border` (0xRRGGBB). Windows 11; older
/// systems ignore the attributes.
#[cfg(windows)]
pub fn round_popup(hwnd: isize, border: u32) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    let corner = DWMWCP_ROUND;
    // COLORREF is 0x00BBGGRR.
    let colorref = ((border & 0xFF) << 16) | (border & 0xFF00) | ((border >> 16) & 0xFF);
    // SAFETY: `hwnd` is a live window of this process; the values are the
    // documented 4-byte types for these attributes.
    unsafe {
        DwmSetWindowAttribute(hwnd as _, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&corner as *const i32).cast(), 4);
        DwmSetWindowAttribute(hwnd as _, DWMWA_BORDER_COLOR as u32, (&colorref as *const u32).cast(), 4);
    }
}

/// How many "lines" one mouse-wheel notch scrolls, as GPUI reports wheel
/// deltas in lines: the Windows setting (default 3); on Linux GPUI's fixed
/// `SCROLL_LINES` (3 per notch on both X11 and Wayland).
pub fn wheel_lines_per_notch() -> f32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWHEELSCROLLLINES, SystemParametersInfoW};
        let mut lines: u32 = 3;
        // SAFETY: SPI_GETWHEELSCROLLLINES writes one u32.
        let ok = unsafe { SystemParametersInfoW(SPI_GETWHEELSCROLLLINES, 0, (&mut lines as *mut u32).cast(), 0) };
        // 0 disables scrolling; u32::MAX means "one page": treat both as 1.
        if ok != 0 && lines > 0 && lines != u32::MAX { lines as f32 } else { 1. }
    }
    #[cfg(not(windows))]
    {
        3.
    }
}

/// Captures the whole desktop. Fails on Wayland (use [`pick_color_portal`]).
pub fn capture() -> Result<Screenshot> {
    imp::capture()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::bail;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    pub fn capture() -> Result<Screenshot> {
        // SAFETY: plain GDI calls; every handle is released before returning.
        unsafe {
            let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            if w <= 0 || h <= 0 {
                bail!("no desktop to capture");
            }
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bitmap);
            let copied = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);

            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            SelectObject(mem, old);
            let lines = GetDIBits(mem, bitmap, 0, h as u32, bgra.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);

            DeleteObject(bitmap);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);

            if copied == 0 || lines == 0 {
                bail!("screen capture failed");
            }
            // GDI leaves the alpha byte undefined.
            for px in bgra.chunks_exact_mut(4) {
                px[3] = 255;
            }
            Ok(Screenshot { x, y, width: w as u32, height: h as u32, bgra })
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use anyhow::{Context, bail};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    /// Reads the X root window. Under XWayland this would only contain X
    /// clients, so callers use the portal on Wayland sessions instead.
    pub fn capture() -> Result<Screenshot> {
        let (conn, screen_num) = x11rb::connect(None).context("connecting to X server")?;
        let screen = &conn.setup().roots[screen_num];
        let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
        let reply = conn
            .get_image(ImageFormat::Z_PIXMAP, screen.root, 0, 0, w, h, !0)?
            .reply()
            .context("reading the screen")?;
        if reply.depth != 24 && reply.depth != 32 {
            bail!("unsupported screen depth {}", reply.depth);
        }
        let expected = w as usize * h as usize * 4;
        if reply.data.len() < expected {
            bail!("unexpected screen image size");
        }
        // 24/32-bit ZPixmap on little-endian servers is BGRX.
        let mut bgra = reply.data;
        bgra.truncate(expected);
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Ok(Screenshot { x: 0, y: 0, width: w as u32, height: h as u32, bgra })
    }
}

/// Asks the desktop's own color picker (xdg-desktop-portal `PickColor`).
/// Returns `None` if the user cancelled.
#[cfg(target_os = "linux")]
pub async fn pick_color_portal() -> Result<Option<[u8; 3]>> {
    use anyhow::Context;
    let request = ashpd::desktop::Color::pick().send().await.context("color picker portal unavailable")?;
    match request.response() {
        Ok(color) => {
            let to = |v: f64| (v.clamp(0., 1.) * 255.).round() as u8;
            Ok(Some([to(color.red()), to(color.green()), to(color.blue())]))
        }
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(e).context("picking a color"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Screenshot {
        // 3x2 desktop at (-1, 0); pixel value encodes its position.
        let mut bgra = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                bgra.extend_from_slice(&[x, y, 100, 255]);
            }
        }
        Screenshot { x: -1, y: 0, width: 3, height: 2, bgra }
    }

    #[test]
    fn pixel_uses_desktop_coordinates() {
        let s = sample();
        assert_eq!(s.pixel(-1, 0), Some([100, 0, 0]));
        assert_eq!(s.pixel(1, 1), Some([100, 1, 2]));
        assert_eq!(s.pixel(2, 0), None);
        assert_eq!(s.pixel(0, -1), None);
    }

    #[test]
    fn crop_keeps_coordinates_and_clamps() {
        let c = sample().crop(0, 1, 10, 10);
        assert_eq!((c.x, c.y, c.width, c.height), (0, 1, 2, 1));
        assert_eq!(c.pixel(1, 1), Some([100, 1, 2]));
    }

    #[cfg(windows)]
    #[test]
    fn captures_the_real_desktop() {
        let s = capture().unwrap();
        assert!(s.width > 0 && s.height > 0);
        assert_eq!(s.bgra.len(), (s.width * s.height * 4) as usize);
        assert!(s.pixel(s.x, s.y).is_some());
    }
}
