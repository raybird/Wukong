//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod backend;
pub mod cli;
pub mod config;
pub mod error;
pub mod opencode_server;
pub mod pipeline;
pub mod prompt;
pub mod stream;
pub mod summarize;
pub mod workspace;

pub use error::GatewayError;
pub use stream::StreamEvent;
pub use workspace::workspace_dir;
