//! The desktop's interface font, so our windows (and on GNOME the tray
//! menu's shortcut column) match the rest of the desktop.
//!
//! - Linux: GNOME's `font-name` setting (e.g. `'Ubuntu 11'`).
//! - Windows: Segoe UI Variable when installed (the Windows 11 UI font),
//!   else the system message font: Segoe UI on Windows 10, the local font on
//!   CJK systems. The system setting itself still says Segoe UI on
//!   Windows 11, so it is checked second.

/// Family of the desktop's interface font; `None` when it cannot be read.
pub fn family() -> Option<String> {
    imp::family()
}

#[cfg(target_os = "linux")]
pub use imp::file;

#[cfg(target_os = "linux")]
mod imp {
    use std::process::Command;

    pub fn family() -> Option<String> {
        let value = run("gsettings", &["get", "org.gnome.desktop.interface", "font-name"])?;
        family_from_setting(&value)
    }

    /// The font file fontconfig picks for `family()`.
    pub fn file() -> Option<Vec<u8>> {
        let path = run("fc-match", &["-f", "%{file}", &family()?])?;
        std::fs::read(path).ok()
    }

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// `'Ubuntu 11'` -> `Ubuntu`: a GSettings string, a Pango font description
    /// with the size last.
    pub(super) fn family_from_setting(value: &str) -> Option<String> {
        let name = value.trim().trim_matches('\'').trim();
        let family = name.rsplit_once(' ').filter(|(_, size)| size.parse::<f32>().is_ok()).map_or(name, |(f, _)| f);
        let family = family.trim().trim_end_matches(',');
        (!family.is_empty() && family.parse::<f32>().is_err()).then(|| family.to_owned())
    }
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SystemParametersInfoW,
    };

    pub fn family() -> Option<String> {
        let fonts = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts");
        if fonts.join("SegUIVar.ttf").exists() {
            return Some("Segoe UI Variable Text".to_owned());
        }
        message_font()
    }

    /// Face name of the font Windows uses for message boxes and dialogs.
    fn message_font() -> Option<String> {
        // SAFETY: plain struct of integers and arrays; all-zero is valid.
        let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
        metrics.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
        // SAFETY: the buffer is a NONCLIENTMETRICSW of the size passed.
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, (&raw mut metrics).cast(), 0)
        };
        if ok == 0 {
            return None;
        }
        face_name(&metrics.lfMessageFont.lfFaceName)
    }

    /// A NUL-terminated UTF-16 `LOGFONTW` face name.
    pub(super) fn face_name(raw: &[u16]) -> Option<String> {
        let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        let name = String::from_utf16(&raw[..len]).ok()?;
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_setting_drops_the_size() {
        use super::imp::family_from_setting;
        assert_eq!(family_from_setting("'Ubuntu 11'").as_deref(), Some("Ubuntu"));
        assert_eq!(family_from_setting("'Noto Sans 10.5'").as_deref(), Some("Noto Sans"));
        assert_eq!(family_from_setting("'Cantarell'").as_deref(), Some("Cantarell"));
        assert_eq!(family_from_setting("'Fira Sans, 12'").as_deref(), Some("Fira Sans"));
        assert_eq!(family_from_setting("''"), None);
        assert_eq!(family_from_setting("'11'"), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_face_name_stops_at_nul() {
        use super::imp::face_name;
        let mut raw = [0u16; 32];
        for (slot, c) in raw.iter_mut().zip("Segoe UI".encode_utf16()) {
            *slot = c;
        }
        assert_eq!(face_name(&raw).as_deref(), Some("Segoe UI"));
        assert_eq!(face_name(&[0u16; 32]), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_some_font() {
        assert!(super::family().is_some());
    }
}
