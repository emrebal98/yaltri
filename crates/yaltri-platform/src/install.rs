//! Windows per-user install, no admin rights: the executable lives in
//! `%LOCALAPPDATA%\Programs\Yaltri\yaltri.exe`, with a Start menu shortcut
//! and an entry under Settings > Apps > Installed apps whose "Uninstall"
//! runs `yaltri.exe uninstall`.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RegDeleteTreeW, RegSetKeyValueW,
};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Yaltri";
const EXE_NAME: &str = "yaltri.exe";
const REPO_URL: &str = "https://github.com/emrebal98/yaltri";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// `%LOCALAPPDATA%\Programs\Yaltri`.
pub fn install_dir() -> Option<PathBuf> {
    Some(env_dir("LOCALAPPDATA")?.join("Programs").join("Yaltri"))
}

/// Where the installed executable lives.
pub fn installed_exe() -> Option<PathBuf> {
    Some(install_dir()?.join(EXE_NAME))
}

fn shortcut_path() -> Option<PathBuf> {
    Some(env_dir("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs\Yaltri.lnk"))
}

/// Whether `exe` is the installed copy (Windows paths ignore case).
pub fn is_installed_copy(exe: &Path) -> bool {
    let Some(target) = installed_exe() else { return false };
    let normalize = |p: &Path| {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned()).to_string_lossy().to_lowercase()
    };
    normalize(exe) == normalize(&target)
}

/// Copies `exe` over the installed one. Fails while the installed copy is
/// still running, so the caller retries until it has exited.
pub fn copy_into_place(exe: &Path) -> Result<PathBuf> {
    let dir = install_dir().context("LOCALAPPDATA is not set")?;
    let target = dir.join(EXE_NAME);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let staged = dir.join("yaltri.exe.new");
    std::fs::copy(exe, &staged).with_context(|| format!("copying to {}", staged.display()))?;
    // The copy keeps the download's "from the internet" mark, which would
    // make Windows warn again on every start from the Start menu. The user
    // already chose to run this file.
    let _ = std::fs::remove_file(format!("{}:Zone.Identifier", staged.display()));
    if let Err(e) = std::fs::rename(&staged, &target) {
        let _ = std::fs::remove_file(&staged);
        return Err(e).with_context(|| format!("replacing {}", target.display()));
    }
    Ok(target)
}

/// Writes the Start menu shortcut and the Installed apps entry for `exe`.
pub fn register(exe: &Path, version: &str) -> Result<()> {
    write_shortcut(exe)?;
    write_uninstall_entry(exe, version)
}

/// Removes the Start menu shortcut and the Installed apps entry.
pub fn unregister() -> Result<()> {
    if let Some(path) = shortcut_path() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("removing {}", path.display()));
            }
            _ => {}
        }
    }
    let key = wide(UNINSTALL_KEY);
    // SAFETY: NUL-terminated key path.
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
    if status != 0 && status != ERROR_FILE_NOT_FOUND {
        bail!("{}", std::io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

/// Deletes the install folder once this process has exited: a running
/// executable cannot delete itself on Windows, so a hidden `cmd` waits a
/// moment and removes it.
pub fn delete_install_dir_after_exit() -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let dir = install_dir().context("LOCALAPPDATA is not set")?;
    if !dir.ends_with(r"Programs\Yaltri") {
        bail!("refusing to delete unexpected folder {}", dir.display());
    }
    std::process::Command::new("cmd.exe")
        .raw_arg(format!("/C ping -n 3 127.0.0.1 >NUL & rmdir /S /Q \"{}\"", dir.display()))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .context("starting cleanup")?;
    Ok(())
}

fn write_shortcut(exe: &Path) -> Result<()> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface as _};

    let path = shortcut_path().context("APPDATA is not set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let exe_w = HSTRING::from(exe.as_os_str());
    let dir_w = HSTRING::from(exe.parent().unwrap_or(exe).as_os_str());
    // SAFETY: COM is initialised on this thread for the calls below and
    // released only if this call initialised it.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&exe_w)?;
            link.SetWorkingDirectory(&dir_w)?;
            link.SetIconLocation(&exe_w, 0)?;
            link.SetDescription(&HSTRING::from("Search and color picker in the tray"))?;
            link.cast::<IPersistFile>()?.Save(&HSTRING::from(path.as_os_str()), true)
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result.with_context(|| format!("writing {}", path.display()))
    }
}

fn write_uninstall_entry(exe: &Path, version: &str) -> Result<()> {
    let exe_s = exe.display().to_string();
    let dir_s = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let size_kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    for (name, value) in [
        ("DisplayName", "Yaltri"),
        ("DisplayVersion", version),
        ("Publisher", "Yaltri"),
        ("DisplayIcon", &format!("{exe_s},0")),
        ("InstallLocation", &dir_s),
        ("UninstallString", &format!("\"{exe_s}\" uninstall")),
        ("URLInfoAbout", REPO_URL),
    ] {
        set_string(name, value)?;
    }
    for (name, value) in [("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", size_kb)] {
        set_dword(name, value)?;
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_value(name: &str, kind: u32, data: &[u8]) -> Result<()> {
    let (key, name_w) = (wide(UNINSTALL_KEY), wide(name));
    // SAFETY: NUL-terminated strings; `data` is valid for its length.
    let status = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name_w.as_ptr(), kind, data.as_ptr().cast(), data.len() as u32)
    };
    if status != 0 {
        bail!("writing {name}: {}", std::io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

fn set_string(name: &str, value: &str) -> Result<()> {
    let bytes: Vec<u8> = wide(value).iter().flat_map(|c| c.to_le_bytes()).collect();
    set_value(name, REG_SZ, &bytes)
}

fn set_dword(name: &str, value: u32) -> Result<()> {
    set_value(name, REG_DWORD, &value.to_le_bytes())
}

/// A Yes/No question box with no owner window; true on "Yes".
pub fn ask_yes_no(title: &str, text: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
    let (title, text) = (wide(title), wide(text));
    // SAFETY: NUL-terminated strings; no owner window.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONQUESTION) == IDYES }
}
