//! OS integration that GPUI does not provide: system tray, global hotkeys,
//! single-instance IPC and session detection. Every event source funnels into
//! one `Command` channel that the app drains on its main thread.

#[cfg(windows)]
pub mod hookkeys;
#[cfg(windows)]
pub mod powertoys;
pub mod autostart;
pub mod hotkeys;
pub mod icon;
pub mod ipc;
#[cfg(windows)]
pub mod keygrab;
pub mod keys;
pub mod launch;
pub mod screen;
pub mod session;
pub mod tray;

use futures::channel::mpsc::UnboundedSender;
use yaltri_core::Command;

/// Cloneable, thread-safe handle every event source uses to reach the app.
pub type CommandSender = UnboundedSender<Command>;
