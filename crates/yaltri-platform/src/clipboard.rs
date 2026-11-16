//! Images on the clipboard.
//!
//! GPUI writes images only as a registered "PNG" format on Windows (Paint
//! and many others want `CF_DIB`) and not at all on X11. `arboard` writes
//! CF_DIBV5 + PNG on Windows and serves X11 / Wayland selections itself.
//! Keep one [`ImageClipboard`] alive for the app's lifetime: on X11 the
//! image is served from it until another app takes the clipboard.

use anyhow::{Context, Result};

#[derive(Default)]
pub struct ImageClipboard {
    inner: Option<arboard::Clipboard>,
}

impl ImageClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a BGRA image on the clipboard.
    pub fn set_bgra(&mut self, width: u32, height: u32, bgra: &[u8]) -> Result<()> {
        let clipboard = match &mut self.inner {
            Some(c) => c,
            slot => slot.insert(arboard::Clipboard::new().context("opening the clipboard")?),
        };
        let image = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: bgra_to_rgba(bgra).into(),
        };
        clipboard.set_image(image).context("copying the image")
    }
}

/// Swaps blue and red and makes every pixel opaque (screen captures leave
/// alpha undefined).
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_channels_and_sets_alpha() {
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 0, 10, 20, 30, 7]), vec![3, 2, 1, 255, 30, 20, 10, 255]);
    }
}
