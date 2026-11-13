//! Linux application discovery from freedesktop `.desktop` files.
//! The parser is platform independent so it is unit-tested everywhere.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use yaltri_core::ItemAction;

use crate::App;

/// `[Desktop Entry]` keys we care about, with localized values resolved.
#[derive(Debug, Default, PartialEq)]
pub struct DesktopEntry {
    pub name: String,
    pub generic_name: Option<String>,
    pub keywords: Vec<String>,
    pub exec: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub kind: Option<String>,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
    pub icon: Option<String>,
}

/// Parses the `[Desktop Entry]` group. `locale` like `tr_TR` picks
/// `Name[tr_TR]`, then `Name[tr]`, then `Name`.
pub fn parse(text: &str, locale: Option<&str>) -> Option<DesktopEntry> {
    let mut in_main = false;
    let mut raw: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            raw.entry(k.trim()).or_insert(v.trim());
        }
    }

    let localized = |key: &str| -> Option<String> {
        if let Some(loc) = locale {
            let lang = loc.split(['_', '.', '@']).next().unwrap_or(loc);
            for candidate in [format!("{key}[{loc}]"), format!("{key}[{lang}]")] {
                if let Some(v) = raw.get(candidate.as_str()) {
                    return Some(unescape(v));
                }
            }
        }
        raw.get(key).map(|v| unescape(v))
    };
    let flag = |key: &str| raw.get(key).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let list = |v: Option<String>| -> Vec<String> {
        v.map(|v| v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
            .unwrap_or_default()
    };

    Some(DesktopEntry {
        name: localized("Name")?,
        generic_name: localized("GenericName"),
        keywords: list(localized("Keywords")),
        exec: raw.get("Exec").map(|v| v.to_string()),
        terminal: flag("Terminal"),
        no_display: flag("NoDisplay"),
        hidden: flag("Hidden"),
        kind: raw.get("Type").map(|v| v.to_string()),
        only_show_in: list(raw.get("OnlyShowIn").map(|v| v.to_string())),
        not_show_in: list(raw.get("NotShowIn").map(|v| v.to_string())),
        icon: raw.get("Icon").map(|v| v.to_string()),
    })
}

fn unescape(v: &str) -> String {
    v.replace("\\s", " ").replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\")
}

/// Splits an `Exec` value into program + args, dropping field codes
/// (`%f`, `%U`, ...) since we launch without files.
pub fn parse_exec(exec: &str) -> Option<(String, Vec<String>)> {
    let words = shell_words::split(exec).ok()?;
    let mut out = Vec::with_capacity(words.len());
    for w in words {
        if w.len() == 2 && w.starts_with('%') {
            if w == "%%" {
                out.push("%".to_owned());
            }
            // Other field codes expand to nothing without arguments.
            continue;
        }
        out.push(w.replace("%%", "%"));
    }
    let mut it = out.into_iter();
    let program = it.next()?;
    Some((program, it.collect()))
}

impl DesktopEntry {
    /// Whether this entry should appear in a launcher on `desktops`
    /// (`XDG_CURRENT_DESKTOP`, e.g. `["ubuntu", "GNOME"]`).
    pub fn is_visible(&self, desktops: &[String]) -> bool {
        if self.no_display || self.hidden || self.kind.as_deref().is_some_and(|t| t != "Application") {
            return false;
        }
        let on = |list: &[String]| list.iter().any(|d| desktops.iter().any(|c| c.eq_ignore_ascii_case(d)));
        if !self.only_show_in.is_empty() && !on(&self.only_show_in) {
            return false;
        }
        !on(&self.not_show_in)
    }
}

/// Directories holding `.desktop` files, highest priority first.
pub fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());

    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(data_home);
    dirs.extend(data_dirs.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    // Flatpak/Snap export dirs, in case the session did not add them.
    if let Some(h) = &home {
        dirs.push(h.join(".local/share/flatpak/exports/share"));
    }
    dirs.push("/var/lib/flatpak/exports/share".into());
    dirs.push("/var/lib/snapd/desktop".into());

    let mut seen = std::collections::HashSet::new();
    dirs.into_iter().map(|d| d.join("applications")).filter(|d| seen.insert(d.clone())).collect()
}

/// Reads every visible application. Earlier directories shadow later ones
/// with the same desktop-file id (`sub/foo.desktop` -> `sub-foo.desktop`).
pub fn discover(dirs: &[PathBuf], locale: Option<&str>, desktops: &[String]) -> Vec<App> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let mut files = Vec::new();
        collect_desktop_files(dir, dir, &mut files);
        files.sort();
        for (id, path) in files {
            if !seen.insert(id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(entry) = parse(&text, locale) else { continue };
            if !entry.is_visible(desktops) {
                continue;
            }
            let Some((program, args)) = entry.exec.as_deref().and_then(parse_exec) else { continue };
            let mut keywords = entry.keywords;
            keywords.extend(entry.generic_name);
            apps.push(App {
                name: entry.name,
                keywords,
                location: path.display().to_string(),
                action: ItemAction::Run { program, args, terminal: entry.terminal },
            });
        }
    }
    apps
}

fn collect_desktop_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect_desktop_files(base, &path, out);
        } else if path.extension().is_some_and(|x| x == "desktop") {
            let id = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace(['/', '\\'], "-");
            out.push((id, path));
        }
    }
}

pub fn current_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| v.split(['.', '@']).next().unwrap_or(&v).to_owned())
}

pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.split(':').map(String::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "\
[Desktop Entry]
Version=1.0
Name=Firefox Web Browser
Name[tr]=Firefox Web Tarayıcısı
GenericName=Web Browser
Keywords=Internet;WWW;Browser;
Exec=firefox %u
Terminal=false
Type=Application
Icon=firefox

[Desktop Action new-window]
Name=Open a New Window
Exec=firefox --new-window %u
";

    #[test]
    fn parses_main_group_only_with_locale() {
        let e = parse(FIREFOX, Some("tr_TR")).unwrap();
        assert_eq!(e.name, "Firefox Web Tarayıcısı");
        assert_eq!(e.exec.as_deref(), Some("firefox %u"));
        assert_eq!(e.keywords, ["Internet", "WWW", "Browser"]);
        assert_eq!(e.generic_name.as_deref(), Some("Web Browser"));
        assert_eq!(parse(FIREFOX, None).unwrap().name, "Firefox Web Browser");
    }

    #[test]
    fn exec_field_codes_are_removed() {
        assert_eq!(parse_exec("firefox %u").unwrap(), ("firefox".into(), vec![]));
        assert_eq!(
            parse_exec("env FOO=1 \"/opt/My App/run\" --x %F").unwrap(),
            ("env".into(), vec!["FOO=1".into(), "/opt/My App/run".into(), "--x".into()])
        );
        assert_eq!(parse_exec("printf 100%%").unwrap(), ("printf".into(), vec!["100%".into()]));
    }

    #[test]
    fn visibility_rules() {
        let mut e = parse(FIREFOX, None).unwrap();
        let gnome = vec!["ubuntu".to_string(), "GNOME".to_string()];
        assert!(e.is_visible(&gnome));
        e.only_show_in = vec!["KDE".into()];
        assert!(!e.is_visible(&gnome));
        e.only_show_in.clear();
        e.not_show_in = vec!["GNOME".into()];
        assert!(!e.is_visible(&gnome));
        e.not_show_in.clear();
        e.no_display = true;
        assert!(!e.is_visible(&gnome));
    }

    #[test]
    fn discover_shadows_by_id() {
        let base = std::env::temp_dir().join(format!("yaltri-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (user, system) = (base.join("user"), base.join("system"));
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(system.join("sub")).unwrap();
        std::fs::write(user.join("firefox.desktop"), FIREFOX.replace("Name=Firefox Web Browser", "Name=My Firefox")).unwrap();
        std::fs::write(system.join("firefox.desktop"), FIREFOX).unwrap();
        std::fs::write(system.join("sub/tool.desktop"), "[Desktop Entry]\nType=Application\nName=Tool\nExec=tool\n").unwrap();
        std::fs::write(system.join("hidden.desktop"), "[Desktop Entry]\nType=Application\nName=H\nExec=h\nNoDisplay=true\n").unwrap();

        let apps = discover(&[user, system], None, &[]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["My Firefox", "Tool"]);
        std::fs::remove_dir_all(base).unwrap();
    }
}
