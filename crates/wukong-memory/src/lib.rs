//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod scope;

pub use error::{MemoryError, Result};
pub use scope::Scope;
