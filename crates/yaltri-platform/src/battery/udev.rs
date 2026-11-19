//! Linux: a udev rule that lets the logged-in user open the dongles the
//! battery tool asks (`/dev/hidraw*`), installed through polkit so the user
//! only types their password into the system's own prompt.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const RULE_PATH: &str = "/etc/udev/rules.d/70-yaltri-battery.rules";

/// The rule for every vendor a driver asks, so a new driver makes the
/// installed rule out of date and the user is asked again.
pub fn rule() -> String {
    let mut vendors: Vec<u16> = super::DRIVERS.iter().chain(super::headsets::DRIVERS).map(|d| d.vendor).collect();
    vendors.sort_unstable();
    vendors.dedup();
    let vendors: Vec<String> = vendors.iter().map(|v| format!("{v:04x}")).collect();
    format!("KERNEL==\"hidraw*\", ATTRS{{idVendor}}==\"{}\", TAG+=\"uaccess\"\n", vendors.join("|"))
}

/// The current rule is in place.
pub fn installed() -> bool {
    std::fs::read_to_string(RULE_PATH).is_ok_and(|r| r == rule())
}

/// The shell command that installs the rule, to run as root.
fn script() -> String {
    format!(
        "printf '%s' '{}' > {RULE_PATH} && udevadm control --reload && udevadm trigger --subsystem-match=hidraw && udevadm settle",
        rule()
    )
}

/// The same, for the user to paste into a terminal when polkit is missing.
pub fn manual_command() -> String {
    format!("sudo sh -c \"{}\"", script().replace('"', "\\\""))
}

/// polkit's `pkexec` is there to ask for the password.
pub fn can_install() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join("pkexec").is_file()))
}

/// What came of `install`.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// The user closed the password prompt.
    Cancelled,
}

/// Installs the rule through `pkexec`. Blocks until the user answers the
/// system's password prompt.
pub fn install() -> Result<Outcome> {
    let status = Command::new("pkexec").args(["sh", "-c", &script()]).status().context("running pkexec")?;
    match status.code() {
        Some(0) if Path::new(RULE_PATH).exists() => Ok(Outcome::Installed),
        // 126: the prompt was dismissed; 127: not authorized.
        Some(126) => Ok(Outcome::Cancelled),
        Some(127) => bail!("not authorized"),
        _ => bail!("installing the udev rule failed ({status})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_lists_each_vendor_once() {
        let rule = rule();
        assert!(rule.starts_with("KERNEL==\"hidraw*\", ATTRS{idVendor}==\"03f0|"), "{rule}");
        assert_eq!(rule.matches("1038").count(), 1, "{rule}");
        assert!(rule.contains("373b") && rule.contains("3554") && rule.ends_with("TAG+=\"uaccess\"\n"));
        // Single quotes would break the script.
        assert!(!rule.contains('\''));
    }
}
