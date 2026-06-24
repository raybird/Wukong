# Web Skill Preferences Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add global Web-configurable role and Superpowers preferences that persist in settings, guide the planner prompt, and appear in live Web helper-baton labels.

**Architecture:** Persist preferences as stable lowercase string IDs in `wukong-settings`, expose token-protected Web APIs for read/write, and thread an optional string-based hint through `GatewayConfig` to avoid a crate dependency cycle. `wukong-runtime` converts strings into orchestrator roles/skills before calling a new preference-aware skill planner, while existing callers keep compatible wrappers.

**Tech Stack:** Rust workspace crates (`wukong-settings`, `wukong-gateway`, `wukong-orchestrator`, `wukong-runtime`, `wukong-web`, `wukong-cli`, `wukong-telegram`, `wukong-scheduler`), Axum JSON APIs, serde settings JSON, vanilla ES Modules/Web Components.

---

## Spec Reference

- Read first: `docs/superpowers/specs/2026-06-24-web-skill-preferences-design.md`.
- Keep the first implementation global-only. Do not add per-scope overrides.
- Preferences are guidance only. Do not filter, force, or lock planner output.
- Preserve existing callers unless preferences are explicitly configured.
- Keep persisted historical `turn_steps` role-only in this phase.

## File Structure

- Modify `crates/wukong-settings/src/lib.rs`: add `PlannerPreferences`, serde defaults, and effective preference normalization.
- Modify `crates/wukong-gateway/src/config.rs`: add a string-only `PlannerPreferenceConfig` plus `planner_preferences` field and setter methods. Do not import `wukong-orchestrator` here.
- Modify all `GatewayConfig` struct literals in tests and runtime call sites to include `planner_preferences: None`.
- Modify `crates/wukong-orchestrator/src/router.rs`: add `PlannerPreferenceHint`, `skill_planning_prompt_with_preferences`, and `plan_skill_chain_with_preferences`; keep existing functions as wrappers.
- Modify `crates/wukong-orchestrator/src/lib.rs`: re-export `PlannerPreferenceHint`, `skill_planning_prompt_with_preferences`, and `plan_skill_chain_with_preferences`.
- Modify `crates/wukong-runtime/src/turn.rs`: add `ObservedStep`, `run_turn_traced`, convert gateway preference strings into orchestrator hints, and keep `run_turn` / `run_turn_observed` wrappers.
- Modify `crates/wukong-cli/src/lib.rs`: re-export `run_turn_traced` for Web's existing `wukong_cli` import boundary.
- Modify `crates/wukong-web/src/skills_api.rs`: add request/response structs and validation/normalization helpers for preference APIs.
- Modify `crates/wukong-web/src/lib.rs`: add `GET/PUT /api/skills/preferences`, load preferences into Web chat `GatewayConfig`, switch live chat from `run_turn_observed` to `run_turn_traced`, and include optional skill in SSE step events.
- Modify `crates/wukong-cli/src/main.rs`: apply settings preferences to CLI one-shot and inline REPL config after loading settings.
- Modify `crates/wukong-cli/src/repl.rs`: reload/apply current settings preferences for each turn, matching the existing session-command settings path behavior.
- Modify `crates/wukong-telegram/src/dispatch.rs`: apply preferences after loading settings for command/turn configs.
- Modify `crates/wukong-scheduler/src/executor.rs`: preserve `base_config.planner_preferences` when cloning config for scheduled turn jobs.
- Modify `crates/wukong-web/static/components/wukong-skills.js`: render preference controls, save preferences, and keep catalog cards below.
- Modify `crates/wukong-web/static/components/wukong-chat.js`: load preference status and render live step summary as role + skill when present.

---

### Task 1: Settings Persistence And Normalization

**Files:**
- Modify: `crates/wukong-settings/src/lib.rs`

- [ ] **Step 1: Write failing settings tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `crates/wukong-settings/src/lib.rs`:

```rust
#[test]
fn missing_planner_preferences_defaults_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"telegram":{"token":"123:abc","allowed":"42"}}"#).unwrap();

    let loaded = load_settings(&path).unwrap();

    assert_eq!(loaded.planner_preferences, PlannerPreferences::default());
}

#[test]
fn saves_and_loads_planner_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let settings = Settings {
        telegram: TelegramSettings::default(),
        agent: AgentSettings::default(),
        planner_preferences: PlannerPreferences {
            enabled: true,
            roles: vec!["fixer".to_string(), "oracle".to_string()],
            skills: vec!["systematic-debugging".to_string()],
        },
    };

    save_settings(&path, &settings).unwrap();
    let loaded = load_settings(&path).unwrap();

    assert_eq!(loaded.planner_preferences, settings.planner_preferences);
}

#[test]
fn effective_planner_preferences_trims_dedupes_and_drops_empty_values() {
    let settings = Settings {
        telegram: TelegramSettings::default(),
        agent: AgentSettings::default(),
        planner_preferences: PlannerPreferences {
            enabled: true,
            roles: vec![
                " fixer ".to_string(),
                "".to_string(),
                "oracle".to_string(),
                "fixer".to_string(),
            ],
            skills: vec![
                " systematic-debugging ".to_string(),
                "systematic-debugging".to_string(),
                " ".to_string(),
                "verification-before-completion".to_string(),
            ],
        },
    };

    let effective = effective_planner_preferences(&settings);

    assert!(effective.enabled);
    assert_eq!(effective.roles, vec!["fixer", "oracle"]);
    assert_eq!(
        effective.skills,
        vec!["systematic-debugging", "verification-before-completion"]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-settings planner_preferences -- --nocapture`

Expected: FAIL because `PlannerPreferences`, `Settings::planner_preferences`, and `effective_planner_preferences` do not exist yet.

- [ ] **Step 3: Implement settings model**

Update the top of `crates/wukong-settings/src/lib.rs` to include the new field and struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub planner_preferences: PlannerPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct PlannerPreferences {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}
```

Add this helper below `effective_agent_settings`:

```rust
pub fn effective_planner_preferences(file: &Settings) -> PlannerPreferences {
    PlannerPreferences {
        enabled: file.planner_preferences.enabled,
        roles: normalize_preference_ids(&file.planner_preferences.roles),
        skills: normalize_preference_ids(&file.planner_preferences.skills),
    }
}

fn normalize_preference_ids(values: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || normalized.iter().any(|existing| existing == value) {
            continue;
        }
        normalized.push(value.to_string());
    }
    normalized
}
```

Update all existing `Settings { ... }` test literals in this file to include `planner_preferences: PlannerPreferences::default(),`.

- [ ] **Step 4: Run settings tests**

Run: `cargo test -p wukong-settings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-settings/src/lib.rs
git commit -m "feat(settings): persist planner preferences"
```

---

### Task 2: Gateway Config Carries String-Based Preferences

**Files:**
- Modify: `crates/wukong-gateway/src/config.rs`
- Modify: every existing `GatewayConfig { ... }` literal that fails to compile after adding the new field

- [ ] **Step 1: Write failing gateway config test**

Add this test inside `crates/wukong-gateway/src/config.rs` tests:

```rust
#[test]
fn apply_planner_preferences_sets_none_when_disabled_or_empty() {
    let mut cfg = GatewayConfig {
        scope: "global".to_string(),
        db_url: "sqlite://x.db".to_string(),
        agent_command: vec!["opencode".to_string(), "run".to_string()],
        default_model: None,
        planner_preferences: None,
        thinking: true,
        recall_top_k: 5,
        stream: false,
    };

    cfg.apply_planner_preferences(false, vec!["fixer".to_string()], vec!["systematic-debugging".to_string()]);
    assert!(cfg.planner_preferences.is_none());

    cfg.apply_planner_preferences(true, vec![" ".to_string()], vec![]);
    assert!(cfg.planner_preferences.is_none());
}

#[test]
fn apply_planner_preferences_sets_normalized_values() {
    let mut cfg = GatewayConfig {
        scope: "global".to_string(),
        db_url: "sqlite://x.db".to_string(),
        agent_command: vec!["opencode".to_string(), "run".to_string()],
        default_model: None,
        planner_preferences: None,
        thinking: true,
        recall_top_k: 5,
        stream: false,
    };

    cfg.apply_planner_preferences(
        true,
        vec![" fixer ".to_string(), "fixer".to_string(), "oracle".to_string()],
        vec![" systematic-debugging ".to_string()],
    );

    let prefs = cfg.planner_preferences.unwrap();
    assert_eq!(prefs.preferred_roles, vec!["fixer", "oracle"]);
    assert_eq!(prefs.preferred_skills, vec!["systematic-debugging"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-gateway planner_preferences -- --nocapture`

Expected: FAIL because `PlannerPreferenceConfig`, `GatewayConfig::planner_preferences`, and `apply_planner_preferences` do not exist.

- [ ] **Step 3: Implement gateway preference config**

In `crates/wukong-gateway/src/config.rs`, add this type above `GatewayConfig`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPreferenceConfig {
    pub preferred_roles: Vec<String>,
    pub preferred_skills: Vec<String>,
}
```

Add this field to `GatewayConfig` after `default_model`:

```rust
pub planner_preferences: Option<PlannerPreferenceConfig>,
```

In `GatewayConfig::resolve`, set:

```rust
planner_preferences: None,
```

Add this method inside `impl GatewayConfig`:

```rust
pub fn apply_planner_preferences(
    &mut self,
    enabled: bool,
    roles: Vec<String>,
    skills: Vec<String>,
) {
    if !enabled {
        self.planner_preferences = None;
        return;
    }
    let preferred_roles = normalize_preference_ids(roles);
    let preferred_skills = normalize_preference_ids(skills);
    if preferred_roles.is_empty() && preferred_skills.is_empty() {
        self.planner_preferences = None;
        return;
    }
    self.planner_preferences = Some(PlannerPreferenceConfig {
        preferred_roles,
        preferred_skills,
    });
}
```

Add this helper near `split_ws`:

```rust
fn normalize_preference_ids(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || normalized.iter().any(|existing| existing == value) {
            continue;
        }
        normalized.push(value.to_string());
    }
    normalized
}
```

Update every `GatewayConfig { ... }` literal reported by the compiler to include:

```rust
planner_preferences: None,
```

- [ ] **Step 4: Run gateway tests**

Run: `cargo test -p wukong-gateway`

Expected: PASS.

- [ ] **Step 5: Run workspace check for struct literal fallout**

Run: `cargo check --workspace`

Expected: PASS. If it fails only because more `GatewayConfig` literals need the new field, add `planner_preferences: None` to those literals and rerun.

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-gateway/src/config.rs crates/wukong-cli/src crates/wukong-runtime/src crates/wukong-scheduler/src crates/wukong-telegram/src crates/wukong-web/src
git commit -m "feat(gateway): carry planner preferences in config"
```

---

### Task 3: Orchestrator Preference-Aware Skill Planning Prompt

**Files:**
- Modify: `crates/wukong-orchestrator/src/router.rs`
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: Write failing orchestrator tests**

Add these tests in `crates/wukong-orchestrator/src/router.rs` tests:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-orchestrator skill_planning_prompt -- --nocapture`

Expected: FAIL because `PlannerPreferenceHint` and `skill_planning_prompt_with_preferences` do not exist.

- [ ] **Step 3: Implement preference-aware prompt builder**

Add this type near `PlannedStep`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPreferenceHint {
    pub preferred_roles: Vec<Role>,
    pub preferred_skills: Vec<String>,
}
```

Replace the current `skill_planning_prompt` function with a wrapper plus implementation:

```rust
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
```

Add this async wrapper below `plan_skill_chain` or replace `plan_skill_chain` with wrapper plus implementation:

```rust
pub async fn plan_skill_chain_with_preferences(
    backend: &impl AiBackend,
    task: &str,
    skills: &[SkillRouteOption],
    preferences: Option<&PlannerPreferenceHint>,
) -> Result<Vec<PlannedStep>, OrchestratorError> {
    let resp = backend
        .run(AgentRequest {
            prompt: skill_planning_prompt_with_preferences(task, skills, preferences),
            session_id: None,
            thinking: false,
            model: None,
        })
        .await?;
    Ok(parse_skill_chain(&resp.text))
}
```

Then make `plan_skill_chain` delegate:

```rust
pub async fn plan_skill_chain(
    backend: &impl AiBackend,
    task: &str,
    skills: &[SkillRouteOption],
) -> Result<Vec<PlannedStep>, OrchestratorError> {
    plan_skill_chain_with_preferences(backend, task, skills, None).await
}
```

- [ ] **Step 4: Re-export preference-aware orchestrator APIs**

Update `crates/wukong-orchestrator/src/lib.rs` exports so downstream crates can use the new public API:

```rust
pub use router::{
    parse_chain, parse_role, parse_skill_chain, plan_chain, plan_skill_chain,
    plan_skill_chain_with_preferences, planning_prompt, route, routing_prompt,
    skill_planning_prompt, skill_planning_prompt_with_preferences, PlannedStep,
    PlannerPreferenceHint, SkillRouteOption,
};
```

- [ ] **Step 5: Run orchestrator tests**

Run: `cargo test -p wukong-orchestrator`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-orchestrator/src/router.rs crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): guide skill planning with preferences"
```

---

### Task 4: Runtime Applies Preferences And Exposes Traced Steps

**Files:**
- Modify: `crates/wukong-runtime/src/turn.rs`
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: Write failing runtime tests**

Add these tests to `crates/wukong-runtime/src/turn.rs` tests:

```rust
#[tokio::test]
async fn run_turn_sends_planner_preferences_to_skill_planner() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&["fixer|systematic-debugging", "done"]);
    let mut cfg = test_cfg("project:T");
    cfg.apply_planner_preferences(
        true,
        vec!["fixer".to_string(), "oracle".to_string()],
        vec!["systematic-debugging".to_string()],
    );

    run_turn(&mem, &backend, &cfg, "fix the bug", &mut |_| {}, &mut |_| {})
        .await
        .unwrap();

    let planner_prompt = backend.prompts.lock().unwrap()[0].clone();
    assert!(planner_prompt.contains("[User Preferences]"));
    assert!(planner_prompt.contains("Preferred roles: fixer, oracle"));
    assert!(planner_prompt.contains("Preferred skills: systematic-debugging"));
    assert!(planner_prompt.contains("not hard constraints"));
}

#[tokio::test]
async fn run_turn_drops_invalid_preference_ids_before_planning() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&["oracle|none", "answer"]);
    let mut cfg = test_cfg("project:T");
    cfg.apply_planner_preferences(
        true,
        vec!["unknown-role".to_string()],
        vec!["unknown-skill".to_string()],
    );

    run_turn(&mem, &backend, &cfg, "think", &mut |_| {}, &mut |_| {})
        .await
        .unwrap();

    let planner_prompt = backend.prompts.lock().unwrap()[0].clone();
    assert!(!planner_prompt.contains("[User Preferences]"));
}

#[tokio::test]
async fn run_turn_traced_reports_role_skill_and_output_for_helper_steps() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&["explorer|systematic-debugging\nfixer|none", "e1", "f2"]);
    let mut steps: Vec<(Role, Option<String>, String)> = Vec::new();

    let out = run_turn_traced(
        &mem,
        &backend,
        &test_cfg("project:T"),
        "build and fix",
        &mut |_| {},
        &mut |_| {},
        &mut |step| {
            steps.push((
                step.role,
                step.skill_name.map(str::to_string),
                step.output.to_string(),
            ));
        },
    )
    .await
    .unwrap();

    assert_eq!(
        steps,
        vec![(
            Role::Explorer,
            Some("systematic-debugging".to_string()),
            "e1".to_string()
        )]
    );
    assert_eq!(out.role, Role::Fixer);
    assert_eq!(out.text, "f2");
}

#[tokio::test]
async fn run_turn_observed_remains_role_output_compatible() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&["explorer|systematic-debugging\nfixer|none", "e1", "f2"]);
    let mut steps: Vec<(Role, String)> = Vec::new();

    run_turn_observed(
        &mem,
        &backend,
        &test_cfg("project:T"),
        "build and fix",
        &mut |_| {},
        &mut |_| {},
        &mut |role, output| steps.push((role, output.to_string())),
    )
    .await
    .unwrap();

    assert_eq!(steps, vec![(Role::Explorer, "e1".to_string())]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-runtime run_turn_ -- --nocapture`

Expected: FAIL because `run_turn_traced` and runtime preference conversion do not exist yet.

- [ ] **Step 3: Implement runtime preference conversion**

Update imports at the top of `crates/wukong-runtime/src/turn.rs`:

```rust
use wukong_orchestrator::{PlannerPreferenceHint, Role};
```

Add helpers near `append_empty_output_repair_directive`:

```rust
fn parse_preferred_role(name: &str) -> Option<Role> {
    let normalized = name.trim().to_ascii_lowercase();
    Role::all()
        .into_iter()
        .find(|role| role.name() == normalized)
}

fn planner_preference_hint(cfg: &GatewayConfig) -> Option<PlannerPreferenceHint> {
    let prefs = cfg.planner_preferences.as_ref()?;
    let preferred_roles = prefs
        .preferred_roles
        .iter()
        .filter_map(|role| parse_preferred_role(role))
        .collect::<Vec<_>>();
    let preferred_skills = prefs
        .preferred_skills
        .iter()
        .filter(|skill| find_skill(skill).is_some())
        .cloned()
        .collect::<Vec<_>>();
    if preferred_roles.is_empty() && preferred_skills.is_empty() {
        None
    } else {
        Some(PlannerPreferenceHint {
            preferred_roles,
            preferred_skills,
        })
    }
}
```

- [ ] **Step 4: Implement traced turn wrapper**

Add this public struct above `run_turn`:

```rust
pub struct ObservedStep<'a> {
    pub role: Role,
    pub skill_name: Option<&'a str>,
    pub output: &'a str,
}
```

Change `run_turn` to delegate to `run_turn_observed` exactly as it does today.

Change `run_turn_observed` body to delegate to `run_turn_traced`:

```rust
pub async fn run_turn_observed(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(Role, &str),
) -> Result<TurnOutput, WukongError> {
    run_turn_traced(
        memory,
        backend,
        cfg,
        input,
        on_event,
        on_role,
        &mut |step| on_step(step.role, step.output),
    )
    .await
}
```

Move the current implementation body of `run_turn_observed` into a new function with this signature:

```rust
pub async fn run_turn_traced(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep<'_>),
) -> Result<TurnOutput, WukongError> {
```

Inside `run_turn_traced`, replace the planner call:

```rust
let preference_hint = planner_preference_hint(cfg);
let steps = wukong_orchestrator::plan_skill_chain_with_preferences(
    backend,
    input,
    &route_options(),
    preference_hint.as_ref(),
)
.await?;
```

Replace the helper-step callback:

```rust
if !is_final && !text.trim().is_empty() {
    on_step(ObservedStep {
        role,
        skill_name: step.skill_name.as_deref(),
        output: &text,
    });
}
```

Important: compute `let skill = step.skill_name.as_deref().and_then(find_skill);` before the backend call as it does today, but do not move `step.skill_name` out before the helper callback needs it.

- [ ] **Step 5: Re-export traced runtime API from CLI crate**

Update `crates/wukong-cli/src/lib.rs` to re-export the new traced turn API:

```rust
pub use wukong_runtime::{
    run_turn, run_turn_observed, run_turn_session_passthrough, run_turn_traced, TurnOutput,
    WukongError,
};
```

- [ ] **Step 6: Run runtime tests**

Run: `cargo test -p wukong-runtime`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-runtime/src/turn.rs crates/wukong-cli/src/lib.rs
git commit -m "feat(runtime): trace skill-aware helper steps"
```

---

### Task 5: Web Skills Preferences API

**Files:**
- Modify: `crates/wukong-web/src/skills_api.rs`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run pre-change API impact analysis**

Run GitNexus API impact before editing `build_router`/route handlers:

```text
gitnexus_api_impact({ route: "/api/skills/catalog", repo: "Wukong" })
```

Expected: note existing consumers and route protection. If risk is HIGH or CRITICAL, report it before editing.

- [ ] **Step 2: Write failing Web API tests**

Add tests to `crates/wukong-web/src/lib.rs` test module:

```rust
#[tokio::test]
async fn skills_preferences_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/skills/preferences")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_skills_preferences_returns_defaults() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/skills/preferences")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""enabled":false"#));
    assert!(body.contains(r#""roles":[]"#));
    assert!(body.contains(r#""skills":[]"#));
    assert!(body.contains(r#""warnings":[]"#));
}

#[tokio::test]
async fn put_skills_preferences_persists_normalized_values() {
    let state = state(None, &[]).await;
    let settings_path = state.settings_path.clone();
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/skills/preferences")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"enabled":true,"roles":["fixer","fixer","oracle"],"skills":["systematic-debugging"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""enabled":true"#));
    assert!(body.contains(r#""roles":["fixer","oracle"]"#));
    assert!(body.contains(r#""skills":["systematic-debugging"]"#));
    let saved = wukong_settings::load_settings(&settings_path).unwrap();
    assert!(saved.planner_preferences.enabled);
    assert_eq!(saved.planner_preferences.roles, vec!["fixer", "oracle"]);
    assert_eq!(saved.planner_preferences.skills, vec!["systematic-debugging"]);
}

#[tokio::test]
async fn put_skills_preferences_rejects_unknown_role() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/skills/preferences")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"enabled":true,"roles":["not-a-role"],"skills":[]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(body_string(resp).await.contains("unknown role"));
}

#[tokio::test]
async fn put_skills_preferences_rejects_unknown_skill() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/skills/preferences")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"enabled":true,"roles":[],"skills":["not-a-skill"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(body_string(resp).await.contains("unknown skill"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p wukong-web skills_preferences -- --nocapture`

Expected: FAIL with 404 or missing route/handlers.

- [ ] **Step 4: Implement skills API validation helpers**

In `crates/wukong-web/src/skills_api.rs`, update imports:

```rust
use serde::{Deserialize, Serialize};
```

Add these structs:

```rust
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
```

Add these helpers:

```rust
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
```

- [ ] **Step 5: Implement Web route handlers**

In `crates/wukong-web/src/lib.rs`, add handlers near `get_skills_catalog`:

```rust
async fn get_skills_preferences<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => {
            let prefs = wukong_settings::effective_planner_preferences(&settings);
            Json(skills_api::preferences_response(&prefs)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_skills_preferences<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<skills_api::SaveSkillPreferencesRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let prefs = match skills_api::validate_preferences(req) {
        Ok(prefs) => prefs,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let mut settings = wukong_settings::load_settings(&state.settings_path).unwrap_or_default();
    settings.planner_preferences = prefs;
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => {
            let effective = wukong_settings::effective_planner_preferences(&settings);
            Json(skills_api::preferences_response(&effective)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

Add the route to `build_router` after `/api/skills/catalog`:

```rust
.route(
    "/api/skills/preferences",
    axum::routing::get(get_skills_preferences::<B>).put(put_skills_preferences::<B>),
)
```

- [ ] **Step 6: Run Web API tests**

Run: `cargo test -p wukong-web skills_preferences -- --nocapture`

Expected: PASS.

- [ ] **Step 7: Run broader Web tests**

Run: `cargo test -p wukong-web`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/wukong-web/src/lib.rs crates/wukong-web/src/skills_api.rs
git commit -m "feat(web): add skill preference APIs"
```

---

### Task 6: Apply Settings Preferences In Entry Points

**Files:**
- Modify: `crates/wukong-cli/src/main.rs`
- Modify: `crates/wukong-cli/src/repl.rs`
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-web/src/lib.rs`
- Modify: `crates/wukong-scheduler/src/executor.rs` only if tests show base config preferences are dropped

- [ ] **Step 1: Update CLI one-shot settings application**

In `crates/wukong-cli/src/main.rs`, the top of `main` currently loads settings and applies the default model:

```rust
let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
let agent_settings = wukong_settings::effective_agent_settings(&settings);
cfg.apply_default_model(agent_settings.default_model.as_deref());
```

Replace it with:

```rust
let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
apply_settings_to_config(&mut cfg, &settings);
```

- [ ] **Step 2: Add a private settings helper in CLI main**

Add this private helper in `crates/wukong-cli/src/main.rs` near `run_one`:

```rust
fn apply_settings_to_config(cfg: &mut GatewayConfig, settings: &wukong_settings::Settings) {
    let agent_settings = wukong_settings::effective_agent_settings(settings);
    cfg.apply_default_model(agent_settings.default_model.as_deref());
    let planner_preferences = wukong_settings::effective_planner_preferences(settings);
    cfg.apply_planner_preferences(
        planner_preferences.enabled,
        planner_preferences.roles,
        planner_preferences.skills,
    );
}
```

- [ ] **Step 3: Update REPL turn setup**

In `crates/wukong-cli/src/repl.rs`, inside `LineAction::Turn(input)`, load and apply settings before calling `run_turn`:

```rust
let settings_path = wukong_settings::default_settings_path();
let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
let agent_settings = wukong_settings::effective_agent_settings(&settings);
cfg.apply_default_model(agent_settings.default_model.as_deref());
let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
cfg.apply_planner_preferences(
    planner_preferences.enabled,
    planner_preferences.roles,
    planner_preferences.skills,
);
```

- [ ] **Step 4: Update Telegram config setup**

In `crates/wukong-telegram/src/dispatch.rs`, after both existing `cfg.apply_default_model(...)` calls, add:

```rust
let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
cfg.apply_planner_preferences(
    planner_preferences.enabled,
    planner_preferences.roles,
    planner_preferences.skills,
);
```

- [ ] **Step 5: Update Web chat config setup**

In `crates/wukong-web/src/lib.rs`, after:

```rust
cfg.apply_default_model(agent_settings.default_model.as_deref());
```

add:

```rust
let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
cfg.apply_planner_preferences(
    planner_preferences.enabled,
    planner_preferences.roles,
    planner_preferences.skills,
);
```

- [ ] **Step 6: Add runtime-facing verification through Web chat test**

Add a Web test in `crates/wukong-web/src/lib.rs` test module. The current `MockBackend` only stores replies; extend it to store prompts if needed:

```rust
struct MockBackend {
    replies: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<String>>,
}
```

In its `run` implementation, push `req.prompt` before returning.

Then add:

```rust
#[tokio::test]
async fn chat_applies_saved_planner_preferences_to_turn_config() {
    let state = state(None, &["fixer|systematic-debugging", "answer"]).await;
    let backend = state.backend.clone();
    let settings = wukong_settings::Settings {
        telegram: wukong_settings::TelegramSettings::default(),
        agent: wukong_settings::AgentSettings::default(),
        planner_preferences: wukong_settings::PlannerPreferences {
            enabled: true,
            roles: vec!["fixer".to_string()],
            skills: vec!["systematic-debugging".to_string()],
        },
    };
    wukong_settings::save_settings(&state.settings_path, &settings).unwrap();
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=fix%20it")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = body_string(resp).await;

    let prompts = backend.prompts.lock().unwrap();
    assert!(prompts[0].contains("[User Preferences]"));
    assert!(prompts[0].contains("Preferred roles: fixer"));
    assert!(prompts[0].contains("Preferred skills: systematic-debugging"));
}
```

- [ ] **Step 7: Run entry-point tests**

Run: `cargo test -p wukong-cli -p wukong-telegram -p wukong-scheduler -p wukong-web`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/wukong-cli/src/main.rs crates/wukong-cli/src/repl.rs crates/wukong-telegram/src/dispatch.rs crates/wukong-web/src/lib.rs crates/wukong-scheduler/src/executor.rs
git commit -m "feat(runtime): apply saved planner preferences"
```

---

### Task 7: Web SSE Step Events Include Skill Metadata

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write failing SSE serialization test**

Add a focused unit test in `crates/wukong-web/src/lib.rs` tests:

```rust
#[test]
fn sse_step_event_includes_skill_when_present() {
    let event = SseMsg::Step {
        role: "fixer".to_string(),
        skill: Some("systematic-debugging".to_string()),
        html: "<p>done</p>".to_string(),
    }
    .into_event();
    let debug = format!("{event:?}");

    assert!(debug.contains("step"));
    assert!(debug.contains("systematic-debugging"));
}
```

If `Event` debug output is not stable enough, skip this unit test and verify through the end-to-end chat SSE test in Step 2.

- [ ] **Step 2: Write failing chat SSE test**

Add this test to the Web test module:

```rust
#[tokio::test]
async fn chat_step_event_includes_skill_when_planned() {
    let app = build_router(state(None, &["explorer|systematic-debugging\nfixer|none", "helper", "final"]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=debug%20it")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("event: step"));
    assert!(body.contains(r#""role":"explorer""#));
    assert!(body.contains(r#""skill":"systematic-debugging""#));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p wukong-web step_event -- --nocapture`

Expected: FAIL because `SseMsg::Step` has no `skill` field and Web chat still calls `run_turn_observed`.

- [ ] **Step 4: Update `SseMsg::Step`**

In `crates/wukong-web/src/lib.rs`, change the enum variant:

```rust
Step { role: String, skill: Option<String>, html: String },
```

Update `into_event`:

```rust
SseMsg::Step { role, skill, html } => Event::default()
    .event("step")
    .data(serde_json::json!({ "role": role, "skill": skill, "html": html }).to_string()),
```

- [ ] **Step 5: Switch Web chat to `run_turn_traced`**

At the top of `crates/wukong-web/src/lib.rs`, replace:

```rust
use wukong_cli::run_turn_observed;
```

with:

```rust
use wukong_cli::run_turn_traced;
```

In the chat handler, replace `run_turn_observed(` with `run_turn_traced(` and update the step callback:

```rust
&mut |step| {
    let html = wukong_render::to_web_html(step.output);
    let _ = step_tx.send(SseMsg::Step {
        role: step.role.name().to_string(),
        skill: step.skill_name.map(str::to_string),
        html: html.clone(),
    });
    steps_buf.push((step.role.name().to_string(), step.output.to_string(), html));
},
```

Do not add skill to `steps_buf`; persisted historical helper steps remain role-only.

- [ ] **Step 6: Run Web SSE tests**

Run: `cargo test -p wukong-web step_event -- --nocapture`

Expected: PASS.

- [ ] **Step 7: Run Web tests**

Run: `cargo test -p wukong-web`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat(web): include skill in live step events"
```

---

### Task 8: Skills Panel Preference Controls

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-skills.js`
- Modify: `crates/wukong-web/static/styles.css` only if the existing form/card classes are insufficient

- [ ] **Step 1: Inspect existing style classes**

Open `crates/wukong-web/static/styles.css` and confirm whether `.control-card`, `.control-row`, `.settings-status`, `.tag`, and form controls already cover the new UI. Prefer reusing them.

- [ ] **Step 2: Replace `wukong-skills.js` with preference-aware component**

Use this structure in `crates/wukong-web/static/components/wukong-skills.js`:

```js
import { html, escapeHTML } from '/lib/html.js';

const ROLE_LABELS = {
  explorer: 'Explorer',
  oracle: 'Oracle',
  librarian: 'Librarian',
  fixer: 'Fixer',
  designer: 'Designer',
};

export class WukongSkills extends HTMLElement {
  connectedCallback() {
    this.catalog = { roles: [], skills: [] };
    this.preferences = { enabled: false, roles: [], skills: [], warnings: [] };
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>技能</h2>
            <p class="panel-help">設定全域角色與 Superpowers 偏好。這是 planner guidance，不會強制鎖定路由。</p>
          </div>
        </div>
        <div id="skills-status" class="settings-status">載入中…</div>
        <section class="control-card">
          <h3>Planner 偏好</h3>
          <label><input id="pref-enabled" type="checkbox" /> Enable planner preferences</label>
          <p class="panel-help">偏好會提示悟空優先考慮，但任務需要時仍可選擇其他角色或技能。</p>
          <h4>角色偏好</h4>
          <div id="role-preferences" class="control-row"></div>
          <h4>Superpowers 偏好</h4>
          <div id="skill-preferences" class="skill-grid"></div>
          <button id="save-preferences" type="button">儲存偏好</button>
        </section>
        <section class="control-card"><h3>角色</h3><div id="roles" class="control-row"></div></section>
        <section><h3>Superpowers</h3><div id="skills" class="skill-grid"></div></section>
      </section>
    `.toString();
    this.status = this.querySelector('#skills-status');
    this.enabled = this.querySelector('#pref-enabled');
    this.rolePreferences = this.querySelector('#role-preferences');
    this.skillPreferences = this.querySelector('#skill-preferences');
    this.roles = this.querySelector('#roles');
    this.skills = this.querySelector('#skills');
    this.querySelector('#save-preferences').addEventListener('click', () => this.savePreferences());
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    try {
      const [catalogResp, prefResp] = await Promise.all([
        fetch('/api/skills/catalog' + this.tokenParam()),
        fetch('/api/skills/preferences' + this.tokenParam()),
      ]);
      if (!catalogResp.ok) throw new Error('技能目錄 HTTP ' + catalogResp.status);
      if (!prefResp.ok) throw new Error('偏好 HTTP ' + prefResp.status);
      this.catalog = await catalogResp.json();
      this.preferences = await prefResp.json();
      this.status.textContent = '已載入技能目錄與偏好';
      this.render();
    } catch (err) {
      this.status.textContent = '無法讀取技能設定：' + err.message;
    }
  }

  render() {
    const preferredRoles = new Set(this.preferences.roles || []);
    const preferredSkills = new Set(this.preferences.skills || []);
    this.enabled.checked = Boolean(this.preferences.enabled);
    this.rolePreferences.innerHTML = (this.catalog.roles || []).map((role) => {
      const id = String(role.name || '').toLowerCase();
      const label = ROLE_LABELS[id] || role.name || id;
      return '<label class="tag"><input type="checkbox" name="preferred-role" value="' + escapeHTML(id) + '" ' + (preferredRoles.has(id) ? 'checked' : '') + ' /> ' + escapeHTML(label) + '</label>';
    }).join('');
    this.skillPreferences.innerHTML = (this.catalog.skills || []).map((skill) => {
      const name = skill.name || '';
      return '<label class="skill-card"><input type="checkbox" name="preferred-skill" value="' + escapeHTML(name) + '" ' + (preferredSkills.has(name) ? 'checked' : '') + ' /> <strong>' + escapeHTML(name) + '</strong><p>' + escapeHTML(skill.description || '') + '</p></label>';
    }).join('');
    this.roles.innerHTML = (this.catalog.roles || []).map((role) => '<span class="tag">' + escapeHTML(role.name) + '</span>').join('');
    this.skills.innerHTML = (this.catalog.skills || []).map((skill) => html`
      <article class="skill-card">
        <h3>${skill.name}</h3>
        <p>${skill.description}</p>
        <p><span class="tag">主責 ${skill.primary_role}</span> ${skill.collaborator_role ? '<span class="tag">協作 ' + escapeHTML(skill.collaborator_role) + '</span>' : ''}</p>
      </article>
    `.toString()).join('');
  }

  selectedValues(name) {
    return Array.from(this.querySelectorAll('input[name="' + name + '"]:checked')).map((input) => input.value);
  }

  async savePreferences() {
    const payload = {
      enabled: this.enabled.checked,
      roles: this.selectedValues('preferred-role'),
      skills: this.selectedValues('preferred-skill'),
    };
    this.status.textContent = '儲存中…';
    const resp = await fetch('/api/skills/preferences' + this.tokenParam(), {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      this.status.textContent = '儲存失敗：' + await resp.text();
      return;
    }
    this.preferences = await resp.json();
    this.status.textContent = this.preferences.enabled ? '已儲存並啟用技能偏好' : '已儲存，技能偏好未啟用';
    this.render();
  }
}
```

- [ ] **Step 3: Run Web tests/checks**

Run: `cargo test -p wukong-web`

Expected: PASS. This proves the embedded file path is valid and the Rust crate still builds; it does not parse JavaScript syntax. The manual browser smoke in the next step is required for JS behavior.

- [ ] **Step 4: Manual browser smoke**

Run: `cargo run -p wukong-web -- --no-stream`

Expected: server starts. Open the Web Console, go to Skills, confirm catalog and preference controls load, save a role and skill, reload the page, and confirm selections persist.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-web/static/components/wukong-skills.js crates/wukong-web/static/styles.css
git commit -m "feat(web): edit skill preferences from console"
```

---

### Task 9: Chat Preference Status And Skill-Aware Baton Labels

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`

- [ ] **Step 1: Update initialization to track skill status element**

In `connectedCallback`, after:

```js
this.modelStatus = this.querySelector('#chat-model-status');
```

add:

```js
this.skillStatus = this.querySelector('#chat-skill-status');
```

In `initialize`, change:

```js
await this.loadModelStatus();
```

to:

```js
await Promise.all([this.loadModelStatus(), this.loadSkillStatus()]);
```

- [ ] **Step 2: Add skill status loader**

Add this method after `loadModelStatus`:

```js
async loadSkillStatus() {
  if (!this.skillStatus) return;
  const token = window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  try {
    const resp = await fetch('/api/skills/preferences' + token);
    if (!resp.ok) {
      this.skillStatus.textContent = '技能偏好：讀取失敗';
      return;
    }
    const data = await resp.json();
    this.skillStatus.textContent = data.enabled ? '技能偏好：已啟用' : '技能偏好：未啟用';
  } catch (_err) {
    this.skillStatus.textContent = '技能偏好：讀取失敗';
  }
}
```

- [ ] **Step 3: Render live step summary with skill when present**

In the `step` event listener, replace:

```js
let role = '', stepHtml = '';
```

with:

```js
let role = '', skill = '', stepHtml = '';
```

After parsing JSON, add:

```js
skill = parsed.skill || '';
```

Replace the summary string with:

```js
const label = skill ? role + ' + ' + skill : role;
details.innerHTML =
  '<summary>🔍 悟空·' + escapeHTML(label) + ' 的產出</summary>' +
  '<div class="baton-body">' + stepHtml + '</div>';
```

Do not change `lazyStepsNode`; history-loaded steps remain role-only.

- [ ] **Step 4: Run Web tests**

Run: `cargo test -p wukong-web`

Expected: PASS.

- [ ] **Step 5: Manual browser smoke**

Run: `cargo run -p wukong-web -- --no-stream`

Expected: Chat toolbar shows `技能偏好：已啟用` or `技能偏好：未啟用` based on saved settings. A live multi-step turn with a planned skill shows a baton summary like `悟空·fixer + systematic-debugging 的產出`.

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat(web): show skill preference status in chat"
```

---

### Task 10: Final Verification And Change Impact

**Files:**
- No planned edits unless verification finds failures.

- [ ] **Step 1: Run formatting**

Run: `cargo fmt`

Expected: exits 0.

- [ ] **Step 2: Run targeted crate tests**

Run: `cargo test -p wukong-settings -p wukong-gateway -p wukong-orchestrator -p wukong-runtime -p wukong-web -p wukong-cli -p wukong-telegram -p wukong-scheduler`

Expected: PASS.

- [ ] **Step 3: Run full workspace tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Run GitNexus change detection before any final commit or completion claim**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols match settings preferences, gateway config, orchestrator planner prompt, runtime tracing, Web APIs, and Web UI. If risk is HIGH or CRITICAL, summarize why before proceeding.

- [ ] **Step 6: Commit any final formatting-only changes**

If `cargo fmt` changed files after the previous commits:

```bash
git status --short
git diff
git add <formatted-files>
git commit -m "chore: format skill preferences changes"
```

If there are no changes, do not create an empty commit.

---

## Self-Review Checklist

- Spec coverage: Tasks 1, 5, 8 cover Web-configurable global preferences and persistence; Tasks 3, 4, 6 cover planner guidance and all settings-loading entry points; Tasks 7, 9 cover live Web skill display; Task 10 covers verification and GitNexus change detection.
- Token protection: Task 5 explicitly checks `GET /api/skills/preferences` returns `401` when a token is configured and omitted.
- Compatibility: Tasks 3 and 4 preserve `skill_planning_prompt`, `plan_skill_chain`, `run_turn`, and `run_turn_observed` as wrappers.
- Crate dependency safety: `GatewayConfig` stores only string IDs; orchestrator role conversion happens in `wukong-runtime`, avoiding a `wukong-gateway` -> `wukong-orchestrator` dependency cycle.
- Historical data: Task 7 intentionally does not add skill to persisted `turn_steps`.
- Placeholder scan: no task uses TBD/TODO/fill-in-later language; each code-changing step includes concrete code or exact location-specific replacement instructions.
