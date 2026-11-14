//! UI-agnostic core of Yaltri: settings, tool ids, commands and the search
//! provider contract. Nothing in here may depend on GPUI or on a platform API.

pub mod command;
pub mod history;
pub mod paths;
pub mod search;
pub mod settings;
pub mod tool;

pub use command::Command;
pub use history::History;
pub use search::{ItemAction, ItemKind, Scope, SearchItem, SearchProvider};
pub use settings::Settings;
pub use tool::ToolId;
