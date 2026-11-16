//! What happens to a finished screenshot: always on the clipboard, and with
//! Save also a PNG in the screenshots folder.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui_kit::App;
use yaltri_core::settings::ShotSettings;
use yaltri_platform::screen::Screenshot;

use crate::daemon::Daemon;
use crate::i18n::{t, tf};
use yaltri_platform::ocr::OcrError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Copy,
    Save,
    /// Copy the text in it.
    Text,
}

pub fn finish(action: Action, shot: Screenshot, cx: &mut App) {
    if action == Action::Text {
        copy_text(shot, cx);
        return;
    }
    let daemon = cx.global_mut::<Daemon>();
    match daemon.image_clipboard.set_bgra(shot.width, shot.height, &shot.bgra) {
        Ok(()) => tracing::info!(width = shot.width, height = shot.height, "screenshot copied"),
        Err(e) => tracing::warn!("copying the screenshot failed: {e:#}"),
    }
    if action == Action::Save {
        let dir = save_dir(&daemon.settings.shot);
        cx.background_executor()
            .spawn(async move {
                match save(&shot, &dir) {
                    Ok(path) => tracing::info!(path = %path.display(), "screenshot saved"),
                    Err(e) => tracing::warn!("saving the screenshot failed: {e:#}"),
                }
            })
            .detach();
    }
}

/// Reads the text in `shot` off the UI thread, copies it and says how it
/// went.
fn copy_text(shot: Screenshot, cx: &mut App) {
    let task = cx
        .background_executor()
        .spawn(async move { yaltri_platform::ocr::recognize(shot.width, shot.height, &shot.bgra) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(text) if text.trim().is_empty() => crate::notice::show(t("ocr.empty"), true, cx),
            Ok(text) => {
                let lines = text.lines().count();
                tracing::info!(lines, "text copied");
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                crate::notice::show(tf("ocr.copied", &[("n", &lines.to_string())]), false, cx);
            }
            Err(OcrError::MissingTesseract) => crate::notice::show(t("ocr.tesseract"), true, cx),
            Err(OcrError::NoLanguage) => crate::notice::show(t("ocr.no_language"), true, cx),
            Err(e) => {
                tracing::warn!("reading text failed: {e}");
                crate::notice::show(t("ocr.failed"), true, cx);
            }
        });
    })
    .detach();
}

/// The folder screenshots are saved to.
pub fn save_dir(settings: &ShotSettings) -> PathBuf {
    if !settings.save_dir.as_os_str().is_empty() {
        return settings.save_dir.clone();
    }
    let pictures = directories::UserDirs::new()
        .and_then(|u| u.picture_dir().map(Path::to_path_buf).or_else(|| Some(u.home_dir().join("Pictures"))))
        .unwrap_or_else(|| PathBuf::from("."));
    pictures.join("Screenshots")
}

fn save(shot: &Screenshot, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let path = dir.join(file_name(&stamp, |name| dir.join(name).exists()));
    let rgba = yaltri_platform::clipboard::bgra_to_rgba(&shot.bgra);
    let image = image::RgbaImage::from_raw(shot.width, shot.height, rgba).context("screenshot buffer size")?;
    image.save_with_format(&path, image::ImageFormat::Png).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// `Yaltri_<stamp>.png`, or `Yaltri_<stamp>_2.png`… when taken.
pub fn file_name(stamp: &str, exists: impl Fn(&str) -> bool) -> String {
    let base = format!("Yaltri_{stamp}");
    let mut name = format!("{base}.png");
    let mut n = 2;
    while exists(&name) {
        name = format!("{base}_{n}.png");
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_names_that_are_taken() {
        assert_eq!(file_name("2026-11-17_10-00-00", |_| false), "Yaltri_2026-11-17_10-00-00.png");
        let taken = ["Yaltri_s.png", "Yaltri_s_2.png"];
        assert_eq!(file_name("s", |n| taken.contains(&n)), "Yaltri_s_3.png");
    }

    #[test]
    fn uses_the_chosen_folder_or_pictures() {
        let chosen = ShotSettings { save_dir: PathBuf::from("/tmp/shots"), ..Default::default() };
        assert_eq!(save_dir(&chosen), PathBuf::from("/tmp/shots"));
        assert!(save_dir(&ShotSettings::default()).ends_with("Screenshots"));
    }
}
