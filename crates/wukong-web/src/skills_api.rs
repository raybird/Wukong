use serde::Serialize;
use wukong_orchestrator::Role;

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub name: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SkillResponse {
    pub name: &'static str,
    pub description: &'static str,
    pub primary_role: &'static str,
    pub collaborator_role: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct SkillsCatalogResponse {
    pub roles: Vec<RoleResponse>,
    pub skills: Vec<SkillResponse>,
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Explorer => "Explorer",
        Role::Oracle => "Oracle",
        Role::Librarian => "Librarian",
        Role::Fixer => "Fixer",
        Role::Designer => "Designer",
    }
}

pub fn catalog_response() -> SkillsCatalogResponse {
    SkillsCatalogResponse {
        roles: vec![
            RoleResponse { name: "Explorer" },
            RoleResponse { name: "Oracle" },
            RoleResponse { name: "Librarian" },
            RoleResponse { name: "Fixer" },
            RoleResponse { name: "Designer" },
        ],
        skills: wukong_skills::catalog::all()
            .iter()
            .map(|skill| SkillResponse {
                name: skill.name,
                description: skill.description,
                primary_role: role_name(skill.primary_role),
                collaborator_role: skill.collaborator_role.map(role_name),
            })
            .collect(),
    }
}
