//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod backend;
pub mod cli;
pub mod config;
pub mod error;
pub mod pipeline;
pub mod prompt;

pub use error::GatewayError;
