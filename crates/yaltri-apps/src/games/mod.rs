//! Installed games from PC game launchers, as launcher entries that start
//! the game through its launcher (so login, overlays and DRM work).

#[cfg(windows)]
mod epic;
mod linux_stores;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod riot;
mod steam;
mod vdf;
#[cfg(windows)]
mod windows_stores;

use yaltri_core::ItemAction;
use yaltri_core::settings::GamesSettings;

use crate::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Launcher {
    Steam,
    Epic,
    Gog,
    Ubisoft,
    Ea,
    Riot,
    Xbox,
    Heroic,
    Lutris,
}

impl Launcher {
    /// Launchers that exist on this platform, in settings order.
    pub const PLATFORM: &'static [Launcher] = if cfg!(windows) {
        &[Launcher::Steam, Launcher::Xbox, Launcher::Epic, Launcher::Riot, Launcher::Gog, Launcher::Ubisoft, Launcher::Ea]
    } else {
        &[Launcher::Steam, Launcher::Heroic, Launcher::Lutris]
    };

    pub fn display_name(self) -> &'static str {
        match self {
            Launcher::Steam => "Steam",
            Launcher::Epic => "Epic Games",
            Launcher::Gog => "GOG",
            Launcher::Ubisoft => "Ubisoft Connect",
            Launcher::Ea => "EA app",
            Launcher::Xbox => "Xbox",
            Launcher::Riot => "Riot Games",
            Launcher::Heroic => "Heroic",
            Launcher::Lutris => "Lutris",
        }
    }

    pub fn enabled_in(self, s: &GamesSettings) -> bool {
        match self {
            Launcher::Steam => s.steam,
            Launcher::Epic => s.epic,
            Launcher::Gog => s.gog,
            Launcher::Ubisoft => s.ubisoft,
            Launcher::Ea => s.ea,
            Launcher::Xbox => s.xbox,
            Launcher::Riot => s.riot,
            Launcher::Heroic => s.heroic,
            Launcher::Lutris => s.lutris,
        }
    }

    pub fn set_in(self, s: &mut GamesSettings, on: bool) {
        let field = match self {
            Launcher::Steam => &mut s.steam,
            Launcher::Epic => &mut s.epic,
            Launcher::Gog => &mut s.gog,
            Launcher::Ubisoft => &mut s.ubisoft,
            Launcher::Ea => &mut s.ea,
            Launcher::Xbox => &mut s.xbox,
            Launcher::Riot => &mut s.riot,
            Launcher::Heroic => &mut s.heroic,
            Launcher::Lutris => &mut s.lutris,
        };
        *field = on;
    }
}

/// A game entry; the subtitle names the launcher.
fn game(name: &str, launcher: Launcher, action: ItemAction) -> App {
    App {
        name: name.trim().to_owned(),
        keywords: vec!["oyun".into(), "game".into()],
        location: launcher.display_name().to_owned(),
        action,
        icon: None,
        icon_from: None,
        launcher: Some(launcher),
    }
}

/// Every installed game of the enabled launchers on this platform.
pub fn discover(settings: &GamesSettings) -> Vec<App> {
    let mut games = Vec::new();
    for &launcher in Launcher::PLATFORM {
        if !launcher.enabled_in(settings) {
            continue;
        }
        let found = discover_one(launcher);
        tracing::debug!(launcher = launcher.display_name(), count = found.len(), "games discovered");
        games.extend(found);
    }
    games
}

fn discover_one(launcher: Launcher) -> Vec<App> {
    match launcher {
        Launcher::Steam => steam::roots().iter().flat_map(|r| steam::discover(r)).collect(),
        #[cfg(windows)]
        Launcher::Epic => epic::manifests_dir().map(|d| epic::discover(&d)).unwrap_or_default(),
        #[cfg(windows)]
        Launcher::Gog => windows_stores::gog(),
        #[cfg(windows)]
        Launcher::Ubisoft => windows_stores::ubisoft(),
        #[cfg(windows)]
        Launcher::Ea => windows_stores::ea(),
        #[cfg(windows)]
        Launcher::Riot => riot::discover(),
        Launcher::Heroic => linux_stores::heroic_roots().iter().flat_map(|r| linux_stores::heroic(r)).collect(),
        Launcher::Lutris if !cfg!(windows) => linux_stores::lutris(),
        // Xbox games come with the Store app listing (see `windows::store_apps`).
        _ => Vec::new(),
    }
}
