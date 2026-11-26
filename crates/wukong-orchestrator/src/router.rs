use crate::role::Role;

/// 技能與它的主要、協作角色。原本給 planner 選技能用；2026-11-26（#45）planner 移除後，
/// 只剩 Web 的偏好 API 與技能清單還在用，Task 5.2 一併移除。
#[derive(Debug, Clone, Copy)]
pub struct SkillRouteOption {
    pub skill_name: &'static str,
    pub description: &'static str,
    pub primary_role: Role,
    pub collaborator_role: Option<Role>,
}
