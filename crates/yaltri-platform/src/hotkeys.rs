//! Global shortcuts on Windows and X11. Wayland does not let clients grab
//! keys; there the user binds `yaltri toggle <tool>` in their desktop settings.
//!
//! On Windows the manager must be created on the thread that runs the message
//! loop (GPUI's main thread).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use yaltri_core::settings::normalize_hotkey;
use yaltri_core::{Command, ToolId};

use crate::CommandSender;

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    by_id: Arc<Mutex<HashMap<u32, ToolId>>>,
}

/// Why a tool's shortcut is not active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyError {
    pub tool: ToolId,
    pub message: String,
}

impl Hotkeys {
    pub fn new(tx: CommandSender) -> Result<Self> {
        let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("global hotkeys unavailable: {e}"))?;
        let by_id: Arc<Mutex<HashMap<u32, ToolId>>> = Arc::default();
        let lookup = by_id.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            let tool = lookup.lock().ok().and_then(|m| m.get(&event.id).copied());
            if let Some(tool) = tool {
                let _ = tx.unbounded_send(Command::Toggle(tool));
            }
        }));
        Ok(Self { manager, registered: Vec::new(), by_id })
    }

    /// Replaces all registrations with `bindings`. Returns the bindings that
    /// failed (bad syntax, or the combination is held by another program).
    pub fn apply(&mut self, bindings: &[(ToolId, String)]) -> Vec<HotkeyError> {
        for hk in self.registered.drain(..) {
            if let Err(e) = self.manager.unregister(hk) {
                tracing::warn!(%e, "unregistering hotkey failed");
            }
        }
        let mut map = HashMap::new();
        let mut errors = Vec::new();
        for (tool, spec) in bindings {
            if spec.trim().is_empty() {
                continue;
            }
            let result = parse(spec).and_then(|hk| {
                self.manager.register(hk).map(|()| hk).map_err(|e| {
                    tracing::debug!(%e, "register failed");
                    anyhow!("{spec} başka bir uygulama tarafından kullanılıyor (ör. PowerToys Run). Farklı bir kısayol seçin.")
                })
            });
            match result {
                Ok(hk) => {
                    tracing::info!(%tool, spec, "hotkey registered");
                    map.insert(hk.id(), *tool);
                    self.registered.push(hk);
                }
                Err(e) => {
                    tracing::warn!(%tool, spec, %e, "hotkey failed");
                    errors.push(HotkeyError { tool: *tool, message: e.to_string() });
                }
            }
        }
        if let Ok(mut by_id) = self.by_id.lock() {
            *by_id = map;
        }
        errors
    }
}

/// Accepts user-friendly input (`Win+Shift+C`, `space + alt`) and converts it
/// to global-hotkey's strict `MODS+KEY` syntax.
pub fn parse(spec: &str) -> Result<HotKey> {
    let normalized = normalize_hotkey(spec);
    normalized
        .parse::<HotKey>()
        .map_err(|e| anyhow!("geçersiz kısayol '{spec}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_friendly_specs() {
        assert!(parse("Alt+Space").is_ok());
        assert!(parse("Win+Shift+C").is_ok());
        assert!(parse("space + alt").is_ok());
        assert_eq!(parse("Alt+Space").unwrap(), parse("space+ALT").unwrap());
        assert!(parse("Alt+Nope").is_err());
    }
}
