//! Wayland global shortcuts through the XDG desktop portal
//! (`org.freedesktop.portal.GlobalShortcuts`): KDE Plasma, Hyprland, GNOME
//! 48+ and others whose portal implements it. Wayland does not let an app
//! grab keys itself; instead the app names its shortcuts with a preferred
//! key combination, the desktop asks the user to approve (and may let them
//! change the keys), and then tells the app when one is pressed.
//!
//! A portal session lives as long as the app; changing a shortcut in
//! Settings closes it and binds again. The desktop keeps the user's own
//! assignment once approved, so the keys may differ from Settings; the ones
//! in effect are logged.
//!
//! Desktops without this portal (Sway and other wlroots compositors) leave
//! shortcuts to the user's own config: `yaltri toggle <tool>`.

use std::collections::HashMap;

use ashpd::AppID;
use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use futures::channel::mpsc;
use futures::{FutureExt as _, StreamExt as _};
use yaltri_core::Command;

use crate::CommandSender;

/// One shortcut for the portal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalBinding {
    /// Stable id (`launcher`); the desktop remembers the user's choice by it.
    pub id: String,
    /// Shown by the desktop ("Yaltri: Search").
    pub description: String,
    /// Our `Super+Shift+F` form; sent as the preferred trigger.
    pub spec: String,
    /// What a press runs.
    pub command: Command,
}

/// Handle to the portal thread.
pub struct PortalKeys {
    bindings: mpsc::UnboundedSender<Vec<PortalBinding>>,
}

impl PortalKeys {
    /// Starts talking to the portal on its own thread. Presses arrive on
    /// `commands`. If the portal is missing, this only logs.
    pub fn start(commands: CommandSender) -> Self {
        let (tx, rx) = mpsc::unbounded();
        let spawned = std::thread::Builder::new()
            .name("yaltri-portal-keys".into())
            .spawn(move || futures::executor::block_on(run(commands, rx)));
        if let Err(e) = spawned {
            tracing::error!("starting the global shortcuts thread failed: {e}");
        }
        Self { bindings: tx }
    }

    /// Makes `bindings` the app's shortcuts (an empty list removes them).
    pub fn apply(&self, bindings: Vec<PortalBinding>) {
        let _ = self.bindings.unbounded_send(bindings);
    }
}

async fn run(commands: CommandSender, mut requests: mpsc::UnboundedReceiver<Vec<PortalBinding>>) {
    // A host (non-sandboxed) app tells the portal who it is first, so the
    // desktop can show its name and icon and remember the user's choice.
    // Older portals lack the registry; binding may still work without it.
    match AppID::try_from(crate::APP_ID) {
        Ok(id) => {
            if let Err(e) = ashpd::register_host_app(id).await {
                tracing::info!("portal app registration unavailable: {e}");
            }
        }
        Err(e) => tracing::warn!("invalid app id {}: {e}", crate::APP_ID),
    }
    let portal = match GlobalShortcuts::new().await {
        Ok(p) => p,
        Err(e) => {
            tracing::info!("no global shortcuts portal ({e}); bind `yaltri toggle <tool>` in the desktop settings");
            return;
        }
    };
    let mut activated = match portal.receive_activated().await {
        Ok(stream) => stream.fuse(),
        Err(e) => {
            tracing::warn!("listening for global shortcuts failed: {e}");
            return;
        }
    };

    let mut session = None;
    let mut commands_by_id: HashMap<String, Command> = HashMap::new();
    loop {
        futures::select! {
            request = requests.next() => {
                let Some(mut bindings) = request else { break };
                // Only the latest request matters.
                while let Some(Some(newer)) = requests.next().now_or_never() {
                    bindings = newer;
                }
                if let Some(old) = session.take() {
                    let _ = ashpd::desktop::Session::close(&old).await;
                }
                commands_by_id = bindings.iter().map(|b| (b.id.clone(), b.command)).collect();
                if bindings.is_empty() {
                    continue;
                }
                match bind(&portal, &bindings).await {
                    Ok(s) => session = Some(s),
                    Err(e) => tracing::warn!("binding global shortcuts failed: {e}"),
                }
            }
            event = activated.next() => {
                let Some(event) = event else { break };
                if let Some(command) = commands_by_id.get(event.shortcut_id()) {
                    tracing::info!(id = event.shortcut_id(), "global shortcut");
                    let _ = commands.unbounded_send(*command);
                }
            }
        }
    }
    if let Some(old) = session {
        let _ = ashpd::desktop::Session::close(&old).await;
    }
}

async fn bind(
    portal: &GlobalShortcuts,
    bindings: &[PortalBinding],
) -> ashpd::Result<ashpd::desktop::Session<GlobalShortcuts>> {
    let session = portal.create_session(CreateSessionOptions::default()).await?;
    let triggers: Vec<Option<String>> = bindings.iter().map(|b| trigger_from_spec(&b.spec)).collect();
    let shortcuts: Vec<NewShortcut> = bindings
        .iter()
        .zip(&triggers)
        .map(|(b, trigger)| NewShortcut::new(b.id.clone(), b.description.clone()).preferred_trigger(trigger.as_deref()))
        .collect();
    let response = portal.bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default()).await?.response()?;
    for shortcut in response.shortcuts() {
        tracing::info!(id = shortcut.id(), keys = shortcut.trigger_description(), "global shortcut bound by the desktop");
    }
    Ok(session)
}

/// `Super+Shift+F` -> `LOGO+SHIFT+f`, the freedesktop shortcuts format the
/// portal takes as a preferred trigger (modifiers, then an xkb key name).
pub fn trigger_from_spec(spec: &str) -> Option<String> {
    let accel = crate::gnome_keys::accel_from_spec(spec)?;
    let mut rest = accel.as_str();
    let mut parts = Vec::new();
    while let Some(after) = rest.strip_prefix('<') {
        let (modifier, tail) = after.split_once('>')?;
        parts.push(match modifier {
            "Control" => "CTRL",
            "Alt" => "ALT",
            "Shift" => "SHIFT",
            "Super" => "LOGO",
            _ => return None,
        });
        rest = tail;
    }
    if rest.is_empty() {
        return None;
    }
    let mut trigger = parts.join("+");
    if !trigger.is_empty() {
        trigger.push('+');
    }
    trigger.push_str(rest);
    Some(trigger)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_specs_to_portal_triggers() {
        assert_eq!(trigger_from_spec("Super+Shift+F").as_deref(), Some("LOGO+SHIFT+f"));
        assert_eq!(trigger_from_spec("Ctrl+Alt+Space").as_deref(), Some("CTRL+ALT+space"));
        assert_eq!(trigger_from_spec("Win+PageUp").as_deref(), Some("LOGO+Page_Up"));
        assert_eq!(trigger_from_spec("F12").as_deref(), Some("F12"));
        assert_eq!(trigger_from_spec("Super+Shift+/").as_deref(), Some("LOGO+SHIFT+slash"));
        assert_eq!(trigger_from_spec(""), None);
        assert_eq!(trigger_from_spec("Super+Nonsense"), None);
    }
}
