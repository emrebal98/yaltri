//! OS integration that GPUI does not provide: system tray, global hotkeys,
//! single-instance IPC and session detection. Every event source funnels into
//! one `Command` channel that the app drains on its main thread.

#[cfg(windows)]
pub mod hookkeys;
#[cfg(windows)]
pub mod powertoys;
pub mod autostart;
#[cfg(target_os = "linux")]
pub mod desktop_font;
#[cfg(target_os = "linux")]
pub mod gnome_keys;
pub mod hotkeys;
pub mod icon;
pub mod ipc;
#[cfg(any(windows, target_os = "linux"))]
pub mod keygrab;
pub mod keys;
pub mod launch;
pub mod screen;
pub mod session;
pub mod tray;
#[cfg(target_os = "linux")]
pub mod x11;

use futures::channel::mpsc::UnboundedSender;
use yaltri_core::Command;

/// Cloneable, thread-safe handle every event source uses to reach the app.
pub type CommandSender = UnboundedSender<Command>;
