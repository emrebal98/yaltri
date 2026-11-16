use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ToolId;

pub const SETTINGS_VERSION: u32 = 1;

/// User settings, stored as TOML. Every field has a default, so a partial or
/// older file still loads; unknown keys are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub general: GeneralSettings,
    pub launcher: LauncherSettings,
    pub files: FilesSettings,
    pub games: GamesSettings,
    pub picker: PickerSettings,
    pub pin: PinSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub start_with_system: bool,
    /// UI language: "auto" (follow the system), "en" or "tr".
    pub language: String,
    /// "system" (follow the OS light/dark setting), "dark" or "light".
    pub theme: String,
    /// Look for a new release once a day and download it in the background.
    pub check_updates: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSettings {
    pub enabled: bool,
    /// Global shortcut, e.g. `Alt+Space`. Ignored on Wayland, where the
    /// compositor binds `yaltri toggle launcher` instead.
    pub hotkey: String,
    pub max_results: usize,
}

/// Game launchers whose installed games show up in the launcher. Launchers
/// that are not installed are simply skipped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GamesSettings {
    pub steam: bool,
    pub epic: bool,
    pub gog: bool,
    pub ubisoft: bool,
    pub ea: bool,
    /// Xbox app / PC Game Pass (Microsoft Store games).
    pub xbox: bool,
    pub riot: bool,
    pub heroic: bool,
    pub lutris: bool,
}

impl Default for GamesSettings {
    fn default() -> Self {
        Self { steam: true, epic: true, gog: true, ubisoft: true, ea: true, xbox: true, riot: true, heroic: true, lutris: true }
    }
}

/// What the launcher searches besides the calculator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesSettings {
    /// Index and search files and folders.
    pub files_enabled: bool,
    /// Search installed applications (Start Menu / `.desktop` files).
    pub apps_enabled: bool,
    /// Folders to index. Empty means every fixed drive on Windows and `/` on
    /// Linux.
    pub roots: Vec<PathBuf>,
    /// Folders skipped entirely, on top of the built-in system folders
    /// (`C:\Windows`, `/proc`, ...).
    pub exclude_paths: Vec<PathBuf>,
    /// Folder names skipped wherever they appear (`node_modules`, `.git`).
    pub exclude_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickerSettings {
    pub enabled: bool,
    pub hotkey: String,
    /// Format copied to the clipboard on pick: hex, rgb, hsl, hsv, cmyk,
    /// hwb, decimal, float.
    pub copy_format: String,
    /// Open the editor (history, shades, all formats) after picking.
    pub open_editor: bool,
}

/// Always on top: the shortcut toggles it for the focused window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PinSettings {
    pub enabled: bool,
    pub hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            general: GeneralSettings::default(),
            launcher: LauncherSettings::default(),
            files: FilesSettings::default(),
            games: GamesSettings::default(),
            picker: PickerSettings::default(),
            pin: PinSettings::default(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self { start_with_system: true, language: "auto".into(), theme: "system".into(), check_updates: true }
    }
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: "Super+Shift+F".into(),
            max_results: 8,
        }
    }
}

impl Default for FilesSettings {
    fn default() -> Self {
        Self {
            files_enabled: true,
            apps_enabled: true,
            roots: Vec::new(),
            exclude_paths: Vec::new(),
            exclude_names: ["node_modules", ".git", "__pycache__", "$RECYCLE.BIN", "System Volume Information"]
                .map(String::from)
                .to_vec(),
        }
    }
}

impl Default for PinSettings {
    fn default() -> Self {
        Self { enabled: true, hotkey: "Super+Shift+T".into() }
    }
}

impl Default for PickerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: "Super+Shift+C".into(),
            copy_format: "hex".into(),
            open_editor: true,
        }
    }
}

/// Result of loading settings from disk.
#[derive(Debug)]
pub struct Loaded {
    pub settings: Settings,
    /// Set when the file existed but could not be parsed. The broken file was
    /// moved aside to `settings.toml.bak` and defaults are in use.
    pub recovered_from: Option<String>,
}

impl Settings {
    /// Loads settings, falling back to defaults when the file is missing.
    /// A corrupt file is renamed to `*.bak` instead of failing startup.
    pub fn load(path: &Path) -> Result<Loaded> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Loaded { settings: Settings::default(), recovered_from: None });
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };

        match toml::from_str::<Settings>(&text) {
            Ok(settings) => Ok(Loaded { settings: settings.migrate(), recovered_from: None }),
            Err(err) => {
                let backup = path.with_extension("toml.bak");
                fs::rename(path, &backup)
                    .with_context(|| format!("backing up corrupt settings to {}", backup.display()))?;
                tracing::warn!(%err, backup = %backup.display(), "settings file corrupt, using defaults");
                Ok(Loaded {
                    settings: Settings::default(),
                    recovered_from: Some(err.to_string()),
                })
            }
        }
    }

    /// Writes atomically: temp file in the same directory, then rename.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing settings")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    /// Upgrades older on-disk layouts. Only v1 exists so far.
    fn migrate(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self
    }

    pub fn is_enabled(&self, tool: ToolId) -> bool {
        match tool {
            ToolId::Launcher => self.launcher.enabled,
            ToolId::Picker => self.picker.enabled,
            ToolId::Pin => self.pin.enabled,
        }
    }

    pub fn set_enabled(&mut self, tool: ToolId, enabled: bool) {
        match tool {
            ToolId::Launcher => self.launcher.enabled = enabled,
            ToolId::Picker => self.picker.enabled = enabled,
            ToolId::Pin => self.pin.enabled = enabled,
        }
    }

    pub fn hotkey(&self, tool: ToolId) -> &str {
        match tool {
            ToolId::Launcher => &self.launcher.hotkey,
            ToolId::Picker => &self.picker.hotkey,
            ToolId::Pin => &self.pin.hotkey,
        }
    }

    pub fn set_hotkey(&mut self, tool: ToolId, hotkey: String) {
        match tool {
            ToolId::Launcher => self.launcher.hotkey = hotkey,
            ToolId::Picker => self.picker.hotkey = hotkey,
            ToolId::Pin => self.pin.hotkey = hotkey,
        }
    }

    /// Enabled tools whose hotkeys collide (case-insensitive, modifier order
    /// ignored). Returned as pairs of tool ids.
    pub fn hotkey_conflicts(&self) -> Vec<(ToolId, ToolId)> {
        let enabled: Vec<ToolId> = ToolId::ALL
            .into_iter()
            .filter(|t| self.is_enabled(*t) && !self.hotkey(*t).trim().is_empty())
            .collect();
        let mut conflicts = Vec::new();
        for (i, a) in enabled.iter().enumerate() {
            for b in &enabled[i + 1..] {
                if normalize_hotkey(self.hotkey(*a)) == normalize_hotkey(self.hotkey(*b)) {
                    conflicts.push((*a, *b));
                }
            }
        }
        conflicts
    }
}

/// Canonical form for comparing shortcuts: modifiers in a fixed order with
/// aliases folded, then the key. `"space + Shift+Win"` -> `"shift+super+space"`.
pub fn normalize_hotkey(s: &str) -> String {
    const ORDER: [&str; 4] = ["ctrl", "alt", "shift", "super"];
    let mut mods = Vec::new();
    let mut keys = Vec::new();
    for part in s.split('+').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()) {
        let canonical = match part.as_str() {
            "control" | "ctrl" => "ctrl",
            "alt" | "option" => "alt",
            "shift" => "shift",
            "super" | "win" | "meta" | "cmd" | "command" => "super",
            _ => {
                keys.push(part);
                continue;
            }
        };
        if !mods.contains(&canonical) {
            mods.push(canonical);
        }
    }
    mods.sort_by_key(|m| ORDER.iter().position(|o| o == m));
    mods.into_iter().map(str::to_owned).chain(keys).collect::<Vec<_>>().join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yaltri-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        let loaded = Settings::load(&dir.join("settings.toml")).unwrap();
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.recovered_from.is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("nested").join("settings.toml");
        let mut s = Settings::default();
        s.launcher.hotkey = "Ctrl+Space".into();
        s.picker.enabled = true;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().settings, s);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let dir = temp_dir("partial");
        let path = dir.join("settings.toml");
        fs::write(&path, "[launcher]\nhotkey = \"Ctrl+K\"\n").unwrap();
        let s = Settings::load(&path).unwrap().settings;
        assert_eq!(s.launcher.hotkey, "Ctrl+K");
        assert!(s.launcher.enabled);
        assert_eq!(s.picker, PickerSettings::default());
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.toml");
        fs::write(&path, "this is = = not toml").unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.recovered_from.is_some());
        assert!(!path.exists());
        assert!(dir.join("settings.toml.bak").exists());
    }

    #[test]
    fn detects_conflicts_only_between_enabled_tools() {
        let mut s = Settings::default();
        s.picker.hotkey = "f + SHIFT+super".into();
        s.picker.enabled = false;
        assert!(s.hotkey_conflicts().is_empty(), "disabled tools never conflict");
        s.picker.enabled = true;
        assert_eq!(s.hotkey_conflicts(), vec![(ToolId::Launcher, ToolId::Picker)]);
    }

    #[test]
    fn pin_defaults_to_super_shift_t_without_conflicts() {
        let s = Settings::default();
        assert_eq!(s.pin, PinSettings { enabled: true, hotkey: "Super+Shift+T".into() });
        assert!(s.hotkey_conflicts().is_empty());
        let old: Settings = toml::from_str("[launcher]
enabled = true
").unwrap();
        assert_eq!(old.pin, PinSettings::default(), "older files get the pin defaults");
    }

    #[test]
    fn normalizes_hotkeys() {
        assert_eq!(normalize_hotkey("Shift + Alt+Space"), "alt+shift+space");
        assert_eq!(normalize_hotkey("space+Win+shift"), "shift+super+space");
        assert_eq!(normalize_hotkey("Control+K"), "ctrl+k");
    }
}
