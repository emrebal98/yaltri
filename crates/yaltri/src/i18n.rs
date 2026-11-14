//! UI strings. Each language is one flat TOML file in `locales/`, embedded at
//! build time; English is the fallback for keys a translation lacks. Adding a
//! language = adding a file here and a line in `LANGUAGES`.
//!
//! `t("key")` returns the string; `tf("key", &[("name", value)])` fills
//! `{name}` placeholders.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// (code, file contents). The first one is the fallback.
const LANGUAGES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en.toml")), ("tr", include_str!("../locales/tr.toml"))];

type Table = HashMap<String, String>;

static TABLES: LazyLock<Vec<(&'static str, Table)>> = LazyLock::new(|| {
    LANGUAGES
        .iter()
        .map(|(code, text)| {
            (*code, toml::from_str::<Table>(text).unwrap_or_else(|e| panic!("locales/{code}.toml: {e}")))
        })
        .collect()
});

static CURRENT: RwLock<usize> = RwLock::new(0);

/// Picks the language: `setting` is "auto" (follow the system) or a code.
pub fn set_language(setting: &str) {
    let wanted = if setting == "auto" || setting.is_empty() { system_language() } else { setting.to_owned() };
    let index = TABLES.iter().position(|(code, _)| *code == wanted).unwrap_or(0);
    if let Ok(mut current) = CURRENT.write() {
        *current = index;
    }
}

/// The system UI language as a two-letter code ("tr-TR" -> "tr").
fn system_language() -> String {
    sys_locale::get_locale()
        .and_then(|l| l.split(['-', '_']).next().map(str::to_ascii_lowercase))
        .unwrap_or_else(|| "en".into())
}

/// Code of the active language.
#[cfg(test)]
pub fn current() -> &'static str {
    TABLES[*CURRENT.read().unwrap()].0
}

/// The string for `key`, from the active language or English. Unknown keys
/// come back as the key itself, so a missing string is visible, not blank.
pub fn t(key: &str) -> &'static str {
    let tables: &'static Vec<(&'static str, Table)> = &TABLES;
    let current = *CURRENT.read().unwrap();
    tables[current].1.get(key).or_else(|| tables[0].1.get(key)).map(String::as_str).unwrap_or_else(|| {
        tracing::warn!(key, "missing UI string");
        Box::leak(key.to_owned().into_boxed_str())
    })
}

/// `t(key)` with `{name}` placeholders replaced.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = t(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_key() {
        let reference: Vec<&String> = {
            let mut keys: Vec<_> = TABLES[0].1.keys().collect();
            keys.sort();
            keys
        };
        for (code, table) in TABLES.iter().skip(1) {
            let mut keys: Vec<_> = table.keys().collect();
            keys.sort();
            let missing: Vec<_> = reference.iter().filter(|k| !table.contains_key(k.as_str())).collect();
            let extra: Vec<_> = keys.iter().filter(|k| !TABLES[0].1.contains_key(k.as_str())).collect();
            assert!(missing.is_empty() && extra.is_empty(), "{code}: missing {missing:?}, extra {extra:?}");
        }
    }

    #[test]
    fn placeholders_match_between_languages() {
        let names = |s: &str| {
            let mut v: Vec<String> =
                s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(n, _)| n.to_owned())).collect();
            v.sort();
            v
        };
        for (key, en) in &TABLES[0].1 {
            for (code, table) in TABLES.iter().skip(1) {
                if let Some(other) = table.get(key) {
                    assert_eq!(names(en), names(other), "{code}: placeholders differ in {key}");
                }
            }
        }
    }

    #[test]
    fn falls_back_and_fills_placeholders() {
        set_language("tr");
        assert_eq!(current(), "tr");
        set_language("xx");
        assert_eq!(current(), "en");
        assert_eq!(tf("test.greeting", &[("name", "Yaltri")]), "Hello, Yaltri");
    }
}
