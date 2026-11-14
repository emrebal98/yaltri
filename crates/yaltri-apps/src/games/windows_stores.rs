//! Windows launchers that record installs in the registry or ProgramData:
//! GOG Galaxy, Ubisoft Connect, EA app.

use std::path::{Path, PathBuf};

use yaltri_core::ItemAction;

use super::registry::{self, Hive};
use super::{Launcher, game};
use crate::App;

const GOG_KEY: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";
const UBISOFT_KEY: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";

/// GOG games are DRM-free: start the game's exe directly.
pub fn gog() -> Vec<App> {
    registry::subkeys(Hive::LocalMachine, GOG_KEY)
        .into_iter()
        .filter_map(|id| {
            let key = format!(r"{GOG_KEY}\{id}");
            let get = |v: &str| registry::string(Hive::LocalMachine, &key, v).filter(|s| !s.is_empty());
            // DLCs point at their base game.
            if get("dependsOn").is_some() {
                return None;
            }
            let name = get("gameName")?;
            let exe = PathBuf::from(get("exe")?);
            if !exe.is_file() {
                return None;
            }
            let mut app = game(&name, Launcher::Gog, ItemAction::Open(exe.clone()));
            app.icon_from = Some(exe.display().to_string());
            Some(app)
        })
        .collect()
}

/// Ubisoft only records the install folder; its name is the game's name.
pub fn ubisoft() -> Vec<App> {
    registry::subkeys(Hive::LocalMachine, UBISOFT_KEY)
        .into_iter()
        .filter_map(|id| {
            let dir = registry::string(Hive::LocalMachine, &format!(r"{UBISOFT_KEY}\{id}"), "InstallDir")?;
            let dir = PathBuf::from(dir.trim_end_matches(['/', '\\']));
            if !dir.is_dir() {
                return None;
            }
            let name = dir.file_name()?.to_string_lossy().into_owned();
            Some(game(&name, Launcher::Ubisoft, ItemAction::OpenUrl(format!("uplay://launch/{id}/0"))))
        })
        .collect()
}

/// EA app: `ProgramData\EA Desktop\InstallData\<title>\base-<offerId>`.
/// That folder outlives uninstalls, so a game only counts if its install
/// folder still exists under one of the usual library locations.
pub fn ea() -> Vec<App> {
    let Some(data) = std::env::var_os("ProgramData").map(|d| Path::new(&d).join(r"EA Desktop\InstallData")) else {
        return Vec::new();
    };
    let Ok(titles) = std::fs::read_dir(&data) else { return Vec::new() };
    let libraries = ea_libraries();
    titles
        .flatten()
        .filter_map(|title_dir| {
            let title = title_dir.file_name().to_string_lossy().into_owned();
            let offer = std::fs::read_dir(title_dir.path())
                .ok()?
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .find_map(|n| n.strip_prefix("base-").map(str::to_owned))?;
            let installed = libraries.iter().map(|lib| lib.join(&title)).find(|p| p.is_dir())?;
            let url = format!("origin2://game/launch?offerIds={offer}&autoDownload=0");
            let mut app = game(&title, Launcher::Ea, ItemAction::OpenUrl(url));
            app.icon_from = largest_exe(&installed).map(|p| p.display().to_string());
            Some(app)
        })
        .collect()
}

fn ea_libraries() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            v.push(Path::new(&pf).join("EA Games"));
            v.push(Path::new(&pf).join("Origin Games"));
        }
    }
    v
}

/// The game's main exe is usually the largest one in its root folder.
fn largest_exe(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .max_by_key(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .map(|e| e.path())
}
