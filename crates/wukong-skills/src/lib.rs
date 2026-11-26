//! wukong-skills：內嵌的 Superpowers 技能，由 runtime 寫到工作區的 `.opencode/skills/`。

pub mod catalog;

pub use catalog::{all, source_content, SkillSpec};
