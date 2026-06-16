//! wukong-telegram: Telegram bot entry point over the Wukong turn engine.
//!
//! The Telegram transport primitives (`client`, `parse`, `error`) live in the
//! dependency-free `wukong-tg-client` crate so the scheduler daemon can reuse
//! them; they are re-exported here so existing `crate::client` / `crate::parse`
//! / `crate::error` paths keep working.

pub use wukong_tg_client::error::TgError;
pub use wukong_tg_client::{client, error, parse};

pub mod command;
pub mod dispatch;
