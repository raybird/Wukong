use serde::{Deserialize, Serialize};
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

#[derive(Debug, Deserialize)]
pub struct SaveSkillPreferencesRequest {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SkillPreferencesResponse {
    pub enabled: bool,
    pub roles: Vec<String>,
    pub skills: Vec<String>,
    pub warnings: Vec<String>,
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

pub fn preferences_response(
    prefs: &wukong_settings::PlannerPreferences,
) -> SkillPreferencesResponse {
    SkillPreferencesResponse {
        enabled: prefs.enabled,
        roles: prefs.roles.clone(),
        skills: prefs.skills.clone(),
        warnings: Vec::new(),
    }
}

pub fn validate_preferences(
    req: SaveSkillPreferencesRequest,
) -> Result<wukong_settings::PlannerPreferences, String> {
    let roles = normalize_unique(req.roles, validate_role)?;
    let skills = normalize_unique(req.skills, validate_skill)?;
    Ok(wukong_settings::PlannerPreferences {
        enabled: req.enabled,
        roles,
        skills,
    })
}

fn normalize_unique(
    values: Vec<String>,
    validate: fn(&str) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_ascii_lowercase();
        if value.is_empty() || normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        validate(&value)?;
        normalized.push(value);
    }
    Ok(normalized)
}

fn validate_role(role: &str) -> Result<(), String> {
    if Role::all().iter().any(|known| known.name() == role) {
        Ok(())
    } else {
        Err(format!("unknown role: {role}"))
    }
}

fn validate_skill(skill: &str) -> Result<(), String> {
    if wukong_skills::find(skill).is_some() {
        Ok(())
    } else {
        Err(format!("unknown skill: {skill}"))
    }
}
