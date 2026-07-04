use crate::error::OrchestratorError;
use crate::role::Role;
use wukong_gateway::backend::{AgentRequest, AiBackend};

/// Build the routing prompt: list the roles and ask for exactly one name.
pub fn routing_prompt(task: &str) -> String {
    let mut s =
        String::from("You are a router. Pick the single best role to handle the task.\nRoles:\n");
    for role in Role::all() {
        s.push_str(&format!("- {}: {}\n", role.name(), role.description()));
    }
    s.push_str("\nReply with ONLY the role name (one lowercase word).\n\n[Task]\n");
    s.push_str(task);
    s
}

/// Parse the routed role from the backend's reply. Scans in `Role::all()`
/// order and returns the first role whose name appears (case-insensitive);
/// falls back to `Role::Oracle` when none match.
pub fn parse_role(response: &str) -> Role {
    let lower = response.to_lowercase();
    for role in Role::all() {
        if lower.contains(role.name()) {
            return role;
        }
    }
    Role::Oracle
}

/// Build the planning prompt: list roles and ask for an ordered, comma-
/// separated chain (one role for simple tasks, at most three).
pub fn planning_prompt(task: &str) -> String {
    let mut s = String::from(
        "You are a planner. Decide which roles should collaborate on the task, \
         in execution order.\nRoles:\n",
    );
    for role in Role::all() {
        s.push_str(&format!("- {}: {}\n", role.name(), role.description()));
    }
    s.push_str(
        "\nReply with ONLY a comma-separated list of role names in execution order \
         (lowercase). Use a single role for simple tasks; at most three. No explanation.\n\n[Task]\n",
    );
    s.push_str(task);
    s
}

/// Parse an ordered role chain from the reply. Each role is matched by the
/// earliest position its name appears (case-insensitive); roles are ordered by
/// that position, deduped, and capped at three. Empty match falls back to a
/// single Oracle.
pub fn parse_chain(response: &str) -> Vec<Role> {
    let lower = response.to_lowercase();
    let mut found: Vec<(usize, Role)> = Role::all()
        .into_iter()
        .filter_map(|role| lower.find(role.name()).map(|pos| (pos, role)))
        .collect();
    found.sort_by_key(|(pos, _)| *pos);
    let chain: Vec<Role> = found.into_iter().map(|(_, r)| r).take(3).collect();
    if chain.is_empty() {
        vec![Role::Oracle]
    } else {
        chain
    }
}

// ---------------------------------------------------------------------------
// Skill-aware planning types and helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct SkillRouteOption {
    pub skill_name: &'static str,
    pub description: &'static str,
    pub primary_role: Role,
    pub collaborator_role: Option<Role>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStep {
    pub role: Role,
    pub skill_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPreferenceHint {
    pub preferred_roles: Vec<Role>,
    pub preferred_skills: Vec<String>,
}

const KNOWN_SKILLS: &[&str] = &[
    "brainstorming",
    "writing-plans",
    "executing-plans",
    "test-driven-development",
    "systematic-debugging",
    "verification-before-completion",
    "requesting-code-review",
    "receiving-code-review",
];

fn parse_role_name(name: &str) -> Option<Role> {
    let lower = name.trim().to_ascii_lowercase();
    Role::all().into_iter().find(|role| lower == role.name())
}

fn normalize_skill_name(name: &str) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "none" {
        return None;
    }
    KNOWN_SKILLS
        .iter()
        .find(|skill| **skill == lower)
        .map(|skill| (*skill).to_string())
}

pub fn parse_skill_chain(response: &str) -> Vec<PlannedStep> {
    let mut steps = Vec::new();

    for line in response.lines() {
        if steps.len() >= 3 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, '|');
        let role_part = parts.next().unwrap_or_default();
        let skill_part = parts.next().unwrap_or_default();
        if let Some(role) = parse_role_name(role_part) {
            steps.push(PlannedStep {
                role,
                skill_name: normalize_skill_name(skill_part),
            });
        }
    }

    if steps.is_empty() {
        let roles = parse_chain(response);
        return roles
            .into_iter()
            .take(3)
            .map(|role| PlannedStep {
                role,
                skill_name: None,
            })
            .collect();
    }

    steps
}

pub fn skill_planning_prompt(task: &str, skills: &[SkillRouteOption]) -> String {
    skill_planning_prompt_with_preferences(task, skills, None)
}

pub fn skill_planning_prompt_with_preferences(
    task: &str,
    skills: &[SkillRouteOption],
    preferences: Option<&PlannerPreferenceHint>,
) -> String {
    let mut s = String::from(
        "You are a planner. Decide which roles should collaborate on the task, \
         and which Superpowers skill each role should follow.\nRoles:\n",
    );
    for role in Role::all() {
        s.push_str(&format!("- {}: {}\n", role.name(), role.description()));
    }
    s.push_str("\nSkills:\n");
    for skill in skills {
        let collaborator = skill
            .collaborator_role
            .map(|role| role.name())
            .unwrap_or("none");
        s.push_str(&format!(
            "- {}: {} (primary: {}, collaborator: {})\n",
            skill.skill_name,
            skill.description,
            skill.primary_role.name(),
            collaborator
        ));
    }
    if let Some(preferences) = preferences {
        if !preferences.preferred_roles.is_empty() || !preferences.preferred_skills.is_empty() {
            s.push_str("\n[User Preferences]\n");
            if !preferences.preferred_roles.is_empty() {
                let roles = preferences
                    .preferred_roles
                    .iter()
                    .map(Role::name)
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("Preferred roles: {roles}\n"));
            }
            if !preferences.preferred_skills.is_empty() {
                s.push_str(&format!(
                    "Preferred skills: {}\n",
                    preferences.preferred_skills.join(", ")
                ));
            }
            s.push_str(
                "These are preferences, not hard constraints. Choose other roles or skills when the task requires it.\n",
            );
        }
    }
    s.push_str(
        "\nReply with ONLY one step per line in this exact format:\n\
         <role>|<skill-or-none>\n\
         Use lowercase role names and skill names. Use none when no listed skill fits. \
         Use a single step for simple tasks; at most three steps. No explanation.\n\n[Task]\n",
    );
    s.push_str(task);
    s
}

pub async fn plan_skill_chain(
    backend: &impl AiBackend,
    task: &str,
    skills: &[SkillRouteOption],
) -> Result<Vec<PlannedStep>, OrchestratorError> {
    plan_skill_chain_with_preferences(backend, task, skills, None).await
}

pub async fn plan_skill_chain_with_preferences(
    backend: &impl AiBackend,
    task: &str,
    skills: &[SkillRouteOption],
    preferences: Option<&PlannerPreferenceHint>,
) -> Result<Vec<PlannedStep>, OrchestratorError> {
    let tool_overrides = std::collections::BTreeMap::from([("question".to_string(), false)]);
    let resp = backend
        .run(AgentRequest {
            prompt: skill_planning_prompt_with_preferences(task, skills, preferences),
            session_id: None,
            thinking: false,
            model: None,
            agent: Some("plan".to_string()),
            tool_overrides,
            attachments: Vec::new(),
        })
        .await?;
    Ok(parse_skill_chain(&resp.text))
}

/// Phase 1: ask the backend which role should handle the task.
pub async fn route(backend: &impl AiBackend, task: &str) -> Result<Role, OrchestratorError> {
    let tool_overrides = std::collections::BTreeMap::from([("question".to_string(), false)]);
    let resp = backend
        .run(AgentRequest {
            prompt: routing_prompt(task),
            session_id: None,
            thinking: false,
            model: None,
            agent: Some("plan".to_string()),
            tool_overrides,
            attachments: Vec::new(),
        })
        .await?;
    Ok(parse_role(&resp.text))
}

/// Phase 1 (chain): ask the backend for an ordered role chain.
pub async fn plan_chain(
    backend: &impl AiBackend,
    task: &str,
) -> Result<Vec<Role>, OrchestratorError> {
    let tool_overrides = std::collections::BTreeMap::from([("question".to_string(), false)]);
    let resp = backend
        .run(AgentRequest {
            prompt: planning_prompt(task),
            session_id: None,
            thinking: false,
            model: None,
            agent: Some("plan".to_string()),
            tool_overrides,
            attachments: Vec::new(),
        })
        .await?;
    Ok(parse_chain(&resp.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_prompt_lists_roles_and_task() {
        let p = routing_prompt("refactor the parser");
        for role in Role::all() {
            assert!(p.contains(role.name()), "missing role {}", role.name());
        }
        assert!(p.contains("refactor the parser"));
    }

    #[test]
    fn parse_role_matches_exact_name() {
        assert_eq!(parse_role("fixer"), Role::Fixer);
    }

    #[test]
    fn parse_role_is_case_insensitive() {
        assert_eq!(parse_role("FIXER"), Role::Fixer);
    }

    #[test]
    fn parse_role_finds_name_in_sentence() {
        assert_eq!(parse_role("I'd pick oracle for this"), Role::Oracle);
    }

    #[test]
    fn parse_role_falls_back_to_oracle() {
        assert_eq!(parse_role("garbage with no role"), Role::Oracle);
    }

    #[test]
    fn planning_prompt_lists_roles_and_task() {
        let p = planning_prompt("build and document a parser");
        for role in Role::all() {
            assert!(p.contains(role.name()), "missing role {}", role.name());
        }
        assert!(p.contains("build and document a parser"));
    }

    #[test]
    fn parse_chain_reads_ordered_roles() {
        assert_eq!(
            parse_chain("explorer, fixer, librarian"),
            vec![Role::Explorer, Role::Fixer, Role::Librarian]
        );
    }

    #[test]
    fn parse_chain_orders_by_appearance() {
        // Order follows position in the text, not Role::all() order.
        assert_eq!(
            parse_chain("先 fixer 再 explorer"),
            vec![Role::Fixer, Role::Explorer]
        );
    }

    #[test]
    fn parse_chain_caps_at_three() {
        let c = parse_chain("explorer oracle librarian fixer designer");
        assert_eq!(c.len(), 3);
        assert_eq!(c, vec![Role::Explorer, Role::Oracle, Role::Librarian]);
    }

    #[test]
    fn parse_chain_dedups_repeats() {
        assert_eq!(parse_chain("fixer then fixer again"), vec![Role::Fixer]);
    }

    #[test]
    fn parse_chain_falls_back_to_oracle() {
        assert_eq!(parse_chain("no role mentioned here"), vec![Role::Oracle]);
    }

    #[test]
    fn parse_skill_chain_reads_role_and_skill() {
        let steps = parse_skill_chain("fixer|test-driven-development");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].role, Role::Fixer);
        assert_eq!(
            steps[0].skill_name.as_deref(),
            Some("test-driven-development")
        );
    }

    #[test]
    fn parse_skill_chain_keeps_order_and_caps_at_three() {
        let steps = parse_skill_chain(
            "oracle|brainstorming\nfixer|test-driven-development\nlibrarian|requesting-code-review\ndesigner|none",
        );
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].role, Role::Oracle);
        assert_eq!(steps[1].role, Role::Fixer);
        assert_eq!(steps[2].role, Role::Librarian);
    }

    #[test]
    fn parse_skill_chain_none_empty_and_unknown_skill_become_none() {
        let steps = parse_skill_chain("oracle|none\nfixer|\nlibrarian|unknown-skill");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].skill_name, None);
        assert_eq!(steps[1].skill_name, None);
        assert_eq!(steps[2].skill_name, None);
    }

    #[test]
    fn parse_skill_chain_falls_back_to_oracle() {
        let steps = parse_skill_chain("no usable routing output");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].role, Role::Oracle);
        assert_eq!(steps[0].skill_name, None);
    }

    #[test]
    fn skill_planning_prompt_lists_roles_skills_and_task() {
        let skills = vec![SkillRouteOption {
            skill_name: "test-driven-development",
            description: "新功能或 bugfix 的測試先行實作",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Oracle),
        }];
        let p = skill_planning_prompt("fix the bug", &skills);
        assert!(p.contains("fixer"));
        assert!(p.contains("test-driven-development"));
        assert!(p.contains("新功能或 bugfix"));
        assert!(p.contains("fix the bug"));
        assert!(p.contains("<role>|<skill-or-none>"));
    }

    #[test]
    fn skill_planning_prompt_without_preferences_stays_unchanged() {
        let skills = vec![SkillRouteOption {
            skill_name: "systematic-debugging",
            description: "錯誤追因、根因定位",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Explorer),
        }];

        assert_eq!(
            skill_planning_prompt("fix it", &skills),
            skill_planning_prompt_with_preferences("fix it", &skills, None)
        );
    }

    #[test]
    fn skill_planning_prompt_with_preferences_includes_guidance_not_constraints() {
        let skills = vec![SkillRouteOption {
            skill_name: "systematic-debugging",
            description: "錯誤追因、根因定位",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Explorer),
        }];
        let hint = PlannerPreferenceHint {
            preferred_roles: vec![Role::Fixer, Role::Oracle],
            preferred_skills: vec!["systematic-debugging".to_string()],
        };

        let prompt = skill_planning_prompt_with_preferences("fix it", &skills, Some(&hint));

        assert!(prompt.contains("[User Preferences]"));
        assert!(prompt.contains("Preferred roles: fixer, oracle"));
        assert!(prompt.contains("Preferred skills: systematic-debugging"));
        assert!(prompt.contains("These are preferences, not hard constraints"));
        assert!(prompt.contains("Choose other roles or skills when the task requires it"));
        assert!(prompt.contains("fix it"));
    }

    #[test]
    fn skill_planning_prompt_skips_empty_preference_hint() {
        let skills = vec![SkillRouteOption {
            skill_name: "systematic-debugging",
            description: "錯誤追因、根因定位",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Explorer),
        }];
        let hint = PlannerPreferenceHint {
            preferred_roles: vec![],
            preferred_skills: vec![],
        };

        let prompt = skill_planning_prompt_with_preferences("fix it", &skills, Some(&hint));

        assert!(!prompt.contains("[User Preferences]"));
    }

    #[tokio::test]
    async fn skill_planning_uses_plan_agent_without_question_tool() {
        struct Backend(std::sync::Mutex<Option<AgentRequest>>);

        impl AiBackend for Backend {
            async fn run(
                &self,
                req: AgentRequest,
            ) -> Result<wukong_gateway::backend::AgentResponse, wukong_gateway::GatewayError>
            {
                *self.0.lock().unwrap() = Some(req);
                Ok(wukong_gateway::backend::AgentResponse {
                    text: "fixer|none".into(),
                    session_id: None,
                })
            }
        }

        let backend = Backend(std::sync::Mutex::new(None));
        let _ = plan_skill_chain_with_preferences(&backend, "pull updates", &[], None)
            .await
            .unwrap();

        let req = backend.0.lock().unwrap().take().unwrap();
        assert_eq!(req.agent.as_deref(), Some("plan"));
        assert_eq!(req.tool_overrides.get("question"), Some(&false));
    }
}
