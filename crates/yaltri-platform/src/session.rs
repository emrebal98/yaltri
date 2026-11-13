//! Which desktop we are running on, so the app can pick a window strategy.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    Windows,
    /// Linux X11 session: global hotkeys and free window placement work.
    X11,
    /// Wayland compositor that implements wlr-layer-shell (KDE, Hyprland, Sway...).
    WaylandLayerShell,
    /// GNOME on Wayland: no layer-shell, no global hotkey grabs. Run the
    /// launcher through XWayland and let the user bind `yaltri toggle launcher`.
    WaylandGnome,
    Other,
}

impl Session {
    pub fn detect() -> Self {
        if cfg!(windows) {
            return Session::Windows;
        }
        if !cfg!(target_os = "linux") {
            return Session::Other;
        }
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
        if !wayland {
            return Session::X11;
        }
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
        if desktop.split(':').any(|d| d == "gnome" || d == "unity" || d == "ubuntu") {
            Session::WaylandGnome
        } else {
            Session::WaylandLayerShell
        }
    }

    /// Whether this process can grab global shortcuts by itself.
    pub fn supports_global_hotkeys(self) -> bool {
        matches!(self, Session::Windows | Session::X11)
    }

    pub fn is_wayland(self) -> bool {
        matches!(self, Session::WaylandLayerShell | Session::WaylandGnome)
    }
}
