//! Global shortcuts on Windows and X11. Wayland does not let clients grab
//! keys; there the user binds `yaltri toggle <tool>` in their desktop settings.
//!
//! On Windows the manager must be created on the thread that runs the message
//! loop (GPUI's main thread). Combinations Windows refuses to register
//! (reserved by the shell or owned by another program) fall back to a
//! low-level keyboard hook, see `hookkeys`.

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
    #[cfg(windows)]
    tx: CommandSender,
    /// Started on first use; serves the shortcuts registration refused.
    #[cfg(windows)]
    hook: Option<crate::hookkeys::HookThread>,
}

/// Why a tool's shortcut is not active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyError {
    pub tool: ToolId,
    pub kind: HotkeyErrorKind,
}

/// Why a shortcut does not work; the UI turns this into a localized message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyErrorKind {
    /// The spec does not parse (detail is the parser's English message).
    Invalid(String),
    /// Another program or the OS holds the combination.
    InUse,
    /// PowerToys uses the same combination in this module; it may win.
    SharedWithPowerToys(String),
}

impl Hotkeys {
    pub fn new(tx: CommandSender) -> Result<Self> {
        let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("global hotkeys unavailable: {e}"))?;
        let by_id: Arc<Mutex<HashMap<u32, ToolId>>> = Arc::default();
        let lookup = by_id.clone();
        #[cfg(windows)]
        let hook_tx = tx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            let tool = lookup.lock().ok().and_then(|m| m.get(&event.id).copied());
            if let Some(tool) = tool {
                let _ = tx.unbounded_send(Command::Toggle(tool));
            }
        }));
        Ok(Self {
            manager,
            registered: Vec::new(),
            by_id,
            #[cfg(windows)]
            tx: hook_tx,
            #[cfg(windows)]
            hook: None,
        })
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
        #[cfg(windows)]
        let mut hooked: Vec<(crate::hookkeys::Combo, ToolId)> = Vec::new();
        for (tool, spec) in bindings {
            if spec.trim().is_empty() {
                continue;
            }
            let hk = match parse(spec) {
                Ok(hk) => hk,
                Err(e) => {
                    errors.push(HotkeyError { tool: *tool, kind: HotkeyErrorKind::Invalid(e.to_string()) });
                    continue;
                }
            };
            #[cfg(windows)]
            if let Some(module) = crate::hookkeys::combo_from_spec(spec).and_then(|c| crate::powertoys::owner_of(&c)) {
                // Still registered below: it works as soon as PowerToys lets go.
                errors.push(HotkeyError { tool: *tool, kind: HotkeyErrorKind::SharedWithPowerToys(module) });
            }
            match self.manager.register(hk) {
                Ok(()) => {
                    tracing::debug!(%tool, spec, "hotkey registered");
                    map.insert(hk.id(), *tool);
                    self.registered.push(hk);
                }
                Err(e) => {
                    tracing::debug!(%tool, spec, %e, "RegisterHotKey refused");
                    #[cfg(windows)]
                    if let Some(combo) = crate::hookkeys::combo_from_spec(spec) {
                        tracing::debug!(%tool, spec, "shortcut is reserved elsewhere; using keyboard hook");
                        hooked.push((combo, *tool));
                        continue;
                    }
                    errors.push(HotkeyError { tool: *tool, kind: HotkeyErrorKind::InUse });
                }
            }
        }
        if let Ok(mut by_id) = self.by_id.lock() {
            *by_id = map;
        }
        #[cfg(windows)]
        self.set_hooked(hooked, &mut errors);
        errors
    }

    #[cfg(windows)]
    fn set_hooked(&mut self, hooked: Vec<(crate::hookkeys::Combo, ToolId)>, errors: &mut Vec<HotkeyError>) {
        if hooked.is_empty() {
            if let Some(hook) = &self.hook {
                hook.set_bindings(Vec::new());
            }
            return;
        }
        if self.hook.is_none() {
            match crate::hookkeys::HookThread::start(self.tx.clone()) {
                Ok(h) => self.hook = Some(h),
                Err(e) => {
                    tracing::warn!("keyboard hook unavailable: {e:#}");
                    errors.extend(
                        hooked.iter().map(|(_, tool)| HotkeyError { tool: *tool, kind: HotkeyErrorKind::InUse }),
                    );
                    return;
                }
            }
        }
        if let Some(hook) = &self.hook {
            hook.set_bindings(hooked);
        }
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
