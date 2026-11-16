//! Asset source: our Lucide icons (`assets/icons`, ISC) and brand marks
//! (`assets/brand`), embedded at build time, in front of GPUI Kit's own
//! component icons.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

macro_rules! embed {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../../../assets/", $path)) as &[u8])),*]
    };
}

const FILES: &[(&str, &[u8])] = embed!(
    "brand/app-icon.svg",
    "brand/mark-key.svg",
    "brand/mark-glint.svg",
    "icons/app-window.svg",
    "icons/calculator.svg",
    "icons/camera.svg",
    "icons/check.svg",
    "icons/chevron-down.svg",
    "icons/circle-alert.svg",
    "icons/copy.svg",
    "icons/corner-down-left.svg",
    "icons/download.svg",
    "icons/eraser.svg",
    "icons/external-link.svg",
    "icons/file.svg",
    "icons/folder.svg",
    "icons/folder-open.svg",
    "icons/gamepad-2.svg",
    "icons/minus.svg",
    "icons/monitor.svg",
    "icons/moon.svg",
    "icons/pencil.svg",
    "icons/pipette.svg",
    "icons/power.svg",
    "icons/refresh-cw.svg",
    "icons/search.svg",
    "icons/search-x.svg",
    "icons/settings.svg",
    "icons/square-dashed.svg",
    "icons/sun.svg",
    "icons/trash-2.svg",
    "icons/x.svg",
);

/// Raw bytes of an embedded asset (also used for the tray icon).
pub fn bytes(path: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(p, _)| *p == path).map(|(_, b)| *b)
}

/// An embedded SVG as text (menu icons drawn by the tray).
pub fn svg(path: &str) -> Option<&'static str> {
    bytes(path).and_then(|b| std::str::from_utf8(b).ok())
}

pub struct YaltriAssets;

impl AssetSource for YaltriAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = bytes(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(FILES.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
