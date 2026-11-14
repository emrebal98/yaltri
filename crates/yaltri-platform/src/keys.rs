//! Turning a recorded key press into a shortcut spec (`Ctrl+Alt+Space`).
//! Key names follow GPUI's keystroke names; the produced spec is what the
//! settings store and `hotkeys::parse` accepts.

/// Why a recorded key press cannot be a global shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// Plain keys would fire while typing; F-keys are the only exception.
    NeedsModifier,
    /// The key has no layout-independent code (e.g. `ş` on a Turkish layout).
    UnsupportedKey,
}

/// Name shown for the platform ("Windows"/"Super") key.
pub const SUPER_NAME: &str = if cfg!(windows) { "Win" } else { "Super" };

/// Maps a GPUI key name to the spec's key name.
fn key_name(key: &str) -> Option<String> {
    let named = match key {
        "space" => "Space",
        "enter" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "escape" => "Escape",
        "printscreen" => "PrintScreen",
        "pause" => "Pause",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase().to_string()),
        (Some(c), None) if "`\\[],=-.';/".contains(c) => Some(c.to_string()),
        _ => None,
    }
}

/// Whether GPUI reports this "key" for a modifier pressed on its own.
pub fn is_modifier_key(key: &str) -> bool {
    matches!(key, "shift" | "control" | "ctrl" | "alt" | "platform" | "super" | "cmd" | "win" | "function" | "fn")
}

/// Builds `Win+Ctrl+Alt+Shift+Key` from a key press (the order Windows
/// and PowerToys display).
pub fn spec_from_press(ctrl: bool, alt: bool, shift: bool, sup: bool, key: &str) -> Result<String, RecordError> {
    let name = key_name(key).ok_or(RecordError::UnsupportedKey)?;
    let is_fkey = name.starts_with('F') && name.len() > 1 && name[1..].chars().all(|c| c.is_ascii_digit());
    if !(ctrl || alt || shift || sup) && !is_fkey {
        return Err(RecordError::NeedsModifier);
    }
    // Shift alone with a printable key is just typing a symbol.
    if shift && !(ctrl || alt || sup) && !is_fkey && name.chars().count() == 1 {
        return Err(RecordError::NeedsModifier);
    }
    let mut parts: Vec<&str> = Vec::new();
    if sup {
        parts.push(SUPER_NAME);
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    parts.push(&name);
    Ok(parts.join("+"))
}

/// Splits a stored spec into the names shown as key caps.
pub fn display_parts(spec: &str) -> Vec<String> {
    spec.split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "super" | "win" | "meta" | "cmd" => SUPER_NAME.to_owned(),
            "control" | "ctrl" => "Ctrl".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            other if other.len() == 1 => other.to_ascii_uppercase(),
            _ => p.to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_specs_in_canonical_order() {
        assert_eq!(spec_from_press(true, true, false, false, "space").unwrap(), "Ctrl+Alt+Space");
        assert_eq!(spec_from_press(false, false, true, true, "c").unwrap(), format!("{SUPER_NAME}+Shift+C"));
        assert_eq!(spec_from_press(false, false, false, false, "f5").unwrap(), "F5");
        assert_eq!(spec_from_press(true, false, false, false, "pageup").unwrap(), "Ctrl+PageUp");
    }

    #[test]
    fn rejects_plain_and_shift_only_typing() {
        assert_eq!(spec_from_press(false, false, false, false, "a"), Err(RecordError::NeedsModifier));
        assert_eq!(spec_from_press(false, false, true, false, "a"), Err(RecordError::NeedsModifier));
        assert!(spec_from_press(false, false, true, false, "f2").is_ok());
        assert!(spec_from_press(false, false, true, false, "space").is_ok());
    }

    #[test]
    fn rejects_layout_specific_keys() {
        assert_eq!(spec_from_press(true, false, false, false, "ş"), Err(RecordError::UnsupportedKey));
        assert_eq!(spec_from_press(true, false, false, false, "f30"), Err(RecordError::UnsupportedKey));
    }

    #[test]
    fn display_parts_are_readable() {
        assert_eq!(display_parts("ctrl+alt+space"), ["Ctrl", "Alt", "space"]);
        assert_eq!(display_parts("Super+Shift+c"), [SUPER_NAME, "Shift", "C"]);
        assert!(display_parts("").is_empty());
    }

    /// Every spec the recorder can produce must be registrable.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn every_recordable_key_parses_as_a_hotkey() {
        let mut keys: Vec<String> = ["space", "enter", "tab", "backspace", "delete", "insert", "home", "end", "pageup",
            "pagedown", "up", "down", "left", "right", "escape", "printscreen", "pause"]
            .map(String::from)
            .to_vec();
        keys.extend(('a'..='z').chain('0'..='9').map(String::from));
        keys.extend("`\\[],=-.';/".chars().map(String::from));
        keys.extend((1..=24).map(|n| format!("f{n}")));
        for key in keys {
            let spec = spec_from_press(true, true, false, false, &key).unwrap();
            assert!(crate::hotkeys::parse(&spec).is_ok(), "{key} -> {spec}");
        }
    }
}
