//! Windows: which PowerToys modules use which shortcut.
//!
//! PowerToys listens through keyboard hooks, so a clash with it is invisible
//! to `RegisterHotKey`, and whichever hook was installed last wins the key.
//! PowerToys stores every module's shortcuts in its settings files, so
//! clashes can be named precisely instead of guessed.

use std::path::{Path, PathBuf};

use serde_json::Value;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

use crate::hookkeys::Combo;

/// A shortcut an enabled PowerToys module currently uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerToysShortcut {
    /// Readable module name, e.g. "Color Picker".
    pub module: String,
    pub combo: Combo,
}

fn settings_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| Path::new(&d).join("Microsoft").join("PowerToys"))
}

/// The PowerToys module using `combo`, if PowerToys is running and that
/// module is enabled.
pub fn owner_of(combo: &Combo) -> Option<String> {
    if !is_running() {
        return None;
    }
    active_shortcuts(&settings_root()?).into_iter().find(|s| s.combo == *combo).map(|s| s.module)
}

/// Shortcuts of every enabled module under a PowerToys settings folder.
fn active_shortcuts(root: &Path) -> Vec<PowerToysShortcut> {
    let enabled: Option<Value> = std::fs::read_to_string(root.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("enabled").cloned());
    let Ok(dirs) = std::fs::read_dir(root) else { return Vec::new() };

    let mut out = Vec::new();
    for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
        let folder = dir.file_name().to_string_lossy().into_owned();
        // Modules missing from the map are treated as enabled.
        let is_enabled = enabled.as_ref().and_then(|e| e.get(&folder)).and_then(Value::as_bool).unwrap_or(true);
        if !is_enabled {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.path().join("settings.json")) else { continue };
        let Ok(json) = serde_json::from_str::<Value>(&text) else { continue };
        let mut combos = Vec::new();
        collect(&json, None, &mut combos);
        out.extend(combos.into_iter().map(|combo| PowerToysShortcut { module: readable(&folder), combo }));
    }
    out
}

/// Finds `{win, ctrl, alt, shift, code}` objects, skipping `Default*` keys
/// (factory values, not the active ones) and unassigned (`code: 0`) ones.
fn collect(value: &Value, key: Option<&str>, out: &mut Vec<Combo>) {
    match value {
        Value::Object(map) => {
            let flag = |k: &str| map.get(k).and_then(Value::as_bool);
            if let (Some(win), Some(ctrl), Some(alt), Some(shift), Some(code)) =
                (flag("win"), flag("ctrl"), flag("alt"), flag("shift"), map.get("code").and_then(Value::as_u64))
            {
                let is_default = key.is_some_and(|k| k.to_ascii_lowercase().starts_with("default"));
                if code != 0 && !is_default {
                    out.push(Combo { ctrl, alt, shift, win, vk: code as u32 });
                }
                return;
            }
            for (k, v) in map {
                collect(v, Some(k), out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect(v, key, out)),
        _ => {}
    }
}

/// "ColorPicker" -> "Color Picker"; "PowerToys Run" stays.
fn readable(folder: &str) -> String {
    if folder.contains(' ') {
        return folder.to_owned();
    }
    let mut out = String::new();
    let mut prev_lower = false;
    for c in folder.chars() {
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    out
}

fn is_running() -> bool {
    // SAFETY: process snapshot walk; the handle is closed before returning.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry);
        while ok != 0 {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case("PowerToys.exe") {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut entry);
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn combo(win: bool, ctrl: bool, alt: bool, shift: bool, vk: u32) -> Combo {
        Combo { ctrl, alt, shift, win, vk }
    }

    #[test]
    fn reads_enabled_modules_and_skips_defaults() {
        let root = std::env::temp_dir().join(format!("yaltri-pt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for m in ["ColorPicker", "PowerToys Run", "TextExtractor"] {
            fs::create_dir_all(root.join(m)).unwrap();
        }
        fs::write(root.join("settings.json"), r#"{"enabled":{"ColorPicker":true,"PowerToys Run":true,"TextExtractor":false}}"#).unwrap();
        fs::write(
            root.join("ColorPicker/settings.json"),
            r#"{"properties":{"DefaultActivationShortcut":{"win":true,"ctrl":false,"alt":false,"shift":true,"code":67},
                "ActivationShortcut":{"win":true,"ctrl":false,"alt":false,"shift":true,"code":67}}}"#,
        )
        .unwrap();
        fs::write(
            root.join("PowerToys Run/settings.json"),
            r#"{"properties":{"open_powerlauncher":{"win":false,"ctrl":false,"alt":true,"shift":false,"code":32},
                "open_console":{"win":false,"ctrl":false,"alt":false,"shift":false,"code":0}}}"#,
        )
        .unwrap();
        fs::write(
            root.join("TextExtractor/settings.json"),
            r#"{"properties":{"ActivationShortcut":{"win":true,"ctrl":false,"alt":false,"shift":true,"code":84}}}"#,
        )
        .unwrap();

        let mut got = active_shortcuts(&root);
        got.sort_by(|a, b| a.module.cmp(&b.module));
        assert_eq!(
            got,
            vec![
                PowerToysShortcut { module: "Color Picker".into(), combo: combo(true, false, false, true, 67) },
                PowerToysShortcut { module: "PowerToys Run".into(), combo: combo(false, false, true, false, 32) },
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readable_names() {
        assert_eq!(readable("ColorPicker"), "Color Picker");
        assert_eq!(readable("PowerToys Run"), "PowerToys Run");
        assert_eq!(readable("AlwaysOnTop"), "Always On Top");
    }
}
