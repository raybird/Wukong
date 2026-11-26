//! wukong-orchestrator：五個專家角色。
//!
//! 2026-11-26（#45）：規劃（planner）與接力鏈移除，派工改由 OpenCode V2 的 primary agent 以
//! subagent 處理（`wukong_runtime::agent_assets`）。這裡只留角色的定義（寫進 agent 定義、入口
//! 顯示用）；技能清單的角色對應同日隨規劃偏好一併移除（Task 5.2）。

pub mod role;

pub use role::Role;
