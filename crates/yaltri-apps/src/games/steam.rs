//! Steam (Windows and Linux): every library folder, installed app manifests,
//! icons from Steam's own library cache, launched through `steam://`.

use std::path::{Path, PathBuf};

use yaltri_core::ItemAction;

use super::{Launcher, game, vdf};
use crate::App;

/// Not games: runtimes and redistributables Steam installs alongside them.
const TOOL_IDS: [&str; 6] = ["228980", "1070560", "1391110", "1628350", "1493710", "2180100"];
const TOOL_PREFIXES: [&str; 4] = ["Proton", "Steam Linux Runtime", "Steamworks", "SteamVR"];
/// `StateFlags` bit for "fully installed".
const FULLY_INSTALLED: u32 = 4;

/// Steam installation folders that exist on this machine.
pub fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    if let Some(p) = super::registry::string(super::registry::Hive::CurrentUser, r"Software\Valve\Steam", "SteamPath") {
        roots.push(PathBuf::from(p));
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            roots.push(home.join(rel));
        }
    }
    roots.retain(|r| r.join("steamapps").is_dir());
    // ~/.steam/steam is usually a symlink to ~/.local/share/Steam.
    let mut seen = std::collections::HashSet::new();
    roots.retain(|r| seen.insert(r.canonicalize().unwrap_or_else(|_| r.clone())));
    roots
}

pub fn discover(root: &Path) -> Vec<App> {
    let mut libraries = vec![root.to_path_buf()];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        let v = vdf::parse(&text);
        if let Some(folders) = v.get("libraryfolders") {
            for (_, lib) in folders.entries() {
                if let Some(p) = lib.str("path") {
                    let p = PathBuf::from(p);
                    if !libraries.iter().any(|l| same_path(l, &p)) {
                        libraries.push(p);
                    }
                }
            }
        }
    }

    let mut games = Vec::new();
    for lib in libraries {
        let Ok(dir) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            if let Some(g) = from_manifest(&text, root) {
                games.push(g);
            }
        }
    }
    games
}

fn from_manifest(text: &str, root: &Path) -> Option<App> {
    let v = vdf::parse(text);
    let state = v.get("AppState")?;
    let id = state.str("appid")?;
    let title = state.str("name")?;
    let flags: u32 = state.str("StateFlags").and_then(|f| f.parse().ok()).unwrap_or(0);
    if flags & FULLY_INSTALLED == 0 || TOOL_IDS.contains(&id) || TOOL_PREFIXES.iter().any(|p| title.starts_with(p)) {
        return None;
    }
    let mut app = game(title, Launcher::Steam, ItemAction::OpenUrl(format!("steam://rungameid/{id}")));
    app.icon = icon(root, id);
    Some(app)
}

/// Steam's cached library icon: `librarycache/<id>/<sha1>.jpg` (current
/// clients) or `librarycache/<id>_icon.jpg` (older ones).
fn icon(root: &Path, id: &str) -> Option<PathBuf> {
    let cache = root.join("appcache").join("librarycache");
    if let Ok(dir) = std::fs::read_dir(cache.join(id)) {
        for e in dir.flatten() {
            let path = e.path();
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let is_jpg = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg"));
            // Newer clients also keep same-named hash folders here.
            if is_jpg && path.is_file() && stem.len() == 40 && stem.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(path);
            }
        }
    }
    let old = cache.join(format!("{id}_icon.jpg"));
    old.is_file().then_some(old)
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn manifest(id: &str, name: &str, flags: u32) -> String {
        format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n}}\n")
    }

    #[test]
    fn finds_installed_games_across_libraries_and_skips_tools() {
        let base = std::env::temp_dir().join(format!("yaltri-steam-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (root, extra) = (base.join("Steam"), base.join("Library2"));
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::create_dir_all(extra.join("steamapps")).unwrap();
        fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}",
                root.display().to_string().replace('\\', "\\\\"),
                extra.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        fs::write(root.join("steamapps/appmanifest_1172470.acf"), manifest("1172470", "Apex Legends", 4)).unwrap();
        fs::write(root.join("steamapps/appmanifest_228980.acf"), manifest("228980", "Steamworks Common Redistributables", 4)).unwrap();
        fs::write(extra.join("steamapps/appmanifest_10.acf"), manifest("10", "Counter-Strike", 4)).unwrap();
        fs::write(extra.join("steamapps/appmanifest_20.acf"), manifest("20", "Half-Downloaded", 2)).unwrap();
        let icon_dir = root.join("appcache/librarycache/1172470");
        fs::create_dir_all(&icon_dir).unwrap();
        fs::write(icon_dir.join("8986dd626da56db5f3fe09bc1b8871739de8b00d.jpg"), b"").unwrap();
        fs::write(icon_dir.join("header.jpg"), b"").unwrap();
        // A hash-named folder must not be taken for the icon.
        fs::create_dir_all(icon_dir.join("1ef0aa349c9e4d17eb4982c6ce48d0b730d49375")).unwrap();

        let mut games = discover(&root);
        games.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Apex Legends", "Counter-Strike"]);
        assert_eq!(games[0].action, ItemAction::OpenUrl("steam://rungameid/1172470".into()));
        assert!(games[0].icon.as_ref().unwrap().ends_with("8986dd626da56db5f3fe09bc1b8871739de8b00d.jpg"));
        assert_eq!(games[0].location, "Steam");
        fs::remove_dir_all(base).unwrap();
    }
}
