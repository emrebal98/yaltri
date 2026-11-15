//! GNOME: our global shortcuts as GNOME custom shortcuts.
//!
//! An X grab cannot catch most `Super+key` combinations on GNOME: the window
//! manager takes the keyboard as soon as Super (its overlay key) goes down
//! and replays other keys past grabs on the root window, so they never fire.
//! GNOME's own custom shortcuts (Settings › Keyboard › Custom Shortcuts) do,
//! on X11 and Wayland alike, so each tool's shortcut becomes one that runs
//! `yaltri toggle <tool>`.
//!
//! A shortcut GNOME already uses (`Super+Space` switches the keyboard layout)
//! would still win, so while Yaltri runs that accelerator is taken out of the
//! GNOME setting holding it (its other accelerators stay), much like the
//! Windows keyboard hook wins over the shell. What we took is written to a
//! file and given back when our shortcut changes or Yaltri quits; after a
//! crash, the next start still knows what to give back.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const CUSTOM_DIR: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
/// Our custom shortcuts are `<CUSTOM_DIR>yaltri-<id>/`.
const OUR_PREFIX: &str = "yaltri-";

/// One of our shortcuts as a GNOME custom shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Stable id (`launcher`); names the custom shortcut's settings path.
    pub id: String,
    /// Shown in GNOME's keyboard settings.
    pub name: String,
    /// `Super+Space` form.
    pub spec: String,
    /// Command line GNOME runs.
    pub command: String,
}

/// Whether this desktop takes custom shortcuts the GNOME way.
pub fn available() -> bool {
    is_gnome() && gsettings(&["get", MEDIA_KEYS, "custom-keybindings"]).is_ok()
}

/// Wait between rewriting our custom shortcuts and giving keys back.
const GIVE_BACK_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// Schemas whose keys are lists of accelerators.
const SCHEMAS: [&str; 4] = [
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.settings-daemon.plugins.media-keys",
];

/// One accelerator we took out of a GNOME setting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taken {
    pub schema: String,
    pub key: String,
    pub accel: String,
}

/// What we took from GNOME, persisted in `file`.
pub struct GnomeKeys {
    file: PathBuf,
    taken: Vec<Taken>,
}

impl GnomeKeys {
    pub fn load(file: PathBuf) -> Self {
        let taken = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { file, taken }
    }

    /// Makes `bindings` our only GNOME custom shortcuts: frees their keys
    /// from GNOME's own shortcuts, and gives back what we took earlier but no
    /// longer need. Returns the ids that could not be set up.
    ///
    /// Order matters: GNOME refuses a shortcut whose keys another one holds
    /// and does not retry later, so keys are taken before ours claim them
    /// and given back only after ours let go.
    pub fn apply(&mut self, bindings: &[Binding]) -> Vec<String> {
        let wanted: Vec<Combo> = bindings.iter().filter_map(|b| Combo::from_spec(&b.spec)).collect();
        self.take(&wanted);
        let failed = self.bind(bindings);
        self.give_back(&wanted);
        self.save();
        failed
    }

    /// Removes our custom shortcuts and gives everything back (on quit).
    pub fn restore_all(&mut self) {
        self.apply(&[]);
    }

    fn give_back(&mut self, wanted: &[Combo]) {
        let (keep, give_back): (Vec<Taken>, Vec<Taken>) = std::mem::take(&mut self.taken)
            .into_iter()
            .partition(|t| Combo::from_accel(&t.accel).is_some_and(|c| wanted.contains(&c)));
        self.taken = keep;
        if give_back.is_empty() {
            return;
        }
        // Our custom shortcut changes reach the shell over D-Bus; let it
        // release the keys first or it refuses its own shortcut.
        std::thread::sleep(GIVE_BACK_DELAY);
        for t in give_back {
            match give_back_one(&t) {
                Ok(()) => tracing::info!(schema = t.schema, key = t.key, accel = t.accel, "gave shortcut back to GNOME"),
                Err(e) => {
                    tracing::warn!("giving {} back to GNOME failed: {e:#}", t.accel);
                    self.taken.push(t);
                }
            }
        }
    }

    fn take(&mut self, wanted: &[Combo]) {
        if !wanted.is_empty() {
            for schema in SCHEMAS {
                let listing = match gsettings(&["list-recursively", schema]) {
                    Ok(out) => out,
                    Err(e) => {
                        tracing::debug!("reading {schema}: {e:#}");
                        continue;
                    }
                };
                for (key, accels) in parse_listing(&listing) {
                    let (clash, rest): (Vec<String>, Vec<String>) = accels
                        .into_iter()
                        .partition(|a| Combo::from_accel(a).is_some_and(|c| wanted.contains(&c)));
                    if clash.is_empty() {
                        continue;
                    }
                    if let Err(e) = gsettings(&["set", schema, &key, &format_list(&rest)]) {
                        tracing::warn!("freeing {clash:?} from {schema} {key} failed: {e:#}");
                        continue;
                    }
                    for accel in clash {
                        tracing::info!(schema, key, accel, "took shortcut from GNOME");
                        self.taken.push(Taken { schema: schema.into(), key: key.clone(), accel });
                    }
                }
            }
        }
    }

    /// Writes `bindings` as our GNOME custom shortcuts and removes our
    /// others. Returns the ids that could not be set up.
    fn bind(&mut self, bindings: &[Binding]) -> Vec<String> {
        let mut failed = Vec::new();
        let mut list = match gsettings(&["get", MEDIA_KEYS, "custom-keybindings"]).ok().and_then(|v| parse_list(v.trim())) {
            Some(list) => list,
            None => {
                tracing::warn!("cannot read GNOME custom shortcuts");
                return bindings.iter().map(|b| b.id.clone()).collect();
            }
        };
        let ours: Vec<String> = bindings.iter().map(|b| custom_path(&b.id)).collect();
        // Drop ours that are no longer wanted.
        let stale: Vec<String> =
            list.iter().filter(|p| is_ours(p) && !ours.contains(p)).cloned().collect();
        for path in &stale {
            let _ = gsettings(&["reset-recursively", &format!("{CUSTOM_SCHEMA}:{path}")]);
        }
        list.retain(|p| !stale.contains(p));

        for (binding, path) in bindings.iter().zip(&ours) {
            let Some(accel) = accel_from_spec(&binding.spec) else {
                failed.push(binding.id.clone());
                continue;
            };
            let schema = format!("{CUSTOM_SCHEMA}:{path}");
            let result = gsettings(&["set", &schema, "name", &binding.name])
                .and_then(|_| gsettings(&["set", &schema, "command", &binding.command]))
                .and_then(|_| gsettings(&["set", &schema, "binding", &accel]));
            match result {
                Ok(_) => {
                    if !list.contains(path) {
                        list.push(path.clone());
                    }
                    tracing::debug!(id = binding.id, accel, "GNOME custom shortcut set");
                }
                Err(e) => {
                    tracing::warn!("setting GNOME shortcut {accel} failed: {e:#}");
                    failed.push(binding.id.clone());
                }
            }
        }
        if let Err(e) = gsettings(&["set", MEDIA_KEYS, "custom-keybindings", &format_list(&list)]) {
            tracing::warn!("updating GNOME custom shortcut list failed: {e:#}");
            return bindings.iter().map(|b| b.id.clone()).collect();
        }
        failed
    }

    fn save(&self) {
        let result = if self.taken.is_empty() {
            match std::fs::remove_file(&self.file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            }
        } else {
            serde_json::to_string_pretty(&self.taken)
                .map_err(anyhow::Error::from)
                .and_then(|json| Ok(std::fs::write(&self.file, json)?))
        };
        if let Err(e) = result {
            tracing::warn!("saving {} failed: {e:#}", self.file.display());
        }
    }
}

fn custom_path(id: &str) -> String {
    format!("{CUSTOM_DIR}{OUR_PREFIX}{id}/")
}

fn is_ours(path: &str) -> bool {
    path.strip_prefix(CUSTOM_DIR).is_some_and(|rest| rest.starts_with(OUR_PREFIX))
}

/// `Super+Space` -> `<Super>space`, the form GNOME's settings take.
pub(crate) fn accel_from_spec(spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let key = parts.pop()?;
    let mut accel = String::new();
    for part in parts {
        accel.push_str(match modifier(part)? {
            "ctrl" => "<Control>",
            "alt" => "<Alt>",
            "shift" => "<Shift>",
            _ => "<Super>",
        });
    }
    let key = match key.to_ascii_lowercase().as_str() {
        "space" => "space".to_owned(),
        "enter" | "return" => "Return".to_owned(),
        "tab" => "Tab".to_owned(),
        "escape" | "esc" => "Escape".to_owned(),
        "backspace" => "BackSpace".to_owned(),
        "delete" => "Delete".to_owned(),
        "insert" => "Insert".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "pageup" => "Page_Up".to_owned(),
        "pagedown" => "Page_Down".to_owned(),
        "left" | "right" | "up" | "down" => {
            let mut k = key.to_ascii_lowercase();
            k[..1].make_ascii_uppercase();
            k
        }
        "printscreen" => "Print".to_owned(),
        "pause" => "Pause".to_owned(),
        ";" => "semicolon".to_owned(),
        "=" => "equal".to_owned(),
        "," => "comma".to_owned(),
        "-" => "minus".to_owned(),
        "." => "period".to_owned(),
        "/" => "slash".to_owned(),
        "`" => "grave".to_owned(),
        "[" => "bracketleft".to_owned(),
        "\\" => "backslash".to_owned(),
        "]" => "bracketright".to_owned(),
        "'" => "apostrophe".to_owned(),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => k.to_owned(),
        k if k.starts_with('f') && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => k.to_ascii_uppercase(),
        _ => return None,
    };
    accel.push_str(&key);
    Some(accel)
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("gnome")))
}

fn give_back_one(t: &Taken) -> Result<()> {
    let current = gsettings(&["get", &t.schema, &t.key])?;
    let mut accels = parse_list(current.trim()).context("unexpected gsettings value")?;
    if !accels.contains(&t.accel) {
        accels.insert(0, t.accel.clone());
        gsettings(&["set", &t.schema, &t.key, &format_list(&accels)])?;
    }
    Ok(())
}

fn gsettings(args: &[&str]) -> Result<String> {
    let out = Command::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Modifiers plus a normalized key name, comparable across our specs
/// (`Super+Space`) and GNOME accelerators (`<Super>space`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Combo {
    mods: BTreeSet<&'static str>,
    key: String,
}

impl Combo {
    fn from_spec(spec: &str) -> Option<Self> {
        let mut parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        let key = parts.pop()?;
        let mods = parts.into_iter().map(modifier).collect::<Option<_>>()?;
        Some(Self { mods, key: normalize_key(key) })
    }

    fn from_accel(accel: &str) -> Option<Self> {
        let mut mods = BTreeSet::new();
        let mut rest = accel.trim();
        while let Some(after) = rest.strip_prefix('<') {
            let (name, tail) = after.split_once('>')?;
            mods.insert(modifier(name)?);
            rest = tail;
        }
        (!rest.is_empty()).then(|| Self { mods, key: normalize_key(rest) })
    }
}

fn modifier(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "primary" => "ctrl",
        "alt" | "mod1" => "alt",
        "shift" => "shift",
        "super" | "win" | "mod4" | "meta" => "super",
        _ => return None,
    })
}

fn normalize_key(key: &str) -> String {
    let key = key.to_ascii_lowercase().replace('_', "");
    match key.as_str() {
        "return" => "enter".into(),
        "prior" => "pageup".into(),
        "next" => "pagedown".into(),
        "esc" => "escape".into(),
        "semicolon" => ";".into(),
        "equal" => "=".into(),
        "comma" => ",".into(),
        "minus" => "-".into(),
        "period" => ".".into(),
        "slash" => "/".into(),
        "grave" => "`".into(),
        "bracketleft" => "[".into(),
        "backslash" => "\\".into(),
        "bracketright" => "]".into(),
        "apostrophe" => "'".into(),
        _ => key,
    }
}

/// `gsettings list-recursively` lines with list values: (key, accelerators).
fn parse_listing(listing: &str) -> Vec<(String, Vec<String>)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut it = line.splitn(3, ' ');
            let (_schema, key, value) = (it.next()?, it.next()?, it.next()?);
            Some((key.to_owned(), parse_list(value)?))
        })
        .collect()
}

/// `['<Super>space', 'XF86Keyboard']` or `@as []`.
fn parse_list(value: &str) -> Option<Vec<String>> {
    let inner = value.trim().trim_start_matches("@as").trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    for item in inner.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        out.push(item.strip_prefix('\'')?.strip_suffix('\'')?.to_owned());
    }
    Some(out)
}

fn format_list(accels: &[String]) -> String {
    if accels.is_empty() {
        return "@as []".into();
    }
    let items: Vec<String> = accels.iter().map(|a| format!("'{a}'")).collect();
    format!("[{}]", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(spec: &str, accel: &str) -> bool {
        Combo::from_spec(spec).unwrap() == Combo::from_accel(accel).unwrap()
    }

    #[test]
    fn matches_our_specs_to_gnome_accelerators() {
        assert!(same("Super+Space", "<Super>space"));
        assert!(same("Ctrl+Alt+T", "<Primary><Alt>t"));
        assert!(same("Super+Shift+C", "<Shift><Super>c"));
        assert!(same("Ctrl+PageUp", "<Control>Page_Up"));
        assert!(same("Alt+Enter", "<Mod1>Return"));
        assert!(!same("Super+Space", "<Shift><Super>space"));
        assert!(!same("Super+Space", "XF86Keyboard"));
    }

    #[test]
    fn writes_gnome_accelerators() {
        assert_eq!(accel_from_spec("Super+Space").as_deref(), Some("<Super>space"));
        assert_eq!(accel_from_spec("Ctrl+Alt+Space").as_deref(), Some("<Control><Alt>space"));
        assert_eq!(accel_from_spec("Super+Shift+C").as_deref(), Some("<Super><Shift>c"));
        assert_eq!(accel_from_spec("Ctrl+PageUp").as_deref(), Some("<Control>Page_Up"));
        assert_eq!(accel_from_spec("Alt+F4").as_deref(), Some("<Alt>F4"));
        assert_eq!(accel_from_spec("Super+Left").as_deref(), Some("<Super>Left"));
        assert_eq!(accel_from_spec("Hyper+X"), None);
    }

    #[test]
    fn written_accelerators_match_their_spec() {
        for spec in ["Super+Space", "Ctrl+Alt+Space", "Super+Shift+C", "Ctrl+Enter", "Alt+F12", "Super+\\", "Ctrl+;"] {
            let accel = accel_from_spec(spec).unwrap();
            assert_eq!(Combo::from_accel(&accel), Combo::from_spec(spec), "{spec} -> {accel}");
        }
    }

    #[test]
    fn recognizes_only_our_custom_shortcuts() {
        assert!(is_ours(&custom_path("launcher")));
        assert!(!is_ours("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/"));
    }

    #[test]
    fn skips_what_it_cannot_read() {
        assert_eq!(Combo::from_accel("<Hyper>x"), None);
        assert_eq!(Combo::from_accel("<Super>"), None);
    }

    #[test]
    fn parses_list_values_only() {
        let listing = "org.gnome.desktop.wm.keybindings switch-input-source ['<Super>space', 'XF86Keyboard']\n\
                       org.gnome.desktop.wm.keybindings minimize @as []\n\
                       org.gnome.settings-daemon.plugins.media-keys max-screencast-length uint32 30\n";
        assert_eq!(
            parse_listing(listing),
            vec![
                ("switch-input-source".to_owned(), vec!["<Super>space".to_owned(), "XF86Keyboard".to_owned()]),
                ("minimize".to_owned(), vec![]),
            ]
        );
    }

    #[test]
    fn formats_what_gsettings_reads_back() {
        let accels = vec!["XF86Keyboard".to_owned()];
        assert_eq!(parse_list(&format_list(&accels)), Some(accels));
        assert_eq!(format_list(&[]), "@as []");
        assert_eq!(parse_list(&format_list(&[])), Some(vec![]));
    }
}
