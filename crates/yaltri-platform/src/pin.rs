//! Always on top for the focused window.
//!
//! Where the desktop has its own "keep above" action, it does the work:
//! GNOME runs it from a Mutter keybinding (`gnome_keys` sets it up), so the
//! key never reaches us there. Elsewhere we get the shortcut and change the
//! window ourselves: Win32 on Windows, EWMH on X11, a KWin script on KDE
//! Wayland and `hyprctl` on Hyprland.

use anyhow::Result;

/// Toggles always-on-top for the window that has focus. Pinned windows stay
/// pinned after Yaltri quits.
pub fn toggle_focused() -> Result<()> {
    #[cfg(windows)]
    return win::toggle();
    #[cfg(target_os = "linux")]
    return linux::toggle();
    #[cfg(not(any(windows, target_os = "linux")))]
    anyhow::bail!("always on top is not supported on this system")
}

/// Desktop windows a pin or a window screenshot makes no sense on: the
/// wallpaper and taskbars.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_shell_class(class: &str) -> bool {
    matches!(class, "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

#[cfg(windows)]
mod win {
    use anyhow::{Result, bail};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowThreadProcessId, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WS_EX_TOPMOST,
    };

    pub fn toggle() -> Result<()> {
        // SAFETY: plain Win32 calls on a window handle the system gave us;
        // a stale handle only makes them fail.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return Ok(());
            }
            let mut class = [0u16; 64];
            let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
            let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
            if super::is_shell_class(&class) {
                tracing::debug!(class, "pin: desktop or taskbar has focus, ignored");
                return Ok(());
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == std::process::id() {
                tracing::debug!("pin: one of our own windows has focus, ignored");
                return Ok(());
            }
            let pinned = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
            let after = if pinned { HWND_NOTOPMOST } else { HWND_TOPMOST };
            if SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) == 0 {
                // Typically an elevated window: Windows does not let a normal
                // process change it.
                bail!("changing always on top of {class} failed (error {})", GetLastError());
            }
            tracing::info!(class, pinned = !pinned, "pin toggled");
            Ok(())
        }
    }
}

/// Which way this Linux desktop keeps a window above the others.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    /// Mutter's own keybinding does it; nothing to do here.
    Gnome,
    Hyprland,
    /// KDE Plasma on Wayland: a KWin script.
    Kwin,
    /// Any X11 window manager: EWMH `_NET_WM_STATE_ABOVE`.
    X11,
    Unsupported,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_backend(wayland: bool, desktop: &str, hyprland: bool) -> Backend {
    let desktops: Vec<String> = desktop.split(':').map(str::to_ascii_lowercase).collect();
    let is = |name: &str| desktops.iter().any(|d| d == name);
    if is("gnome") || is("unity") || is("ubuntu") {
        Backend::Gnome
    } else if hyprland {
        Backend::Hyprland
    } else if !wayland {
        Backend::X11
    } else if is("kde") {
        Backend::Kwin
    } else {
        Backend::Unsupported
    }
}

/// The id in `dbus-send --print-reply` output (`   int32 7`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_dbus_int(out: &str) -> Option<i32> {
    out.lines().find_map(|l| l.trim().strip_prefix("int32 ")?.trim().parse().ok())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use anyhow::{Context, Result, bail};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask};

    use super::Backend;

    pub fn toggle() -> Result<()> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let hyprland = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();
        match super::linux_backend(wayland, &desktop, hyprland) {
            Backend::Gnome => {
                tracing::debug!("pin: GNOME handles the shortcut itself");
                Ok(())
            }
            Backend::Hyprland => hyprland_pin(),
            Backend::Kwin => kwin_keep_above(),
            Backend::X11 => x11_keep_above(),
            Backend::Unsupported => bail!("always on top is not supported on this desktop ({desktop})"),
        }
    }

    fn hyprland_pin() -> Result<()> {
        let out = Command::new("hyprctl").args(["dispatch", "pin", "active"]).output().context("running hyprctl")?;
        let text = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || text.trim() != "ok" {
            bail!("hyprctl dispatch pin: {}", text.trim());
        }
        Ok(())
    }

    const KWIN_SCRIPT: &str = "const w = workspace.activeWindow || workspace.activeClient;\nif (w) w.keepAbove = !w.keepAbove;\n";
    const KWIN_NAME: &str = "yaltri-pin";

    fn kwin_keep_above() -> Result<()> {
        let script = std::env::temp_dir().join("yaltri-pin.js");
        std::fs::write(&script, KWIN_SCRIPT).with_context(|| format!("writing {}", script.display()))?;
        // KWin refuses a second script with the same name.
        let _ = kwin(&["/Scripting", "org.kde.kwin.Scripting.unloadScript", &format!("string:{KWIN_NAME}")]);
        let out = kwin(&[
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
            &format!("string:{}", script.display()),
            &format!("string:{KWIN_NAME}"),
        ])?;
        let id = super::parse_dbus_int(&out).filter(|id| *id >= 0).context("KWin did not load the script")?;
        // Plasma 6 path first, then Plasma 5.
        let run = kwin(&[&format!("/Scripting/Script{id}"), "org.kde.kwin.Script.run"])
            .or_else(|_| kwin(&[&format!("/{id}"), "org.kde.kwin.Script.run"]));
        let _ = kwin(&["/Scripting", "org.kde.kwin.Scripting.unloadScript", &format!("string:{KWIN_NAME}")]);
        run.map(|_| ())
    }

    fn kwin(args: &[&str]) -> Result<String> {
        let out = Command::new("dbus-send")
            .args(["--session", "--print-reply", "--dest=org.kde.KWin"])
            .args(args)
            .output()
            .context("running dbus-send")?;
        if !out.status.success() {
            bail!("dbus-send {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn x11_keep_above() -> Result<()> {
        let (conn, screen) = x11rb::connect(None).context("connecting to X server")?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| -> Result<u32> { Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom) };
        let first = |window: u32, prop: u32, ty: AtomEnum| -> Result<Option<u32>> {
            let reply = conn.get_property(false, window, prop, ty, 0, 1)?.reply()?;
            Ok(reply.value32().and_then(|mut v| v.next()))
        };

        let Some(window) = first(root, atom("_NET_ACTIVE_WINDOW")?, AtomEnum::WINDOW)?.filter(|w| *w != 0) else {
            return Ok(());
        };
        let types: Vec<u32> = conn
            .get_property(false, window, atom("_NET_WM_WINDOW_TYPE")?, AtomEnum::ATOM, 0, 16)?
            .reply()?
            .value32()
            .map(Iterator::collect)
            .unwrap_or_default();
        let (desktop, dock) = (atom("_NET_WM_WINDOW_TYPE_DESKTOP")?, atom("_NET_WM_WINDOW_TYPE_DOCK")?);
        if types.iter().any(|t| *t == desktop || *t == dock) {
            tracing::debug!("pin: desktop or dock has focus, ignored");
            return Ok(());
        }
        if first(window, atom("_NET_WM_PID")?, AtomEnum::CARDINAL)? == Some(std::process::id()) {
            tracing::debug!("pin: one of our own windows has focus, ignored");
            return Ok(());
        }

        const TOGGLE: u32 = 2;
        const FROM_APP: u32 = 1;
        let event = ClientMessageEvent::new(
            32,
            window,
            atom("_NET_WM_STATE")?,
            [TOGGLE, atom("_NET_WM_STATE_ABOVE")?, 0, FROM_APP, 0],
        );
        conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?
            .check()
            .context("sending _NET_WM_STATE")?;
        tracing::info!(window, "pin toggled");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_desktop_and_taskbars() {
        for class in ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
            assert!(is_shell_class(class), "{class}");
        }
        assert!(!is_shell_class("Notepad"));
    }

    #[test]
    fn reads_the_script_id_from_dbus_send() {
        assert_eq!(parse_dbus_int("method return time=1 sender=:1.5 -> destination=:1.9\n   int32 7\n"), Some(7));
        assert_eq!(parse_dbus_int("method return\n"), None);
    }

    #[test]
    fn picks_a_backend_per_desktop() {
        assert_eq!(linux_backend(true, "ubuntu:GNOME", false), Backend::Gnome);
        assert_eq!(linux_backend(false, "GNOME", false), Backend::Gnome);
        assert_eq!(linux_backend(true, "Hyprland", true), Backend::Hyprland);
        assert_eq!(linux_backend(true, "KDE", false), Backend::Kwin);
        assert_eq!(linux_backend(false, "KDE", false), Backend::X11);
        assert_eq!(linux_backend(false, "XFCE", false), Backend::X11);
        assert_eq!(linux_backend(true, "sway", false), Backend::Unsupported);
    }
}
