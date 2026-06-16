//! Shared Telegram transport primitives (Bot API client + pure parsing/policy
//! helpers) reused by the Telegram bot (`wukong-telegram`) and the scheduler
//! daemon (`wukong-schedulerd`). Deliberately has NO Wukong-internal
//! dependencies so any crate can deliver messages without pulling in the CLI.

pub mod client;
pub mod error;
pub mod parse;

pub use error::TgError;
