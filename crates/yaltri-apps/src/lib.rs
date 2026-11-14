//! Installed-application search: Start Menu + Store apps on Windows,
//! `.desktop` files on Linux, plus games from PC game launchers.
//! Fuzzy-matched by name and keywords.

pub mod desktop;
pub mod games;
pub mod icons;
#[cfg(windows)]
mod windows;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use games::Launcher;
use yaltri_core::settings::GamesSettings;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use yaltri_core::{ItemAction, ItemKind, Scope, SearchItem, SearchProvider};

/// Apps rank above files but below calculator results.
const SCORE_BASE: u32 = 500;
const SCORE_MAX: u32 = 900;
const REFRESH_EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub keywords: Vec<String>,
    /// Where it was found, shown as the subtitle.
    pub location: String,
    pub action: ItemAction,
    pub icon: Option<PathBuf>,
    /// Shell item to take the icon from when it is not the action's target
    /// (a game started via URL, whose exe holds the icon). Windows only.
    pub icon_from: Option<String>,
    /// Set for games: which launcher provides it.
    pub launcher: Option<Launcher>,
}

/// Every app and enabled-launcher game currently installed, de-duplicated by
/// name (a Start Menu shortcut wins over the same game from a launcher).
pub fn discover(games: &GamesSettings) -> Vec<App> {
    #[cfg(windows)]
    let apps = {
        let mut apps = windows::start_menu_apps();
        apps.extend(windows::store_apps());
        apps
    };
    #[cfg(not(windows))]
    let mut apps = desktop::discover(
        &desktop::application_dirs(),
        desktop::current_locale().as_deref(),
        &desktop::current_desktops(),
    );
    #[cfg(windows)]
    let mut apps = apps;
    apps.extend(games::discover(games));
    // Games listed with the apps (Xbox) follow their launcher's switch too.
    apps.retain(|a| a.launcher.is_none_or(|l| l.enabled_in(games)));
    dedupe(apps)
}

/// Fills `icon` for apps that do not have one yet.
#[cfg_attr(not(windows), allow(unused_variables))]
fn attach_icons(apps: &mut [App], icon_cache: &std::path::Path) {
    #[cfg(windows)]
    for app in apps.iter_mut().filter(|a| a.icon.is_none()) {
        let parsing_name = match (&app.icon_from, &app.action) {
            // Launcher manifests mix '/' and '\'; the shell wants '\'.
            (Some(from), _) => from.replace('/', "\\"),
            // A game URL (steam://, uplay://) has no shell icon of its own.
            (None, _) if app.launcher.is_some() => continue,
            (None, ItemAction::Open(path)) => path.to_string_lossy().into_owned(),
            (None, ItemAction::OpenUrl(url)) => url.clone(),
            _ => continue,
        };
        app.icon = icons::icon_for(&parsing_name, icon_cache);
    }
}

fn dedupe(apps: Vec<App>) -> Vec<App> {
    let mut seen = HashSet::new();
    apps.into_iter().filter(|a| seen.insert(a.name.to_lowercase())).collect()
}

pub struct AppsProvider {
    apps: Arc<RwLock<Vec<App>>>,
    matcher: Mutex<Matcher>,
    /// Wakes the discovery thread with new game settings.
    refresh: Option<Sender<GamesSettings>>,
}

impl AppsProvider {
    /// Discovers apps and games in the background now, every few minutes,
    /// and whenever `set_games` changes which launchers are searched.
    /// `icon_cache` holds extracted icons (Windows).
    pub fn start(icon_cache: PathBuf, games: GamesSettings) -> Self {
        let apps: Arc<RwLock<Vec<App>>> = Arc::default();
        let shared = apps.clone();
        let (tx, rx) = mpsc::channel::<GamesSettings>();
        let _ = thread::Builder::new().name("yaltri-apps".into()).spawn(move || {
            let mut games = games;
            loop {
                let mut found = discover(&games);
                let game_count = found.iter().filter(|a| a.launcher.is_some()).count();
                tracing::info!(count = found.len(), games = game_count, "applications discovered");
                // Publish right away; icons follow (first run extracts them).
                *shared.write().unwrap() = found.clone();
                let started = std::time::Instant::now();
                attach_icons(&mut found, &icon_cache);
                let with_icon = found.iter().filter(|a| a.icon.is_some()).count();
                tracing::info!(with_icon, took = ?started.elapsed(), "application icons ready");
                *shared.write().unwrap() = found;
                match rx.recv_timeout(REFRESH_EVERY) {
                    Ok(new) => games = new,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { refresh: Some(tx), ..Self::with_apps(apps) }
    }

    fn with_apps(apps: Arc<RwLock<Vec<App>>>) -> Self {
        Self { apps, matcher: Mutex::new(Matcher::new(Config::DEFAULT)), refresh: None }
    }

    /// Re-discovers now with these launchers enabled.
    pub fn set_games(&self, games: GamesSettings) {
        if let Some(tx) = &self.refresh {
            let _ = tx.send(games);
        }
    }

    pub fn count(&self) -> usize {
        self.apps.read().unwrap().len()
    }

    /// Games found per launcher, for the settings window.
    pub fn game_counts(&self) -> HashMap<Launcher, usize> {
        let mut counts = HashMap::new();
        for launcher in self.apps.read().unwrap().iter().filter_map(|a| a.launcher) {
            *counts.entry(launcher).or_insert(0) += 1;
        }
        counts
    }
}

impl SearchProvider for AppsProvider {
    fn id(&self) -> &'static str {
        "apps"
    }

    fn query(&self, query: &str) -> Vec<SearchItem> {
        self.query_scoped(query, Scope::All)
    }

    fn query_scoped(&self, query: &str, scope: Scope) -> Vec<SearchItem> {
        let in_scope = |app: &App| scope.admits(item_kind(app));
        let query = query.trim();
        if query.is_empty() {
            if scope == Scope::All {
                return Vec::new();
            }
            // Inside a scope an empty query lists everything of that kind;
            // usage history then lifts the ones opened often.
            let apps = self.apps.read().unwrap();
            let mut items: Vec<SearchItem> = apps.iter().filter(|a| in_scope(a)).map(|a| item(a, SCORE_BASE)).collect();
            items.sort_by_cached_key(|i| i.title.to_lowercase());
            return items;
        }
        let lower = query.to_lowercase();
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = self.matcher.lock().unwrap();
        let mut buf = Vec::new();
        let apps = self.apps.read().unwrap();
        let mut items: Vec<SearchItem> = apps
            .iter()
            .filter(|app| in_scope(app))
            .filter_map(|app| {
                let name_lower = app.name.to_lowercase();
                let words: Vec<&str> = name_lower
                    .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
                    .filter(|w| !w.is_empty())
                    .collect();
                let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
                let keyword = app
                    .keywords
                    .iter()
                    .any(|k| k.to_lowercase().split_whitespace().any(|w| w.starts_with(&lower)));

                // Admission: a recognizable match, not scattered letters.
                let bonus = if name_lower == lower {
                    return Some(item(app, SCORE_MAX));
                } else if name_lower.starts_with(&lower) {
                    150
                } else if words.iter().any(|w| w.starts_with(&lower)) {
                    90
                } else if lower.chars().count() >= 2 && initials.starts_with(&lower) {
                    70
                } else if keyword {
                    50
                } else if lower.chars().count() >= 3 && name_lower.contains(&lower) {
                    20
                } else {
                    return None;
                };
                // Fuzzy quality only orders the admitted apps.
                let fuzzy = pattern.score(Utf32Str::new(&app.name, &mut buf), &mut matcher).unwrap_or(0);
                let score = SCORE_BASE + bonus + (fuzzy / 4).min(150);
                Some(item(app, score.min(SCORE_MAX - 1)))
            })
            .collect();
        items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.len().cmp(&b.title.len())));
        items.truncate(12);
        items
    }
}

/// Games are tagged apart from apps so the launcher can filter them.
fn item_kind(app: &App) -> ItemKind {
    if app.launcher.is_some() { ItemKind::Game } else { ItemKind::App }
}

fn item(app: &App, score: u32) -> SearchItem {
    SearchItem {
        title: app.name.clone(),
        subtitle: app.location.clone(),
        kind: item_kind(app),
        score,
        action: app.action.clone(),
        icon: app.icon.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scattered_letters_do_not_match() {
        let p = provider(vec![app("Epic Games Launcher", &[]), app("Calculator", &[])]);
        assert_eq!(titles(&p, "calc"), ["Calculator"]);
    }

    fn app(name: &str, keywords: &[&str]) -> App {
        App {
            name: name.into(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            location: String::new(),
            action: ItemAction::OpenUrl(name.into()),
            icon: None,
            icon_from: None,
            launcher: None,
        }
    }

    fn provider(apps: Vec<App>) -> AppsProvider {
        AppsProvider::with_apps(Arc::new(RwLock::new(apps)))
    }

    fn titles(p: &AppsProvider, q: &str) -> Vec<String> {
        p.query(q).into_iter().map(|i| i.title).collect()
    }

    #[test]
    fn scopes_split_apps_and_games() {
        let mut pal = app("Palworld", &[]);
        pal.launcher = Some(Launcher::Xbox);
        let p = provider(vec![app("Paint", &[]), pal]);
        let scoped = |q, scope| p.query_scoped(q, scope).into_iter().map(|i| (i.title, i.kind)).collect::<Vec<_>>();
        assert_eq!(scoped("pa", Scope::Games), [("Palworld".to_owned(), ItemKind::Game)]);
        assert_eq!(scoped("pa", Scope::Apps), [("Paint".to_owned(), ItemKind::App)]);
        assert_eq!(scoped("pa", Scope::All).len(), 2);
        // An empty query lists the whole scope, but nothing when unscoped.
        assert_eq!(scoped("", Scope::Games), [("Palworld".to_owned(), ItemKind::Game)]);
        assert!(scoped("", Scope::All).is_empty());
    }

    #[test]
    fn prefix_beats_fuzzy_and_exact_beats_all() {
        let p = provider(vec![
            app("Visual Studio Code", &[]),
            app("Discord", &[]),
            app("Code", &[]),
            app("Calculator", &["math"]),
        ]);
        assert_eq!(titles(&p, "code")[0], "Code");
        assert_eq!(titles(&p, "vsc"), ["Visual Studio Code"]);
        assert_eq!(titles(&p, "disc")[0], "Discord");
    }

    #[test]
    fn keywords_match() {
        let p = provider(vec![app("Firefox", &["Web Browser", "Internet"])]);
        assert_eq!(titles(&p, "brow"), ["Firefox"]);
    }

    #[test]
    fn short_scattered_matches_are_dropped() {
        let p = provider(vec![app("Visual Studio Code", &[])]);
        assert!(titles(&p, "vc").is_empty());
        assert_eq!(titles(&p, "vi"), ["Visual Studio Code"]);
    }

    #[test]
    fn scores_stay_in_band() {
        let p = provider(vec![app("Code", &[]), app("Codex Helper Tool", &[])]);
        for item in p.query("code") {
            assert!((SCORE_BASE..=SCORE_MAX).contains(&item.score), "{}", item.score);
        }
    }

    #[test]
    fn dedupes_by_name() {
        let apps = dedupe(vec![app("Code", &[]), app("code", &[]), app("Other", &[])]);
        assert_eq!(apps.len(), 2);
    }
}
