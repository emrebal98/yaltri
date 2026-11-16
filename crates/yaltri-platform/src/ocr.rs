//! Text in an image.
//!
//! - Windows: the built-in `Windows.Media.Ocr` engine, in the user's
//!   languages; offline.
//! - Linux: the `tesseract` command, in Turkish and English when those
//!   language packs are installed.

use anyhow::Result;

/// Why text could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrError {
    /// Linux: `tesseract` is not installed.
    MissingTesseract,
    /// Windows: no OCR language is installed.
    NoLanguage,
    Failed(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::MissingTesseract => f.write_str("tesseract is not installed"),
            OcrError::NoLanguage => f.write_str("no OCR language is installed"),
            OcrError::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for OcrError {}

/// The text in a BGRA image, lines joined with `\n`. Blocking; run it off
/// the UI thread.
pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, OcrError> {
    #[cfg(windows)]
    return win::recognize(width, height, bgra);
    #[cfg(target_os = "linux")]
    return linux::recognize(width, height, bgra);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (width, height, bgra);
        Err(OcrError::Failed("text recognition is not supported on this system".into()))
    }
}

/// Small captures read badly; this is how much they are enlarged first.
fn upscale_factor(height: u32) -> u32 {
    match height {
        0..60 => 3,
        60..200 => 2,
        _ => 1,
    }
}

/// Nearest-neighbour enlargement of a BGRA image.
fn upscale(width: u32, height: u32, bgra: &[u8], factor: u32) -> (u32, u32, Vec<u8>) {
    if factor <= 1 {
        return (width, height, bgra.to_vec());
    }
    let (w, h) = (width * factor, height * factor);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let row = ((y / factor) * width) as usize * 4;
        for x in 0..w {
            let i = row + (x / factor) as usize * 4;
            out.extend_from_slice(&bgra[i..i + 4]);
        }
    }
    (w, h, out)
}

/// Lines with trailing spaces trimmed and blank lines at the ends dropped.
fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

#[cfg(windows)]
mod win {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::System::UserProfile::GlobalizationPreferences;
    use windows::Media::Ocr::OcrEngine;
    use windows::Security::Cryptography::CryptographicBuffer;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

    use super::OcrError;

    pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, OcrError> {
        let fail = |e: windows::core::Error| OcrError::Failed(format!("{e}"));
        // SAFETY: initialises WinRT for this (background) thread; a thread
        // already initialised just gets an "already" result.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let engines = engines();
        if engines.is_empty() {
            return Err(OcrError::NoLanguage);
        }
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        let factor = super::upscale_factor(height).min((max / width.max(1)).max(1)).min((max / height.max(1)).max(1));
        let (w, h, pixels) = super::upscale(width, height, bgra, factor);
        let buffer = CryptographicBuffer::CreateFromByteArray(&pixels).map_err(fail)?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32).map_err(fail)?;
        // One engine reads one language; the cleanest reading wins (ties go
        // to the user's first language).
        let mut best: Option<(i32, String)> = None;
        for engine in engines {
            let text = read(&engine, &bitmap).map_err(fail)?;
            let score = super::text_score(&text);
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, text));
            }
        }
        Ok(best.map(|(_, t)| t).unwrap_or_default())
    }

    /// An engine for each OCR language among the user's languages, in their
    /// order; the default engine when none of them has one.
    fn engines() -> Vec<OcrEngine> {
        let mut out = Vec::new();
        if let Ok(tags) = GlobalizationPreferences::Languages() {
            for tag in tags {
                let Ok(language) = Language::CreateLanguage(&tag) else { continue };
                if OcrEngine::IsLanguageSupported(&language).unwrap_or(false)
                    && let Ok(engine) = OcrEngine::TryCreateFromLanguage(&language)
                {
                    out.push(engine);
                }
            }
        }
        if out.is_empty() {
            out.extend(OcrEngine::TryCreateFromUserProfileLanguages().ok());
        }
        out
    }

    fn read(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> windows::core::Result<String> {
        let result = engine.RecognizeAsync(bitmap)?.join()?;
        let mut text = Vec::new();
        for line in result.Lines()? {
            text.push(line.Text()?.to_string());
        }
        Ok(super::tidy(&text.join("\n")))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use super::OcrError;

    pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, OcrError> {
        let fail = |e: String| OcrError::Failed(e);
        let langs = match Command::new("tesseract").arg("--list-langs").output() {
            Ok(out) => super::pick_languages(&String::from_utf8_lossy(&out.stdout)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(OcrError::MissingTesseract),
            Err(e) => return Err(fail(format!("running tesseract: {e}"))),
        };
        let (w, h, pixels) = super::upscale(width, height, bgra, super::upscale_factor(height));
        let rgba: Vec<u8> = pixels.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        let image = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| fail("image size".into()))?;
        let path = std::env::temp_dir().join(format!("yaltri-ocr-{}.png", std::process::id()));
        image.save_with_format(&path, image::ImageFormat::Png).map_err(|e| fail(format!("writing {}: {e}", path.display())))?;
        let out = Command::new("tesseract").arg(&path).arg("stdout").args(["-l", &langs]).output();
        let _ = std::fs::remove_file(&path);
        let out = out.map_err(|e| fail(format!("running tesseract: {e}")))?;
        if !out.status.success() {
            return Err(fail(format!("tesseract: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(super::tidy(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// How clean a reading looks: letters and digits count, stray symbols that
/// misread letters turn into (`$`, `|`, `§`…) count against.
#[cfg_attr(not(windows), allow(dead_code))]
fn text_score(text: &str) -> i32 {
    text.chars()
        .map(|c| match c {
            c if c.is_alphanumeric() => 1,
            '$' | '|' | '§' | '¢' | '£' | '¥' | '©' | '®' | '°' | '¦' | '~' | '^' | '`' | '\\' => -3,
            _ => 0,
        })
        .sum()
}

/// `tur+eng` from what `tesseract --list-langs` prints, keeping what is
/// installed; English alone when neither is.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pick_languages(listing: &str) -> String {
    let installed: Vec<&str> = listing.lines().skip(1).map(str::trim).collect();
    let picked: Vec<&str> = ["tur", "eng"].into_iter().filter(|l| installed.contains(l)).collect();
    if picked.is_empty() { "eng".into() } else { picked.join("+") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_turkish_and_english_when_installed() {
        let listing = "List of available languages in \"/usr/share/tesseract-ocr/5/tessdata/\" (3):\neng\nosd\ntur\n";
        assert_eq!(pick_languages(listing), "tur+eng");
        assert_eq!(pick_languages("List (2):\nosd\ndeu\n"), "eng");
    }

    #[test]
    fn enlarges_small_images() {
        assert_eq!(upscale_factor(20), 3);
        assert_eq!(upscale_factor(100), 2);
        assert_eq!(upscale_factor(400), 1);
        let (w, h, px) = upscale(2, 1, &[1, 1, 1, 1, 2, 2, 2, 2], 2);
        assert_eq!((w, h), (4, 2));
        let row = [[1u8; 4], [1; 4], [2; 4], [2; 4]].concat();
        assert_eq!(px, [row.clone(), row].concat());
    }

    #[test]
    fn prefers_the_reading_without_stray_symbols() {
        // What an English engine made of Turkish text vs the Turkish engine.
        assert!(text_score("İkinci satır: çğşıöü") > text_score("ikinci satir: q$löü"));
        assert_eq!(text_score("ab 12"), 4);
    }

    #[test]
    fn tidies_lines() {
        // Tesseract ends its output with a form feed.
        assert_eq!(tidy("\n\n  hello  \nworld \n\n\u{c}"), "  hello\nworld");
        assert_eq!(tidy("a\n\nb\n"), "a\n\nb");
    }
}
