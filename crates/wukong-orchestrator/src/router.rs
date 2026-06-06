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

/// Phase 1 (chain): ask the backend for an ordered role chain.
pub async fn plan_chain(backend: &impl AiBackend, task: &str) -> Result<Vec<Role>, OrchestratorError> {
    let resp = backend
        .run(AgentRequest {
            prompt: planning_prompt(task),
            continue_session: false,
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
        assert_eq!(parse_chain("先 fixer 再 explorer"), vec![Role::Fixer, Role::Explorer]);
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
}
