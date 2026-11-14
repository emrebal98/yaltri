use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

/// Platform specific directories:
/// - Windows: `%APPDATA%\yaltri\config`, `%LOCALAPPDATA%\yaltri\data`
/// - Linux:   `~/.config/yaltri`, `~/.local/share/yaltri`
#[derive(Clone, Debug)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "yaltri").context("no home directory found")?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.data_dir.join("history.toml")
    }

    pub fn colors_file(&self) -> PathBuf {
        self.data_dir.join("colors.toml")
    }

    pub fn file_index(&self) -> PathBuf {
        self.data_dir.join("files.idx")
    }

    /// GNOME shortcuts Yaltri took over, to give back (Linux).
    pub fn gnome_shortcuts_file(&self) -> PathBuf {
        self.data_dir.join("gnome-shortcuts.json")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}
