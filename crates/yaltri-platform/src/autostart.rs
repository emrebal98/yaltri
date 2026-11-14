//! Start with the system: the real OS entry is the source of truth, so a
//! change in the OS's own startup-apps list shows up in Settings.
//!
//! - Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Yaltri`
//!   = `"<exe>"`. Settings > Apps > Startup toggles a flag under
//!   `Explorer\StartupApproved\Run` instead of removing the value; it is read
//!   too, and cleared when we turn autostart on.
//! - Linux: `~/.config/autostart/yaltri.desktop` (XDG autostart).

use anyhow::{Context as _, Result};

/// Whether Yaltri starts when the user signs in.
pub fn is_enabled() -> bool {
    imp::is_enabled()
}

/// Adds or removes the startup entry for the running executable.
pub fn set_enabled(on: bool) -> Result<()> {
    let exe = std::env::current_exe().context("locating the yaltri executable")?;
    imp::set_enabled(on, &exe)
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use anyhow::{Result, bail};
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
        RegSetKeyValueW,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "Yaltri";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn is_enabled() -> bool {
        let (run, name) = (wide(RUN), wide(NAME));
        let mut len = 0u32;
        // SAFETY: NUL-terminated strings; a null buffer only queries the size.
        let present = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                run.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut len,
            )
        } == 0;
        present && !disabled_in_startup_apps()
    }

    /// Settings > Apps > Startup writes 12 bytes; an odd first byte means
    /// "off" (02 = on, 03 = off).
    fn disabled_in_startup_apps() -> bool {
        let (key, name) = (wide(APPROVED), wide(NAME));
        let mut data = [0u8; 12];
        let mut len = data.len() as u32;
        // SAFETY: NUL-terminated strings and a buffer of `len` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_BINARY,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut len,
            )
        };
        status == 0 && len > 0 && data[0] & 1 == 1
    }

    pub fn set_enabled(on: bool, exe: &Path) -> Result<()> {
        let (run, approved, name) = (wide(RUN), wide(APPROVED), wide(NAME));
        if on {
            let command = wide(&format!("\"{}\"", exe.display()));
            // SAFETY: NUL-terminated strings; the data is the UTF-16 command
            // including its terminator, sized in bytes.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    run.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            };
            if status != 0 {
                bail!("{}", std::io::Error::from_raw_os_error(status as i32));
            }
            // A previous "off" in the system's startup list would still win.
            // SAFETY: NUL-terminated strings.
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, approved.as_ptr(), name.as_ptr()) };
        } else {
            // SAFETY: NUL-terminated strings.
            let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, run.as_ptr(), name.as_ptr()) };
            if status != 0 && status != ERROR_FILE_NOT_FOUND {
                bail!("{}", std::io::Error::from_raw_os_error(status as i32));
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};

    use anyhow::{Context as _, Result};

    fn entry_path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("autostart").join("yaltri.desktop"))
    }

    pub fn is_enabled() -> bool {
        let Some(text) = entry_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return false;
        };
        // Desktop settings can switch an entry off without deleting it.
        !text.lines().any(|l| {
            let l = l.trim().replace(' ', "");
            l.eq_ignore_ascii_case("Hidden=true") || l.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
        })
    }

    pub fn desktop_entry(exe: &Path) -> String {
        // Desktop Entry spec: quote the program, escaping `"`, `` ` ``, `$`, `\`.
        let mut quoted = String::from("\"");
        for c in exe.display().to_string().chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        format!(
            "[Desktop Entry]\nType=Application\nName=Yaltri\nComment=Search and color picker in the tray\nExec={quoted}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }

    pub fn set_enabled(on: bool, exe: &Path) -> Result<()> {
        let path = entry_path().context("no home directory")?;
        if on {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            }
            std::fs::write(&path, desktop_entry(exe)).with_context(|| format!("writing {}", path.display()))?;
        } else {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(e).with_context(|| format!("removing {}", path.display()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quotes_the_binary_path() {
            let entry = desktop_entry(Path::new("/home/a b/.local/bin/yaltri"));
            assert!(entry.contains("Exec=\"/home/a b/.local/bin/yaltri\"\n"));
            assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        }
    }
}
