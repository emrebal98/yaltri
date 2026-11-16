//! Where the other apps' windows are, for picking one to screenshot.
//!
//! Windows and X11 only: Wayland does not tell clients where other windows
//! are, so the list is empty there.

/// A window's visible frame in physical desktop pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WinRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width as i32 && y < self.y + self.height as i32
    }

    /// From edges; `None` when empty.
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    fn from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Option<Self> {
        (right > left && bottom > top).then(|| Self {
            x: left,
            y: top,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        })
    }
}

/// Visible windows of other apps, topmost first.
pub fn visible_windows() -> Vec<WinRect> {
    #[cfg(windows)]
    return win::visible_windows();
    #[cfg(target_os = "linux")]
    return x11::visible_windows().unwrap_or_else(|e| {
        tracing::debug!("listing X11 windows failed: {e:#}");
        Vec::new()
    });
    #[cfg(not(any(windows, target_os = "linux")))]
    Vec::new()
}

/// The topmost window containing the point.
pub fn window_at(windows: &[WinRect], x: i32, y: i32) -> Option<WinRect> {
    windows.iter().copied().find(|w| w.contains(x, y))
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
    };

    use super::WinRect;

    pub fn visible_windows() -> Vec<WinRect> {
        let mut out: Vec<WinRect> = Vec::new();
        // SAFETY: the callback only runs during this call and gets a pointer
        // to `out`, which outlives it.
        unsafe { EnumWindows(Some(collect), &mut out as *mut Vec<WinRect> as LPARAM) };
        out
    }

    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> i32 {
        // SAFETY: `data` is the `Vec` passed by `visible_windows`.
        let out = unsafe { &mut *(data as *mut Vec<WinRect>) };
        if let Some(rect) = unsafe { frame(hwnd) } {
            out.push(rect);
        }
        1
    }

    /// The window's frame, if it is one a user would pick.
    unsafe fn frame(hwnd: HWND) -> Option<WinRect> {
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return None;
            }
            let mut cloaked = 0u32;
            let size = std::mem::size_of::<u32>() as u32;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut c_void, size) == 0
                && cloaked != 0
            {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == std::process::id() {
                return None;
            }
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0;
            if tool && GetWindowTextLengthW(hwnd) == 0 {
                return None;
            }
            let mut class = [0u16; 64];
            let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
            if crate::pin::is_shell_class(&String::from_utf16_lossy(&class[..len.max(0) as usize])) {
                return None;
            }
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let size = std::mem::size_of::<RECT>() as u32;
            // The frame without the invisible resize border.
            if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, &mut r as *mut RECT as *mut c_void, size)
                != 0
                && GetWindowRect(hwnd, &mut r) == 0
            {
                return None;
            }
            WinRect::from_edges(r.left, r.top, r.right, r.bottom)
        }
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use anyhow::{Context, Result};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState};

    use super::WinRect;

    pub fn visible_windows() -> Result<Vec<WinRect>> {
        let (conn, screen) = x11rb::connect(None).context("connecting to X server")?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| -> Result<u32> { Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom) };
        let cardinals = |window: u32, prop: u32, ty: AtomEnum, len: u32| -> Vec<u32> {
            conn.get_property(false, window, prop, ty, 0, len)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32().map(Iterator::collect))
                .unwrap_or_default()
        };

        let stacking = cardinals(root, atom("_NET_CLIENT_LIST_STACKING")?, AtomEnum::WINDOW, 4096);
        let (wm_type, desktop, dock) =
            (atom("_NET_WM_WINDOW_TYPE")?, atom("_NET_WM_WINDOW_TYPE_DESKTOP")?, atom("_NET_WM_WINDOW_TYPE_DOCK")?);
        let (pid_atom, frame_atom, gtk_atom) = (atom("_NET_WM_PID")?, atom("_NET_FRAME_EXTENTS")?, atom("_GTK_FRAME_EXTENTS")?);

        let mut out = Vec::new();
        // Bottom to top in the list; we want topmost first.
        for window in stacking.into_iter().rev() {
            let Ok(attrs) = conn.get_window_attributes(window)?.reply() else { continue };
            if attrs.map_state != MapState::VIEWABLE {
                continue;
            }
            let types = cardinals(window, wm_type, AtomEnum::ATOM, 16);
            if types.iter().any(|t| *t == desktop || *t == dock) {
                continue;
            }
            if cardinals(window, pid_atom, AtomEnum::CARDINAL, 1).first() == Some(&std::process::id()) {
                continue;
            }
            let Ok(geometry) = conn.get_geometry(window)?.reply() else { continue };
            let Ok(origin) = conn.translate_coordinates(window, root, 0, 0)?.reply() else { continue };
            let (mut left, mut top) = (origin.dst_x as i32, origin.dst_y as i32);
            let (mut right, mut bottom) = (left + geometry.width as i32, top + geometry.height as i32);
            // Server-side decorations around the client…
            if let [l, r, t, b] = cardinals(window, frame_atom, AtomEnum::CARDINAL, 4)[..] {
                (left, right, top, bottom) = (left - l as i32, right + r as i32, top - t as i32, bottom + b as i32);
            }
            // …or a client-side shadow inside it.
            if let [l, r, t, b] = cardinals(window, gtk_atom, AtomEnum::CARDINAL, 4)[..] {
                (left, right, top, bottom) = (left + l as i32, right - r as i32, top + t as i32, bottom - b as i32);
            }
            out.extend(WinRect::from_edges(left, top, right, bottom));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_topmost_window_under_the_point() {
        let back = WinRect { x: 0, y: 0, width: 100, height: 100 };
        let front = WinRect { x: 50, y: 50, width: 100, height: 100 };
        let list = [front, back];
        assert_eq!(window_at(&list, 60, 60), Some(front));
        assert_eq!(window_at(&list, 10, 10), Some(back));
        assert_eq!(window_at(&list, 149, 149), Some(front));
        assert_eq!(window_at(&list, 150, 150), None);
    }

    #[test]
    fn empty_edges_give_no_rect() {
        assert_eq!(WinRect::from_edges(5, 5, 5, 10), None);
        assert_eq!(WinRect::from_edges(-10, 0, 10, 4), Some(WinRect { x: -10, y: 0, width: 20, height: 4 }));
    }
}
