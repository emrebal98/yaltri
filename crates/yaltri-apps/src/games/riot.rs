//! Riot Games (Windows): products the Riot Client installed, launched through
//! the client like Riot's own shortcuts do.
//!
//! `ProgramData\Riot Games\RiotClientInstalls.json` points at the client;
//! `Metadata\<product>.<patchline>\<product>.<patchline>.product_settings.yaml`
//! has `product_install_full_path`. Metadata folders outlive uninstalls, so a
//! product only counts when that folder still exists.

use std::path::{Path, PathBuf};

use yaltri_core::ItemAction;

use super::{Launcher, game};
use crate::App;

/// Riot product ids and their titles.
const TITLES: [(&str, &str); 5] = [
    ("valorant", "VALORANT"),
    ("league_of_legends", "League of Legends"),
    ("teamfighttactics", "Teamfight Tactics"),
    ("bacon", "Legends of Runeterra"),
    ("lion", "2XKO"),
];

/// Exe that carries each product's icon, relative to its install folder.
const ICON_EXES: [(&str, &str); 4] = [
    ("valorant", "VALORANT.exe"),
    ("league_of_legends", "LeagueClient.exe"),
    ("teamfighttactics", "LeagueClient.exe"),
    ("bacon", "LoR.exe"),
];

fn data_dir() -> Option<PathBuf> {
    let dir = Path::new(&std::env::var_os("ProgramData")?).join("Riot Games");
    dir.is_dir().then_some(dir)
}

pub fn discover() -> Vec<App> {
    let Some(data) = data_dir() else { return Vec::new() };
    let Some(client) = client_path(&data) else { return Vec::new() };
    let Ok(dirs) = std::fs::read_dir(data.join("Metadata")) else { return Vec::new() };
    dirs.flatten()
        .filter_map(|d| {
            let folder = d.file_name().to_string_lossy().into_owned();
            let yaml = std::fs::read_to_string(d.path().join(format!("{folder}.product_settings.yaml"))).ok()?;
            from_metadata(&folder, &yaml, &client, true)
        })
        .collect()
}

/// `rc_default` from `RiotClientInstalls.json`, if the client exists.
fn client_path(data: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data.join("RiotClientInstalls.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let path = json.get("rc_default").or_else(|| json.get("rc_live"))?.as_str()?.replace('/', "\\");
    Path::new(&path).is_file().then_some(path)
}

/// `folder` is `<product>.<patchline>`; only the live patchline counts.
fn from_metadata(folder: &str, yaml: &str, client: &str, check_disk: bool) -> Option<App> {
    let (product, patchline) = folder.split_once('.')?;
    if patchline != "live" {
        return None;
    }
    let install = yaml_value(yaml, "product_install_full_path")?;
    let install = PathBuf::from(install.replace('/', "\\"));
    if check_disk && !install.is_dir() {
        return None;
    }
    let title = TITLES.iter().find(|(id, _)| *id == product).map_or(product, |(_, t)| *t);
    let action = ItemAction::Run {
        program: client.to_owned(),
        args: vec![format!("--launch-product={product}"), format!("--launch-patchline={patchline}")],
        terminal: false,
    };
    let mut app = game(title, Launcher::Riot, action);
    let icon_exe = ICON_EXES.iter().find(|(id, _)| *id == product).map(|(_, exe)| install.join(exe));
    app.icon_from = icon_exe.filter(|p| !check_disk || p.is_file()).map(|p| p.display().to_string());
    Some(app)
}

/// Top-level `key: "value"` from the simple YAML Riot writes.
fn yaml_value(yaml: &str, key: &str) -> Option<String> {
    yaml.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.trim_start().strip_prefix(':')?.trim();
        let value = rest.trim_matches('"');
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALORANT: &str = "auto_patching_enabled_by_player: false\n\
        product_install_full_path: \"C:/Riot Games/VALORANT/live\"\n\
        product_install_root: \"C:/Riot Games/\"\n";
    const CLIENT: &str = r"C:\Riot Games\Riot Client\RiotClientServices.exe";

    #[test]
    fn live_products_launch_through_the_client() {
        let app = from_metadata("valorant.live", VALORANT, CLIENT, false).unwrap();
        assert_eq!(app.name, "VALORANT");
        assert_eq!(app.location, "Riot Games");
        assert_eq!(
            app.action,
            ItemAction::Run {
                program: CLIENT.into(),
                args: vec!["--launch-product=valorant".into(), "--launch-patchline=live".into()],
                terminal: false,
            }
        );
        assert_eq!(app.icon_from.as_deref(), Some(r"C:\Riot Games\VALORANT\live\VALORANT.exe"));
    }

    #[test]
    fn skips_pbe_and_products_without_an_install_path() {
        assert!(from_metadata("league_of_legends.pbe", VALORANT, CLIENT, false).is_none());
        // TFT's leftover settings only have the root, not a full path.
        let leftover = "product_install_root: \"C:/Riot Games/\"\n";
        assert!(from_metadata("teamfighttactics.live", leftover, CLIENT, false).is_none());
        // On disk check: the folder does not exist here.
        assert!(from_metadata("valorant.live", VALORANT, CLIENT, true).is_none() || Path::new(r"C:\Riot Games\VALORANT\live").is_dir());
    }

    #[test]
    fn unknown_products_keep_their_id() {
        let app = from_metadata("newgame.live", VALORANT, CLIENT, false).unwrap();
        assert_eq!(app.name, "newgame");
    }
}
