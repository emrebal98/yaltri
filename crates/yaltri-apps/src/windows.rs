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

/// Store/UWP apps (Calculator, Settings, Photos...). Desktop apps are already
/// covered by Start Menu shortcuts, so only package ids (`...!App`) are kept.
pub fn store_apps() -> Vec<App> {
    let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
                  Get-StartApps | Select-Object Name,AppID | ConvertTo-Json -Compress";
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
    let json = json.trim();
    // A single result serializes as an object, not an array.
    let items: Vec<StartApp> = if json.starts_with('[') {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        serde_json::from_str::<StartApp>(json).map(|a| vec![a]).unwrap_or_default()
    };
    items
        .into_iter()
        .filter(|a| a.app_id.contains('!') && !is_noise(&a.name))
        .map(|a| App {
            location: "Microsoft Store uygulaması".into(),
            action: ItemAction::OpenUrl(format!("shell:AppsFolder\\{}", a.app_id)),
            name: a.name,
            keywords: Vec::new(),
            icon: None,
            icon_from: None,
            launcher: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_store_apps_and_skips_desktop_ids() {
        let json = r#"[{"Name":"Hesap Makinesi","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"},
                       {"Name":"Notepad++","AppID":"{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe"}]"#;
        let apps = parse_start_apps(json);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Hesap Makinesi");
        assert_eq!(
            apps[0].action,
            ItemAction::OpenUrl("shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into())
        );
        assert_eq!(parse_start_apps(r#"{"Name":"X","AppID":"a!b"}"#).len(), 1);
        assert!(parse_start_apps("garbage").is_empty());
    }

    #[test]
    fn filters_uninstallers() {
        assert!(is_noise("Uninstall Foo"));
        assert!(is_noise("Foo Kaldır"));
        assert!(!is_noise("Visual Studio Code"));
    }
}
