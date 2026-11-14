//! "Check for updates": asks GitHub for the latest release, only when the
//! user clicks. Uses the system `curl` (Windows 10+ and every Linux desktop
//! ship it), so the app itself carries no HTTP/TLS stack.

use std::process::Command;

use anyhow::{Context as _, Result, bail};

pub const REPO_URL: &str = "https://github.com/emrebal98/yaltri";
pub const RELEASES_URL: &str = "https://github.com/emrebal98/yaltri/releases/latest";
pub const LICENSES_URL: &str = "https://github.com/emrebal98/yaltri/blob/main/assets/icons/LICENSE";
const API_URL: &str = "https://api.github.com/repos/emrebal98/yaltri/releases/latest";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Failed,
}

/// The latest release's version if it is newer than ours. Blocking.
pub fn check() -> Result<Option<String>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "10", "-H", "Accept: application/vnd.github+json", "-A", "yaltri", API_URL]);
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
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).context("parsing release")?;
    let tag = json.get("tag_name").and_then(|t| t.as_str()).context("release has no tag")?;
    let latest = tag.trim_start_matches('v');
    Ok(is_newer(latest, VERSION).then(|| latest.to_owned()))
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
}
