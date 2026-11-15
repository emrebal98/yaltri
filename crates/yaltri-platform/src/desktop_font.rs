//! Linux: the desktop's interface font (GNOME `font-name`, e.g. `'Ubuntu 11'`),
//! so our windows and the tray menu match the rest of the desktop.

use std::process::Command;

/// Family of the desktop's interface font; `None` outside GNOME-like
/// desktops or when it cannot be read.
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
fn family_from_setting(value: &str) -> Option<String> {
    let name = value.trim().trim_matches('\'').trim();
    let family = name.rsplit_once(' ').filter(|(_, size)| size.parse::<f32>().is_ok()).map_or(name, |(f, _)| f);
    let family = family.trim().trim_end_matches(',');
    (!family.is_empty() && family.parse::<f32>().is_err()).then(|| family.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_size() {
        assert_eq!(family_from_setting("'Ubuntu 11'").as_deref(), Some("Ubuntu"));
        assert_eq!(family_from_setting("'Noto Sans 10.5'").as_deref(), Some("Noto Sans"));
        assert_eq!(family_from_setting("'Cantarell'").as_deref(), Some("Cantarell"));
        assert_eq!(family_from_setting("'Fira Sans, 12'").as_deref(), Some("Fira Sans"));
    }

    #[test]
    fn rejects_empty_values() {
        assert_eq!(family_from_setting("''"), None);
        assert_eq!(family_from_setting("'11'"), None);
    }
}
