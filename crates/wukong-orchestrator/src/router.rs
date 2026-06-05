use crate::error::OrchestratorError;
use crate::role::Role;
use wukong_gateway::backend::{AgentRequest, AiBackend};

/// Build the routing prompt: list the roles and ask for exactly one name.
pub fn routing_prompt(task: &str) -> String {
    let mut s = String::from(
        "You are a router. Pick the single best role to handle the task.\nRoles:\n",
    );
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

/// Phase 1: ask the backend which role should handle the task.
pub async fn route(backend: &impl AiBackend, task: &str) -> Result<Role, OrchestratorError> {
    let resp = backend
        .run(AgentRequest {
            prompt: routing_prompt(task),
            continue_session: false,
        })
        .await?;
    Ok(parse_role(&resp.text))
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
}
