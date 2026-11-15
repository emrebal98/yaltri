//! Updates from GitHub releases. The daemon checks once a day (Settings →
//! General can turn that off) and when the user asks; a newer release is
//! downloaded in the background, checked against the SHA-256 GitHub
//! publishes for it, and installed when the user picks "Restart to update".
//!
//! - Windows: the downloaded `.exe` is started; like any download it
//!   installs itself over the running copy (see `install`) and starts again.
//! - Linux: the binary is replaced in place when it is the one `install.sh`
//!   put in `~/.local/bin`, then Yaltri starts again. Copies installed some
//!   other way (a package manager) are left to it: the user gets the
//!   release page instead, as in builds that cannot update themselves.
//!
//! Network access goes through the system `curl` (Windows 10+ and every
//! Linux desktop ship it), so the app carries no HTTP/TLS stack.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

pub const REPO_URL: &str = "https://github.com/emrebal98/yaltri";
pub const RELEASES_URL: &str = "https://github.com/emrebal98/yaltri/releases/latest";
pub const LICENSES_URL: &str = "https://github.com/emrebal98/yaltri/blob/main/assets/icons/LICENSE";
const API_URL: &str = "https://api.github.com/repos/emrebal98/yaltri/releases/latest";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// This system's release file.
#[cfg(windows)]
const ASSET: &str = "yaltri-windows-x86_64.exe";
#[cfg(not(windows))]
const ASSET: &str = "yaltri-linux-x86_64.tar.gz";

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    url: String,
    /// `sha256:<hex>` as GitHub reports it, if it does.
    digest: Option<String>,
}

/// Where the updater stands; the tray and Settings show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// Newer release this copy cannot install itself: link to the page.
    Available(Release),
    Downloading(Release),
    /// Downloaded and verified; installs on "Restart to update".
    Ready { release: Release, file: PathBuf },
    Failed,
}

/// The latest release if it is newer than this build. Blocking.
pub fn check() -> Result<Option<Release>> {
    let body = curl(&["-H", "Accept: application/vnd.github+json", API_URL])?;
    let json: serde_json::Value = serde_json::from_slice(&body).context("parsing release")?;
    let tag = json.get("tag_name").and_then(|t| t.as_str()).context("release has no tag")?;
    let version = tag.trim_start_matches('v').to_owned();
    if !is_newer(&version, VERSION) {
        return Ok(None);
    }
    let asset = json
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|assets| assets.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET)))
        .with_context(|| format!("release {tag} has no {ASSET}"))?;
    let url = asset.get("browser_download_url").and_then(|u| u.as_str()).context("asset has no url")?.to_owned();
    let digest = asset.get("digest").and_then(|d| d.as_str()).map(str::to_owned);
    Ok(Some(Release { version, url, digest }))
}

/// Whether this copy can replace itself (otherwise: link to the page).
pub fn can_install() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    #[cfg(windows)]
    return yaltri_platform::install::is_installed_copy(&exe);
    #[cfg(target_os = "linux")]
    return crate::uninstall_linux::is_installer_copy(&exe)
        && exe.parent().is_some_and(|dir| std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly()));
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = exe;
        false
    }
}

/// Downloads `release` into a temporary folder and checks its SHA-256.
/// Blocking. Returns the file to install.
pub fn download(release: &Release) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("yaltri-update-{}", release.version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = dir.join(ASSET);
    let path = file.to_string_lossy().into_owned();
    curl(&["--max-time", "600", "-o", &path, &release.url])?;
    match &release.digest {
        Some(digest) => {
            let expected = digest.strip_prefix("sha256:").context("unknown digest format")?;
            let actual = sha256_hex(&file)?;
            if !actual.eq_ignore_ascii_case(expected) {
                let _ = std::fs::remove_dir_all(&dir);
                bail!("checksum mismatch for {ASSET}");
            }
        }
        None => tracing::warn!("release has no checksum; installing {ASSET} unchecked"),
    }
    Ok(file)
}

/// Starts installing `file` and arranges for Yaltri to start again. The
/// caller quits right after.
pub fn install(file: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        // The new exe quits us, copies itself over the installed one and
        // starts it, like a run from the Downloads folder.
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        Command::new(file).creation_flags(DETACHED_PROCESS).spawn().context("starting the new version")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = file.parent().context("download has no folder")?;
        run(Command::new("tar").arg("xzf").arg(file).arg("-C").arg(dir))?;
        let new_binary = dir.join(ASSET.trim_end_matches(".tar.gz")).join("yaltri");
        let exe = std::env::current_exe().context("locating the yaltri executable")?;
        // Copy next to the old one and rename over it: atomic, and a running
        // program's file may be replaced on Linux.
        let staged = exe.with_extension("new");
        std::fs::copy(&new_binary, &staged).with_context(|| format!("copying to {}", staged.display()))?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&staged, &exe).with_context(|| format!("replacing {}", exe.display()))?;
        // Start again once this process has gone (it holds the IPC socket).
        Command::new("sh")
            .arg("-c")
            .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$0""#)
            .arg(&exe)
            .arg(std::process::id().to_string())
            .spawn()
            .context("scheduling the restart")?;
        Ok(())
    }
}

/// Deletes downloads of this version or older, left in the temp folder by
/// an update that has been installed. A file still in use is skipped and
/// goes on a later start.
pub fn remove_old_downloads() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name.to_str().and_then(|n| n.strip_prefix("yaltri-update-")) else { continue };
        if !is_newer(version, VERSION) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "30", "-A", "yaltri"]).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().context("running curl")?;
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

#[cfg(not(windows))]
fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn sha256_hex(file: &Path) -> Result<String> {
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    let mut reader = std::fs::File::open(file).with_context(|| format!("opening {}", file.display()))?;
    std::io::copy(&mut reader, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Numeric dotted comparison; pre-release suffixes are ignored.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parts(candidate) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-beta.1", "0.1.0"));
    }

    #[test]
    fn hashes_files() {
        let file = std::env::temp_dir().join(format!("yaltri-sha-test-{}", std::process::id()));
        std::fs::write(&file, b"abc").unwrap();
        let hex = sha256_hex(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
