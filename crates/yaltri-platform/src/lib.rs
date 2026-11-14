//! OS integration that GPUI does not provide: system tray, global hotkeys,
//! single-instance IPC and session detection. Every event source funnels into
//! one `Command` channel that the app drains on its main thread.

pub mod hotkeys;
mod icon;
pub mod ipc;
pub mod keys;
pub mod launch;
pub mod screen;
pub mod session;
pub mod tray;

use futures::channel::mpsc::UnboundedSender;
use yaltri_core::Command;

/// Cloneable, thread-safe handle every event source uses to reach the app.
pub type CommandSender = UnboundedSender<Command>;
