//! Linux: `yaltri uninstall [--purge]`. Quits Yaltri and removes what it
//! set up for itself: the startup entry, the app menu entry and icon, GNOME
//! custom shortcuts left over from a crash. Settings and history go too if
//! the user says so (or with `--purge`). The binary is removed only from the
//! folder `install.sh` puts it in; a copy installed some other way (a package
//! manager, say) is left to whatever installed it.

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use yaltri_core::paths::Paths;
use yaltri_platform::{autostart, gnome_keys, ipc};

use crate::i18n::{t, tf};

pub fn uninstall(purge: bool) -> Result<()> {
    ipc::quit_daemon(Duration::from_secs(10))?;
    autostart::set_enabled(false)?;
    autostart::remove_app_entry()?;
    let paths = Paths::detect()?;
    // Yaltri gives its GNOME shortcuts back when it quits; this covers a
    // crash, where they were left behind.
    if gnome_keys::available() {
        gnome_keys::GnomeKeys::load(paths.gnome_shortcuts_file()).restore_all();
    }

    if purge || ask(t("uninstall.delete_data")) {
        for dir in [&paths.config_dir, &paths.data_dir] {
            if dir.file_name().is_some_and(|n| n == "yaltri") {
                match std::fs::remove_dir_all(dir) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        return Err(e).with_context(|| format!("removing {}", dir.display()));
                    }
                    _ => {}
                }
            }
        }
    }

    let exe = std::env::current_exe().context("locating the yaltri executable")?;
    if bin_dir().is_some_and(|dir| installed_by_script(&exe, &dir)) {
        // A running program may delete its own file on Linux.
        std::fs::remove_file(&exe).with_context(|| format!("removing {}", exe.display()))?;
    } else {
        println!("{}", tf("uninstall.binary_kept", &[("path", &exe.display().to_string())]));
    }
    println!("{}", t("uninstall.done"));
    Ok(())
}

/// Where `install.sh` installs: `$XDG_BIN_HOME` or `~/.local/bin`.
fn bin_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_BIN_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin")))
}

fn installed_by_script(exe: &Path, bin_dir: &Path) -> bool {
    exe.parent() == Some(bin_dir) && exe.file_name().is_some_and(|n| n == "yaltri")
}

/// A yes/no question on the terminal; "no" without one.
fn ask(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} {} ", t("uninstall.yes_no"));
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    is_yes(&answer)
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes" | "e" | "evet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_installer_copy_is_removed() {
        let bin = Path::new("/home/a/.local/bin");
        assert!(installed_by_script(Path::new("/home/a/.local/bin/yaltri"), bin));
        assert!(!installed_by_script(Path::new("/usr/bin/yaltri"), bin));
        assert!(!installed_by_script(Path::new("/home/a/src/yaltri/target/release/yaltri"), bin));
        assert!(!installed_by_script(Path::new("/home/a/.local/bin/other"), bin));
    }

    #[test]
    fn reads_yes_in_both_languages() {
        for yes in ["y", "Y\n", "yes", " e ", "Evet"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "h", "hayır", "yess"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }
}
