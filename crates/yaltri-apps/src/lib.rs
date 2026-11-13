//! Installed-application search: Start Menu + Store apps on Windows,
//! `.desktop` files on Linux. Fuzzy-matched by name and keywords.

pub mod desktop;
#[cfg(windows)]
mod windows;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use yaltri_core::{ItemAction, SearchItem, SearchProvider};

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
}

/// Every app currently installed, de-duplicated by name.
pub fn discover() -> Vec<App> {
    #[cfg(windows)]
    let apps = {
        let mut apps = windows::start_menu_apps();
        apps.extend(windows::store_apps());
        apps
    };
    #[cfg(not(windows))]
    let apps = desktop::discover(
        &desktop::application_dirs(),
        desktop::current_locale().as_deref(),
        &desktop::current_desktops(),
    );
    dedupe(apps)
}

fn dedupe(apps: Vec<App>) -> Vec<App> {
    let mut seen = HashSet::new();
    apps.into_iter().filter(|a| seen.insert(a.name.to_lowercase())).collect()
}

pub struct AppsProvider {
    apps: Arc<RwLock<Vec<App>>>,
    matcher: Mutex<Matcher>,
}

impl AppsProvider {
    /// Discovers apps in the background now and every few minutes.
    pub fn start() -> Self {
        let apps: Arc<RwLock<Vec<App>>> = Arc::default();
        let shared = apps.clone();
        let _ = thread::Builder::new().name("yaltri-apps".into()).spawn(move || {
            loop {
                let found = discover();
                tracing::info!(count = found.len(), "applications discovered");
                *shared.write().unwrap() = found;
                thread::sleep(REFRESH_EVERY);
            }
        });
        Self::with_apps(apps)
    }

    fn with_apps(apps: Arc<RwLock<Vec<App>>>) -> Self {
        Self { apps, matcher: Mutex::new(Matcher::new(Config::DEFAULT)) }
    }

    pub fn count(&self) -> usize {
        self.apps.read().unwrap().len()
    }
}

impl SearchProvider for AppsProvider {
    fn id(&self) -> &'static str {
        "apps"
    }

    fn query(&self, query: &str) -> Vec<SearchItem> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let lower = query.to_lowercase();
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = self.matcher.lock().unwrap();
        let mut buf = Vec::new();
        let apps = self.apps.read().unwrap();
        let mut items: Vec<SearchItem> = apps
            .iter()
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

fn item(app: &App, score: u32) -> SearchItem {
    SearchItem {
        title: app.name.clone(),
        subtitle: app.location.clone(),
        provider: "apps",
        score,
        action: app.action.clone(),
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
        }
    }

    fn provider(apps: Vec<App>) -> AppsProvider {
        AppsProvider::with_apps(Arc::new(RwLock::new(apps)))
    }

    fn titles(p: &AppsProvider, q: &str) -> Vec<String> {
        p.query(q).into_iter().map(|i| i.title).collect()
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
