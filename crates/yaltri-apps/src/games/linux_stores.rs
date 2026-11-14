//! Linux launchers: Heroic (Epic via Legendary, GOG) and Lutris.
//! Parsers are platform independent so they are tested everywhere.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use yaltri_core::ItemAction;

use super::{Launcher, game};
use crate::App;

/// Heroic's config folders (native and Flatpak) that exist.
pub fn heroic_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    [config.join("heroic"), home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

pub fn heroic(root: &Path) -> Vec<App> {
    let mut games = Vec::new();
    if let Ok(text) = std::fs::read_to_string(root.join("legendaryConfig/legendary/installed.json")) {
        games.extend(parse_legendary(&text));
    }
    if let Ok(text) = std::fs::read_to_string(root.join("gog_store/installed.json")) {
        games.extend(parse_heroic_gog(&text));
    }
    games
}

fn heroic_launch(app_name: &str, runner: &str) -> ItemAction {
    ItemAction::OpenUrl(format!("heroic://launch?appName={app_name}&runner={runner}"))
}

/// Legendary's `installed.json`: `{ "<appName>": { "title", "is_dlc", ... } }`.
fn parse_legendary(text: &str) -> Vec<App> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    map.iter()
        .filter(|(_, v)| !v.get("is_dlc").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|(app_name, v)| {
            let title = v.get("title")?.as_str()?;
            Some(game(title, Launcher::Heroic, heroic_launch(app_name, "legendary")))
        })
        .collect()
}

#[derive(Deserialize)]
struct GogInstalled {
    #[serde(default)]
    installed: Vec<GogEntry>,
}

#[derive(Deserialize)]
struct GogEntry {
    #[serde(rename = "appName")]
    app_name: String,
    install_path: String,
    #[serde(default, rename = "is_dlc")]
    is_dlc: bool,
}

/// Heroic's GOG list has no titles; the install folder is named after the game.
fn parse_heroic_gog(text: &str) -> Vec<App> {
    let Ok(list) = serde_json::from_str::<GogInstalled>(text) else { return Vec::new() };
    list.installed
        .into_iter()
        .filter(|e| !e.is_dlc)
        .filter_map(|e| {
            let title = Path::new(&e.install_path).file_name()?.to_string_lossy().into_owned();
            Some(game(&title, Launcher::Heroic, heroic_launch(&e.app_name, "gog")))
        })
        .collect()
}

/// Installed Lutris games via its CLI (`lutris -l -o -j`).
pub fn lutris() -> Vec<App> {
    let Ok(out) = std::process::Command::new("lutris").args(["--list-games", "--installed", "--json"]).output() else {
        return Vec::new();
    };
    // Lutris may print log lines before the JSON array.
    let text = String::from_utf8_lossy(&out.stdout);
    let json = text.find('[').map_or("", |i| &text[i..]);
    let icons = std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share/icons/hicolor/128x128/apps"));
    parse_lutris(json, icons.as_deref())
}

#[derive(Deserialize)]
struct LutrisGame {
    id: u64,
    slug: String,
    name: String,
}

fn parse_lutris(json: &str, icon_dir: Option<&Path>) -> Vec<App> {
    let Ok(list) = serde_json::from_str::<Vec<LutrisGame>>(json) else { return Vec::new() };
    list.into_iter()
        .map(|g| {
            let mut app = game(&g.name, Launcher::Lutris, ItemAction::OpenUrl(format!("lutris:rungameid/{}", g.id)));
            app.icon = icon_dir.map(|d| d.join(format!("lutris_{}.png", g.slug))).filter(|p| p.is_file());
            app
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legendary_games_without_dlc() {
        let apps = parse_legendary(
            r#"{"Fortnite": {"title": "Fortnite", "is_dlc": false},
                "dlc1": {"title": "Some DLC", "is_dlc": true}}"#,
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].action, ItemAction::OpenUrl("heroic://launch?appName=Fortnite&runner=legendary".into()));
        assert_eq!(apps[0].location, "Heroic");
    }

    #[test]
    fn heroic_gog_titles_from_folders() {
        let apps = parse_heroic_gog(
            r#"{"installed": [{"appName": "1207658924", "install_path": "/home/me/Games/Heroic/Witcher 3", "platform": "windows"}]}"#,
        );
        assert_eq!(apps[0].name, "Witcher 3");
        assert_eq!(apps[0].action, ItemAction::OpenUrl("heroic://launch?appName=1207658924&runner=gog".into()));
    }

    #[test]
    fn lutris_json() {
        let apps = parse_lutris(r#"[{"id": 7, "slug": "celeste", "name": "Celeste", "runner": "linux"}]"#, None);
        assert_eq!(apps[0].name, "Celeste");
        assert_eq!(apps[0].action, ItemAction::OpenUrl("lutris:rungameid/7".into()));
        assert!(parse_lutris("not json", None).is_empty());
    }
}
