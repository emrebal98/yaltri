//! UI-agnostic core of Yaltri: settings, tool ids, commands and the search
//! provider contract. Nothing in here may depend on GPUI or on a platform API.

pub mod command;
pub mod paths;
pub mod search;
pub mod settings;
pub mod tool;

pub use command::Command;
pub use search::{ItemAction, SearchItem, SearchProvider};
pub use settings::Settings;
pub use tool::ToolId;
