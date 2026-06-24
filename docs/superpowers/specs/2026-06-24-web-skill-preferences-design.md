# Web Skill Preferences Design

**Date:** 2026-06-24  
**Status:** Draft approved for planning  
**Scope:** Phase 2 of Web Console Control Center: global role/Superpowers preferences, planner guidance, and skill-aware Web baton display.

## Context

Phase 1 turned `wukong-web` into a tabbed Control Center with read-only Memory and Skills panels plus global model settings. The Skills panel currently shows available roles and Superpowers from `wukong-skills`, but users cannot express preferences. Runtime skill routing already exists: `wukong-runtime` calls `wukong_orchestrator::plan_skill_chain`, then injects selected `SKILL.md` content into execution prompts.

Phase 2 makes the Skills panel actionable while preserving Wukong's automatic planner behavior. Users can prefer roles and skills, but the planner may still choose other roles or skills when the task calls for it.

## Goals

- Let users configure global preferred roles and preferred Superpowers in Web Console.
- Persist preferences in existing `wukong-settings` JSON settings.
- Expose preference APIs under `/api/skills/preferences` with existing Web token protection.
- Inject preferences into the planner prompt as guidance, not hard constraints.
- Display selected skill names in Web intermediate baton cards when available.
- Preserve existing `run_turn`, CLI, Telegram, Scheduler, and Web chat behavior unless preferences are explicitly configured.

## Non-Goals

- No per-scope or per-project preference override in this phase.
- No forced role or skill locking.
- No per-role model assignment.
- No new skill installation or custom skill authoring UI.
- No planner preference controls in CLI or Telegram in this phase.
- No automatic migration beyond serde defaults for older settings files.

## User Experience

The `Skills` panel gains a preferences section above the catalog:

- `Enable planner preferences` checkbox.
- Role checkboxes: Explorer, Oracle, Librarian, Fixer, Designer.
- Superpowers checkboxes using the existing catalog.
- Save button.
- Status text explaining that preferences are guidance, not forced routing.

The catalog remains visible below the preference controls so users can understand what each skill does before selecting it.

The Chat toolbar keeps the Phase 1 skill status indicator, but updates from `技能偏好：Phase 2` to one of:

- `技能偏好：未啟用`
- `技能偏好：已啟用`
- `技能偏好：讀取失敗`

When a multi-step Web turn emits intermediate baton cards, the baton summary should include the skill if the planner selected one:

```text
悟空·fixer + systematic-debugging 的產出
```

If no skill was selected, keep the current role-only label.

## Settings Model

Extend `wukong-settings`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlannerPreferences {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub planner_preferences: PlannerPreferences,
}
```

Stored values are lowercase stable identifiers:

- Roles use `Role::name()`: `explorer`, `oracle`, `librarian`, `fixer`, `designer`.
- Skills use `SkillSpec.name`: `systematic-debugging`, `test-driven-development`, etc.

Older settings files load cleanly because the new field has `#[serde(default)]`.

## Web API

Add endpoints:

| Endpoint | Method | Purpose |
| --- | --- | --- |
| `/api/skills/preferences` | GET | Return saved global planner preferences plus validation-normalized values. |
| `/api/skills/preferences` | PUT | Validate and persist global planner preferences. |

Request shape:

```json
{
  "enabled": true,
  "roles": ["fixer", "oracle"],
  "skills": ["systematic-debugging", "verification-before-completion"]
}
```

Response shape:

```json
{
  "enabled": true,
  "roles": ["fixer", "oracle"],
  "skills": ["systematic-debugging", "verification-before-completion"],
  "warnings": []
}
```

Validation rules:

- Unknown roles return HTTP 400.
- Unknown skills return HTTP 400.
- Duplicate roles or skills are removed while preserving first-seen order.
- Empty lists are allowed.
- If `enabled` is false, saved roles and skills may remain stored but should not be injected into planner prompts.

## Planner Integration

Add a small preference type at the orchestration boundary:

```rust
pub struct PlannerPreferenceHint {
    pub preferred_roles: Vec<Role>,
    pub preferred_skills: Vec<String>,
}
```

Extend skill planning prompt construction with an optional hint:

```rust
pub fn skill_planning_prompt_with_preferences(
    task: &str,
    skills: &[SkillRouteOption],
    preferences: Option<&PlannerPreferenceHint>,
) -> String;
```

The existing `skill_planning_prompt(task, skills)` remains and delegates with `None` to preserve compatibility.

When preferences are enabled and at least one role or skill is selected, append:

```text
[User Preferences]
Preferred roles: fixer, oracle
Preferred skills: systematic-debugging, verification-before-completion
These are preferences, not hard constraints. Choose other roles or skills when the task requires it.
```

Runtime should pass preferences only to the planner call. Execution prompts should continue to receive only the actual planned role/skill for each step.

## Runtime And Config Flow

Add optional preferences to `GatewayConfig` or an adjacent turn config field used by `run_turn_observed`:

```rust
pub struct GatewayConfig {
    // existing fields
    pub planner_preferences: Option<PlannerPreferenceHint>,
}
```

Callers that do not load settings keep `None`, preserving existing behavior.

Entry points that already load settings should map `settings.planner_preferences` into the config when `enabled` is true:

- `wukong-cli` one-shot and REPL.
- `wukong-web` turns.
- `wukong-telegram` turns.
- `wukong-scheduler` turns.

Although the UI is Web-only in this phase, persisted global preferences should affect all entry points once loaded from the shared settings file. This avoids confusing behavior where Web says a global preference is enabled but Telegram ignores it.

## Step Trace For Web Baton Labels

Current `run_turn_observed` reports helper steps as `on_step(role, output)`. To display selected skills without breaking existing callers, add a richer observed function and keep the current function as a compatibility wrapper:

```rust
pub struct ObservedStep<'a> {
    pub role: Role,
    pub skill_name: Option<&'a str>,
    pub output: &'a str,
}

pub async fn run_turn_traced(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep<'_>),
) -> Result<TurnOutput, WukongError>;
```

`run_turn_observed` delegates to `run_turn_traced` and drops `skill_name`:

```rust
run_turn_traced(..., &mut |step| on_step(step.role, step.output)).await
```

`wukong-web` switches from `run_turn_observed` to `run_turn_traced` so SSE `step` events include:

```json
{ "role": "fixer", "skill": "systematic-debugging", "html": "..." }
```

Existing history table `turn_steps` does not need a schema change in this phase. Persisted historical helper steps stay role-only. Live SSE cards can show role + skill. A later phase can add persisted skill metadata if it proves useful.

## Frontend Structure

Modify `crates/wukong-web/static/components/wukong-skills.js`:

- Load catalog and preferences in parallel.
- Render checkboxes for roles and skills.
- Submit preferences via `PUT /api/skills/preferences`.
- Keep catalog cards below preference controls.

Modify `crates/wukong-web/static/components/wukong-chat.js`:

- Load `/api/skills/preferences` for toolbar status.
- Render live `step` event summaries with role + skill when present.
- Continue rendering old history-loaded steps role-only.

## Error Handling

- Preference GET failures show a non-blocking status in Skills panel and Chat toolbar.
- Preference PUT validation errors show the server message and do not update local UI as saved.
- Planner preference parsing should ignore empty strings and invalid persisted values defensively. Invalid values should not panic the runtime.
- If preferences are enabled but all selected values are invalid after filtering, runtime behaves as if no preferences are configured.

## Testing Strategy

`wukong-settings`:

- Missing `planner_preferences` defaults cleanly.
- Saves and loads enabled preferences.
- Effective preferences trim, dedupe, and ignore empty strings.

`wukong-orchestrator`:

- `skill_planning_prompt` remains unchanged for callers without preferences.
- `skill_planning_prompt_with_preferences` includes role and skill preference lines.
- Preferences text explicitly says they are not hard constraints.

`wukong-runtime`:

- Planner backend receives preference hint when config includes preferences.
- No preference hint is sent when preferences are disabled or absent.
- `run_turn_observed` behavior remains compatible.
- `run_turn_traced` reports non-final role, skill, and output.

`wukong-web` backend:

- `GET /api/skills/preferences` returns defaults.
- `PUT /api/skills/preferences` persists valid preferences.
- Unknown role returns 400.
- Unknown skill returns 400.
- Web SSE `step` event includes `skill` when planned step has one.

Frontend/manual smoke:

- Skills page loads catalog and saved preferences.
- Saving preferences persists across reload.
- Chat toolbar shows enabled/disabled preference state.
- A multi-step turn with a skill shows role + skill in live baton summary.

## Phased Implementation Plan

Task group 1: preference persistence and Web UI.

- Extend `wukong-settings`.
- Add `/api/skills/preferences`.
- Update `wukong-skills` Web component.

Task group 2: planner prompt guidance.

- Add preference hint type and prompt builder.
- Thread preferences through `GatewayConfig` and settings-loading entry points.
- Add runtime/orchestrator tests.

Task group 3: Web trace display.

- Add `run_turn_traced` compatibility layer.
- Include skill in live Web SSE step event.
- Update baton summary rendering.

## Acceptance Criteria

- Users can configure global preferred roles and Superpowers from Web.
- Preferences persist in the existing settings file and survive page reload.
- Planner prompts include preferences only when enabled.
- Preferences are explicitly guidance, not forced routing.
- Existing callers without preferences behave as before.
- Web live helper baton labels show selected skill names when available.
- All new APIs are protected by the existing Web token model.
