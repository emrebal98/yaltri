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
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Security::Cryptography::CryptographicBuffer;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

    use super::OcrError;

    pub fn recognize(width: u32, height: u32, bgra: &[u8]) -> Result<String, OcrError> {
        let fail = |e: windows::core::Error| OcrError::Failed(format!("{e}"));
        // SAFETY: initialises WinRT for this (background) thread; a thread
        // already initialised just gets an "already" result.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| OcrError::NoLanguage)?;
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        let factor = super::upscale_factor(height).min((max / width.max(1)).max(1)).min((max / height.max(1)).max(1));
        let (w, h, pixels) = super::upscale(width, height, bgra, factor);
        let buffer = CryptographicBuffer::CreateFromByteArray(&pixels).map_err(fail)?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32).map_err(fail)?;
        let result = engine.RecognizeAsync(&bitmap).map_err(fail)?.join().map_err(fail)?;
        let lines = result.Lines().map_err(fail)?;
        let mut text = Vec::new();
        for i in 0..lines.Size().map_err(fail)? {
            text.push(lines.GetAt(i).and_then(|l| l.Text()).map_err(fail)?.to_string());
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
    fn tidies_lines() {
        // Tesseract ends its output with a form feed.
        assert_eq!(tidy("\n\n  hello  \nworld \n\n\u{c}"), "  hello\nworld");
        assert_eq!(tidy("a\n\nb\n"), "a\n\nb");
    }
}
