//! Windows: Yaltri installs itself. Run from anywhere else (the Downloads
//! folder, say), a release build quits a running Yaltri, copies itself to
//! `%LOCALAPPDATA%\Programs\Yaltri`, registers the Start menu shortcut and
//! the Installed apps entry, and starts the installed copy. Running a newer
//! download the same way updates it. `yaltri uninstall` undoes it all.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use yaltri_core::paths::Paths;
use yaltri_platform::install;
use yaltri_platform::ipc;

use crate::i18n::t;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a running Yaltri gets to quit and release its executable.
const QUIT_TIMEOUT: Duration = Duration::from_secs(10);

/// What `main` should do after `self_install`.
pub enum Outcome {
    /// Keep starting from this executable.
    Continue,
    /// The installed copy was started; exit.
    Handed,
}

/// Installs and starts the installed copy when this executable is not it.
/// Any failure is logged and the app runs from where it is.
pub fn self_install() -> Outcome {
    // Debug builds run from `target/` while developing.
    if cfg!(debug_assertions) {
        return Outcome::Continue;
    }
    let Ok(exe) = std::env::current_exe() else { return Outcome::Continue };
    if install::is_installed_copy(&exe) {
        if let Err(e) = install::register(&exe, VERSION) {
            tracing::warn!("refreshing install entries failed: {e:#}");
        }
        // An older startup entry may still point at the downloaded file.
        if yaltri_platform::autostart::is_enabled()
            && let Err(e) = yaltri_platform::autostart::set_enabled(true)
        {
            tracing::warn!("refreshing autostart failed: {e:#}");
        }
        return Outcome::Continue;
    }
    match install_from(&exe) {
        Ok(()) => Outcome::Handed,
        Err(e) => {
            tracing::warn!("self-install failed, running from {}: {e:#}", exe.display());
            Outcome::Continue
        }
    }
}

fn install_from(exe: &Path) -> Result<()> {
    quit_running()?;
    let target = retry(|| install::copy_into_place(exe))?;
    install::register(&target, VERSION)?;
    std::process::Command::new(&target).spawn().with_context(|| format!("starting {}", target.display()))?;
    tracing::info!(to = %target.display(), "installed");
    Ok(())
}

fn quit_running() -> Result<()> {
    ipc::quit_daemon(QUIT_TIMEOUT)
}

/// The old process may hold its executable for a moment after closing its
/// socket.
fn retry<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if start.elapsed() >= QUIT_TIMEOUT => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// `yaltri uninstall [--purge]`: quits Yaltri, removes autostart, the Start
/// menu shortcut, the Installed apps entry and the program folder, and (if
/// the user agrees, or with `--purge`) settings and history.
pub fn uninstall(purge: bool) -> Result<()> {
    quit_running()?;
    yaltri_platform::autostart::set_enabled(false)?;
    install::unregister()?;

    if (purge || install::ask_yes_no(t("uninstall.title"), t("uninstall.delete_data")))
        && let Ok(paths) = Paths::detect()
    {
        // `%APPDATA%\yaltri` and `%LOCALAPPDATA%\yaltri`.
        for dir in [paths.config_dir.parent(), paths.data_dir.parent()].into_iter().flatten() {
            if dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("yaltri")) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    let exe = std::env::current_exe().context("locating the yaltri executable")?;
    if install::is_installed_copy(&exe) {
        install::delete_install_dir_after_exit()?;
    }
    Ok(())
}
