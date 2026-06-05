//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod scope;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
