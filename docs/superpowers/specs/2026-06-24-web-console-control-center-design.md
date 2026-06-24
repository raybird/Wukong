# Web Console Control Center Design

**Date:** 2026-06-24  
**Status:** Draft approved for planning  
**Scope:** Web Console product direction for chat, memory observability, skill preferences, schedules, system diagnostics, and global model settings.

## Context

Wukong has moved beyond a CLI-only assistant. The current Web Console already includes chat, shared history, schedules, settings, system panels, SSE progress, and collapsible intermediate baton output. Memory, skill routing, scheduler, Telegram, and Docker runtime features are also implemented as separate crates and entry points.

The next product step is to turn `wukong-web` from a chat page into a personal AI control center. The console should remain lightweight and zero-build, but it should expose Wukong's core advantages: persistent memory, role/skill orchestration, scheduled turns, runtime diagnostics, and model control.

## Goals

- Make Web Console the primary visual control surface for Wukong.
- Keep Chat as the main daily-use entry point while adding dedicated panels for Memory, Skills, Schedules, System, and Settings.
- Bring memory maturity into the Web Console in phases: observe first, manage second, tune recall third.
- Let users express role and Superpowers preferences without disabling planner autonomy.
- Provide a first-class global model setting in Web Console.
- Reuse existing Rust crates and plain-vanilla Web Component patterns.

## Non-Goals

- No multi-user account system beyond existing Web token protection.
- No per-scope model override in the first version.
- No forced role/skill locking in the first version.
- No replacement for CLI, Telegram, or Scheduler entry points.
- No Node build step, SPA framework, or external frontend dependency.
- No destructive memory operation without preview and confirmation.

## Information Architecture

Use a tabbed Control Center layout:

| Tab | Purpose |
| --- | --- |
| `Chat` | Daily conversation, shared history, active scope, intermediate baton output, current model and skill preference indicators. |
| `Memory` | Memory health, scope/kind distribution, recent and frequently recalled records, maintenance and recall tuning in later phases. |
| `Skills` | Preferred roles and Superpowers for planner guidance. |
| `Schedules` | Existing schedule management and run history. |
| `System` | Read-only runtime diagnostics: providers, models, Agent Reach, GitHub CLI, Docker/workspace state. |
| `Settings` | Writable persistent settings: global model, default scope, Telegram/Web settings, skill preference enablement. |

`System` is read-only observation and diagnostics. `Settings` is writeable configuration.

## Chat Panel

The Chat panel remains the primary workflow.

Enhancements:

- Show selected scope and current global model near the composer.
- Show whether role/skill preferences are active for the selected scope.
- Continue showing collapsible intermediate baton output.
- Extend baton labels to include skills when available, for example `悟空·fixer + test-driven-development`.
- Keep shared chat history behavior: Web can inspect Telegram and project scopes, but Web-originated turns do not push replies to Telegram.

The Chat panel should not become the only place to manage memory or skills. It should show enough context to explain the current turn, then link users to the dedicated panels for deeper control.

## Memory Panel

Memory is implemented in three phases.

### Phase 1: Observability

Purpose: help users understand what Wukong remembers without introducing dangerous operations.

Capabilities:

- Global memory summary: total records, scope count, kind distribution, age distribution, embedding coverage.
- Scope explorer: list `global`, `project:*`, `user:tg-*`, and other scopes with record counts and last update time.
- Recent memories: newest records for selected scope.
- Frequently recalled memories: records sorted by `recall_count` and `last_recalled_at`.
- Low-value candidates: records that look old, rarely recalled, and low importance, shown as candidates only.
- Summary cards for maintenance readiness: consolidate candidate count and prune candidate count.

Initial implementation should prefer existing `Memory::snapshot`-style data and add narrow read APIs only where needed.

### Phase 2: Maintenance

Purpose: make memory upkeep manageable from Web while preserving safety.

Capabilities:

- Trigger `snapshot` from Web.
- Trigger `consolidate --dry-run` for a selected scope.
- Trigger `prune --dry-run` for a selected scope.
- Trigger `export` to configured markdown mirror path when available.
- Confirmed destructive operations require a preview result, explicit confirmation, and clear copy explaining what will change.

Rules:

- `Decision`, `Skill`, and `Summary` records remain protected from prune.
- Web maintenance failures should show structured errors and should not corrupt chat history or active turns.
- Long-running operations should stream or poll progress rather than freezing the page.

### Phase 3: Recall Tuning

Purpose: explain and improve why Wukong recalls certain memories.

Capabilities:

- Recall sandbox: input a query and selected scope, then show hits exactly as Wukong would use them.
- Show score components where available: lexical, semantic, decay, importance, and recall hotness.
- Show mode: keyword, tree, or hybrid.
- Show adaptive-gate decisions for too-short or low-signal queries.
- Provide read-only diagnostics first; any scoring weight controls are a later explicit design.

## Skills Panel

The Skills panel expresses preferences, not hard constraints.

Capabilities:

- Show available roles: Explorer, Oracle, Librarian, Fixer, Designer.
- Show available Superpowers from `wukong-skills` catalog.
- Let users mark preferred roles and preferred skills globally.
- Optionally allow project/scope-specific preferences later, but not in the first implementation.
- Show a short description for each skill and its typical role pairing.
- Show whether skill preferences are enabled.

Runtime behavior:

- Preferences are injected into the planner prompt as guidance.
- The planner may still choose another role or skill if the task calls for it.
- Unknown or removed skills must be ignored safely.
- Preferences must not bypass existing skill routing safeguards.

Example planner hint:

```text
[使用者偏好]
偏好角色: fixer, oracle
偏好技能: systematic-debugging, verification-before-completion
這些是偏好而非強制；若任務需要，仍可選擇其他角色或技能。
```

## Settings Panel

The first version of Settings includes a global model setting.

Capabilities:

- Display the current global default model.
- Display where the setting comes from when known: environment, project settings, or persisted Wukong setting.
- Let users set one global model used by future Web, Telegram, Scheduler, and CLI turns.
- Reuse the existing command-control behavior behind `/set_models <model>` where practical, but expose it as a first-class form.
- Validate empty model values client-side and server-side.

Out of scope for the first version:

- Per-scope model override.
- Per-role model assignment.
- Per-skill model assignment.
- Provider credential entry beyond existing opencode/provider workflows.

## System Panel

System is for diagnostics and status, not persistent writes.

Capabilities:

- Show available providers using the same backend mechanism as `/providers`.
- Show available models using the same backend mechanism as `/models`.
- Show Agent Reach availability and recommended initialization status when possible.
- Show GitHub CLI auth status when possible.
- Show Docker runtime hints: workspace path, data path, and whether the runtime appears containerized.

Failures should be reported as diagnostics, not fatal page errors. For example, if `gh auth status` fails, show that GitHub CLI is not authenticated and suggest the documented setup command.

## Data And API Design

Prefer small, panel-specific APIs under `/api`.

Proposed endpoints:

| Endpoint | Method | Purpose |
| --- | --- | --- |
| `/api/memory/summary` | GET | Memory totals, scope counts, kind counts, age distribution, embedding coverage. |
| `/api/memory/scopes` | GET | Scope list with counts and updated time. |
| `/api/memory/records?scope=&limit=&kind=` | GET | Recent or filtered memory records. |
| `/api/memory/recall-preview` | POST | Phase 3 recall sandbox. |
| `/api/memory/maintenance/preview` | POST | Phase 2 consolidate/prune dry-run. |
| `/api/memory/maintenance/run` | POST | Phase 2 confirmed operation. |
| `/api/skills/catalog` | GET | Roles and Superpowers catalog. |
| `/api/skills/preferences` | GET/PUT | Preferred roles and preferred skills. |
| `/api/settings/model` | GET/PUT | Current and updated global model. |
| `/api/system/providers` | GET | Provider diagnostics. |
| `/api/system/models` | GET | Model diagnostics. |
| `/api/system/reach` | GET | Agent Reach and GitHub CLI diagnostics. |

All endpoints keep existing Web token behavior.

## Persistence

Use existing settings infrastructure where possible.

Suggested settings shape:

```toml
[model]
default = "provider/model"

[planner_preferences]
enabled = true
roles = ["fixer", "oracle"]
skills = ["systematic-debugging", "verification-before-completion"]
```

The exact file and merge precedence should follow `wukong-settings` patterns. If environment variables already force a value, the UI should show that the value is env-controlled and may not be editable.

## Frontend Structure

Continue the existing zero-build Web Component style.

Proposed components:

- `wukong-chat` remains the chat panel.
- `wukong-memory` handles Memory panel.
- `wukong-skills` handles Skills panel.
- `wukong-schedules` remains schedule management.
- `wukong-system` remains or expands system diagnostics.
- `wukong-settings` handles writable settings, including global model.
- `app.js` owns top-level tab selection and shared token/scope wiring.

The layout should remain usable on mobile:

- Top tabs can collapse into a select or horizontal scroll.
- Side/context panels should stack below chat on narrow screens.
- Dangerous Memory operations should use full-width confirmation panels on mobile.

## Error Handling

- Token failures remain HTTP 401.
- Missing or invalid query parameters return structured 400 responses.
- Diagnostics command failures show readable status cards.
- Memory maintenance run failures must not partially update UI as success.
- Unknown skill preference names are ignored and surfaced as warnings.
- Model setting failures show both the attempted model and backend error.

## Testing Strategy

Backend tests:

- Memory summary endpoint returns counts for a temporary database.
- Memory records endpoint respects scope and limit.
- Skill catalog endpoint returns roles and selected Superpowers.
- Skill preferences round-trip through settings.
- Global model setting GET/PUT persists and affects generated turn config.
- System diagnostics endpoints handle command failures without panicking.
- Web token protection applies to all new endpoints.

Frontend/manual smoke tests:

- Tabs switch without reloading the page.
- Chat still sends turns and displays intermediate batons.
- Memory Phase 1 loads with an empty database and a populated database.
- Skills preferences can be selected, saved, reloaded, and shown in Chat.
- Global model can be changed and displayed after reload.
- Mobile layout remains usable.

Regression tests:

- Existing Web chat history APIs continue to work.
- Existing schedule UI and APIs continue to work.
- Existing slash commands `/providers`, `/models`, and `/set_models` remain available.

## Phased Delivery

### Phase 1: Control Center Shell And Read-Only Panels

- Add top-level tab shell.
- Keep Chat unchanged except for current model and preference indicators.
- Add Memory Phase 1 observability.
- Add Skills catalog read view.
- Add Settings global model read/write.
- Expand System diagnostics if needed.

### Phase 2: Skill Preferences Into Planner

- Persist preferred roles and skills.
- Inject preference hint into planner prompt.
- Display selected skill per baton when available.
- Add tests proving preferences guide but do not force planner output.

### Phase 3: Memory Maintenance

- Add dry-run previews for consolidate and prune.
- Add confirmed maintenance operations.
- Add export trigger and status.

### Phase 4: Recall Sandbox

- Add recall preview API and UI.
- Show hit explanations and score components.
- Keep scoring controls read-only unless a later spec explicitly designs tuning writes.

## Implementation Decisions For The First Plan

- Tab state should be stored in the URL hash, for example `#/chat`, `#/memory`, and `#/settings`, so refresh and shared links preserve the selected panel without requiring a router framework.
- `System` and `Settings` should remain separate frontend components and backend modules. They may share small internal helpers, but the user-facing boundary stays read-only diagnostics versus writable configuration.
- Model precedence should be: explicit environment override first, persisted Wukong setting second, built-in/default backend behavior last. If an environment override is active, the Settings UI should display it as env-controlled and avoid implying the persisted value will take effect immediately.
- Phase 1 Memory should add a narrow lightweight records API instead of overloading snapshot data. Snapshot is for aggregate health; record browsing needs pagination, scope filtering, and kind filtering.

## Acceptance Criteria

- Web Console presents a clear tabbed Control Center structure.
- Users can see memory health and scope distribution without using CLI commands.
- Users can see available roles and Superpowers in Web.
- Users can set a single global default model from Web Settings.
- Chat remains the primary workflow and continues to support shared history and intermediate baton display.
- New APIs are protected by the existing Web token model.
- No destructive memory action is available without preview and confirmation.
