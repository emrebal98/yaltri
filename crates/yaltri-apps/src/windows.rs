//! Windows application discovery: Start Menu shortcuts, plus Store/UWP apps
//! from `Get-StartApps` (which only PowerShell exposes without COM).

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use yaltri_core::ItemAction;

use crate::App;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn start_menu_dirs() -> Vec<PathBuf> {
    let rel = r"Microsoft\Windows\Start Menu\Programs";
    ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|base| Path::new(&base).join(rel))
        .collect()
}

/// Shortcuts that are not apps a person wants to launch.
fn is_noise(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["uninstall", "kaldır", "readme", "release notes", "documentation", "help", "license"]
        .iter()
        .any(|w| lower.contains(w))
}

pub fn start_menu_apps() -> Vec<App> {
    let mut apps = Vec::new();
    for dir in start_menu_dirs() {
        collect(&dir, &mut apps);
    }
    apps
}

fn collect(dir: &Path, out: &mut Vec<App>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect(&path, out);
            continue;
        }
        let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase());
        if !matches!(ext.as_deref(), Some("lnk" | "url" | "appref-ms")) {
            continue;
        }
        let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        if is_noise(&name) {
            continue;
        }
        out.push(App {
            name,
            keywords: Vec::new(),
            location: path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            action: ItemAction::Open(path),
            icon: None,
            icon_from: None,
            launcher: None,
        });
    }
}

#[derive(Deserialize)]
struct StartApp {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "AppID")]
    app_id: String,
}

/// An Xbox (PC Game Pass / Microsoft Store) game package: it ships a
/// `MicrosoftGame.config`, which plain Store apps do not.
#[derive(Deserialize)]
struct XboxGame {
    #[serde(rename = "Pfn")]
    package_family: String,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Logo", default)]
    logo: Option<String>,
}

#[derive(Deserialize)]
struct StoreListing {
    #[serde(rename = "Apps", default)]
    apps: Vec<StartApp>,
    #[serde(rename = "Games", default)]
    games: Vec<XboxGame>,
}

/// Lists Start apps plus the packages that are Xbox games, as one JSON object.
const STORE_SCRIPT: &str = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$apps = @(Get-StartApps | Select-Object Name, AppID)
$games = @(foreach ($p in Get-AppxPackage) {
    if (-not $p.InstallLocation) { continue }
    $cfg = Join-Path $p.InstallLocation 'MicrosoftGame.config'
    if (-not (Test-Path -LiteralPath $cfg)) { continue }
    try {
        [xml]$x = Get-Content -LiteralPath $cfg -Raw
        $v = $x.Game.ShellVisuals
        $logo = if ($v.Square150x150Logo) { Join-Path $p.InstallLocation $v.Square150x150Logo } else { $null }
        [pscustomobject]@{ Pfn = $p.PackageFamilyName; Name = $v.DefaultDisplayName; Logo = $logo }
    } catch {}
})
[pscustomobject]@{ Apps = $apps; Games = $games } | ConvertTo-Json -Compress -Depth 3
"#;

/// Store/UWP apps (Calculator, Settings, Photos...) and Xbox games. Desktop
/// apps are already covered by Start Menu shortcuts, so only package ids
/// (`...!App`) are kept.
pub fn store_apps() -> Vec<App> {
    let script = STORE_SCRIPT;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            tracing::warn!(status = %o.status, "Get-StartApps failed");
            return Vec::new();
        }
        Err(e) => {
            tracing::warn!(%e, "running powershell failed");
            return Vec::new();
        }
    };
    parse_start_apps(&String::from_utf8_lossy(&output.stdout))
}

fn parse_start_apps(json: &str) -> Vec<App> {
    let Ok(listing) = serde_json::from_str::<StoreListing>(json.trim()) else {
        return Vec::new();
    };
    listing
        .apps
        .into_iter()
        .filter(|a| a.app_id.contains('!') && !is_noise(&a.name))
        .map(|a| {
            let url = format!("shell:AppsFolder\\{}", a.app_id);
            let family = a.app_id.split('!').next().unwrap_or_default();
            match listing.games.iter().find(|g| g.package_family.eq_ignore_ascii_case(family)) {
                Some(g) => xbox_game(a, g, url),
                None => App {
                    location: "Microsoft Store uygulaması".into(),
                    action: ItemAction::OpenUrl(url),
                    name: a.name,
                    keywords: Vec::new(),
                    icon: None,
                    icon_from: None,
                    launcher: None,
                },
            }
        })
        .collect()
}

/// The game's full title from its config (the Start entry can be short:
/// Palworld is "Pal"); the short name stays searchable as a keyword.
fn xbox_game(entry: StartApp, game: &XboxGame, url: String) -> App {
    let title = game
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty() && !n.starts_with("ms-resource:"))
        .unwrap_or(&entry.name)
        .to_owned();
    let mut keywords = vec!["oyun".to_owned(), "game".to_owned(), "xbox".to_owned()];
    if !entry.name.eq_ignore_ascii_case(&title) {
        keywords.push(entry.name.clone());
    }
    let logo = game.logo.as_deref().map(std::path::PathBuf::from).filter(|p| p.is_file());
    App {
        name: title,
        keywords,
        location: crate::games::Launcher::Xbox.display_name().to_owned(),
        icon_from: logo.is_none().then(|| url.clone()),
        icon: logo,
        action: ItemAction::OpenUrl(url),
        launcher: Some(crate::games::Launcher::Xbox),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_store_apps_and_skips_desktop_ids() {
        let json = r#"{"Apps":[{"Name":"Hesap Makinesi","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"},
                       {"Name":"Notepad++","AppID":"{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe"}],"Games":[]}"#;
        let apps = parse_start_apps(json);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Hesap Makinesi");
        assert_eq!(
            apps[0].action,
            ItemAction::OpenUrl("shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into())
        );
        assert!(apps[0].launcher.is_none());
        assert!(parse_start_apps("garbage").is_empty());
    }

    #[test]
    fn xbox_games_get_their_full_title_and_launcher() {
        let json = r#"{"Apps":[{"Name":"Pal","AppID":"PocketpairInc.Palworld_ad4psfrxyesvt!AppPalShipping"},
                               {"Name":"Photos","AppID":"Microsoft.Windows.Photos_8wekyb3d8bbwe!App"}],
                       "Games":[{"Pfn":"PocketpairInc.Palworld_ad4psfrxyesvt","Name":"Palworld","Logo":"C:\\nope\\Logo.png"}]}"#;
        let apps = parse_start_apps(json);
        let pal = apps.iter().find(|a| a.launcher.is_some()).unwrap();
        assert_eq!(pal.name, "Palworld");
        assert_eq!(pal.location, "Xbox");
        assert!(pal.keywords.iter().any(|k| k == "Pal"), "short Start name stays searchable");
        assert_eq!(pal.action, ItemAction::OpenUrl("shell:AppsFolder\\PocketpairInc.Palworld_ad4psfrxyesvt!AppPalShipping".into()));
        // Missing logo file: fall back to the shell icon of the app.
        assert!(pal.icon.is_none() && pal.icon_from.is_some());
        assert!(apps.iter().any(|a| a.name == "Photos" && a.launcher.is_none()));
    }

    #[test]
    fn filters_uninstallers() {
        assert!(is_noise("Uninstall Foo"));
        assert!(is_noise("Foo Kaldır"));
        assert!(!is_noise("Visual Studio Code"));
    }
}
