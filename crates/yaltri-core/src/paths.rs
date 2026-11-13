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

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}
