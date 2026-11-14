//! Asset source: GPUI Kit's default component icons plus the few extra Lucide
//! icons Yaltri uses. Only these SVGs are embedded, not the whole catalog.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(ExtraIcons, [AppWindow, Calculator, Gamepad2, Pencil, Pipette]);

pub struct YaltriAssets;

impl AssetSource for YaltriAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
