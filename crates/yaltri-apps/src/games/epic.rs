//! Epic Games Launcher (Windows): `.item` manifests, launched through the
//! launcher's URL scheme so overlays, login and DRM work.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yaltri_core::ItemAction;

use super::{Launcher, game};
use crate::App;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
    display_name: String,
    app_name: String,
    catalog_namespace: String,
    catalog_item_id: String,
    install_location: String,
    #[serde(default)]
    launch_executable: String,
    #[serde(default)]
    app_categories: Vec<String>,
    #[serde(default)]
    main_game_app_name: String,
    #[serde(default, rename = "bIsIncompleteInstall")]
    incomplete: bool,
}

pub fn manifests_dir() -> Option<PathBuf> {
    let base = std::env::var_os("ProgramData").map(PathBuf::from)?;
    let dir = base.join(r"Epic\EpicGamesLauncher\Data\Manifests");
    dir.is_dir().then_some(dir)
}

pub fn discover(dir: &Path) -> Vec<App> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "item"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| from_manifest(&text, true))
        .collect()
}

/// `check_disk`: require the install folder to exist (off in tests).
fn from_manifest(text: &str, check_disk: bool) -> Option<App> {
    let m: Manifest = serde_json::from_str(text).ok()?;
    let is_game = m.app_categories.iter().any(|c| c == "games");
    let is_dlc = !m.main_game_app_name.is_empty() && m.main_game_app_name != m.app_name;
    if !is_game || is_dlc || m.incomplete || (check_disk && !Path::new(&m.install_location).is_dir()) {
        return None;
    }
    let url = format!(
        "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
        m.catalog_namespace, m.catalog_item_id, m.app_name
    );
    let mut app = game(&m.display_name, Launcher::Epic, ItemAction::OpenUrl(url));
    if !m.launch_executable.is_empty() {
        app.icon_from = Some(Path::new(&m.install_location).join(&m.launch_executable).display().to_string());
    }
    Some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GTA: &str = r#"{
        "DisplayName": "Grand Theft Auto V Enhanced",
        "AppName": "8769e24080ea413b8ebca3f1b8c50951",
        "CatalogNamespace": "b0cd075465c44f87be3b505ac04a2e46",
        "CatalogItemId": "122e5e90b7b8424d930be8bc1a7e05fb",
        "InstallLocation": "C:\\Program Files\\Epic Games/GTAVEnhanced",
        "LaunchExecutable": "PlayGTAV.exe",
        "AppCategories": ["games", "applications"],
        "MainGameAppName": "8769e24080ea413b8ebca3f1b8c50951",
        "bIsIncompleteInstall": false
    }"#;

    #[test]
    fn builds_launch_url_and_icon_source() {
        let app = from_manifest(GTA, false).unwrap();
        assert_eq!(app.name, "Grand Theft Auto V Enhanced");
        assert_eq!(
            app.action,
            ItemAction::OpenUrl(
                "com.epicgames.launcher://apps/b0cd075465c44f87be3b505ac04a2e46%3A122e5e90b7b8424d930be8bc1a7e05fb%3A8769e24080ea413b8ebca3f1b8c50951?action=launch&silent=true".into()
            )
        );
        assert!(app.icon_from.unwrap().ends_with("PlayGTAV.exe"));
        assert_eq!(app.location, "Epic Games");
    }

    #[test]
    fn skips_dlc_tools_and_incomplete_installs() {
        let dlc = GTA.replace(r#""MainGameAppName": "8769e24080ea413b8ebca3f1b8c50951""#, r#""MainGameAppName": "other""#);
        assert!(from_manifest(&dlc, false).is_none());
        let tool = GTA.replace(r#"["games", "applications"]"#, r#"["applications"]"#);
        assert!(from_manifest(&tool, false).is_none());
        let partial = GTA.replace(r#""bIsIncompleteInstall": false"#, r#""bIsIncompleteInstall": true"#);
        assert!(from_manifest(&partial, false).is_none());
    }
}
