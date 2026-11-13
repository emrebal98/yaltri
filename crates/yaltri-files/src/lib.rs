//! Everything-style file search without Everything: a compact in-memory
//! index of names, filled by a parallel walk, persisted between runs and kept
//! current with filesystem change notifications.

pub mod index;
pub mod indexer;
pub mod persist;
pub mod provider;
pub mod scan;
pub mod scope;

pub use indexer::{FileIndexer, Phase, Status};
pub use provider::FilesProvider;
pub use scope::Scope;
