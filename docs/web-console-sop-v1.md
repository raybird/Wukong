# Web Console SOP v1

**Date:** 2026-06-25  
**Version:** v0.16.14  
**Scope:** SOP for operating the completed Web Console Control Center v1.

## Purpose

This SOP defines the stable Web Console v1 operating flow for Wukong. It covers daily chat usage, read-only memory inspection, recall explainability, skill preferences, schedules, system diagnostics, and global model settings.

Web Console v1 is a control center, not a memory maintenance console. Memory mutation workflows such as consolidate, prune, export, edit, or delete are intentionally deferred.

## Completed Surface

- Chat: daily conversation entry point, shared history, scope-aware turns, intermediate baton cards, and skill metadata.
- Memory: read-only records, summary, recall preview, and recall score explanations.
- Skills: role and Superpowers catalog, global planner preferences, and preference enablement.
- Schedules: existing schedule management entry point.
- System: read-only runtime diagnostics for providers, models, GitHub CLI, environment, schedules, and token/memory status.
- Settings: global model setting and persisted Web/Telegram-related settings.

## Deferred Scope

- Memory consolidate, prune, export, edit, delete, or destructive preview flows.
- Recall scoring controls or per-scope tuning writes.
- Per-scope model overrides.
- Per-role or per-skill model assignment.
- Provider credential management in Web Console.

## Daily Operation Flow

1. Open Web Console and confirm the `Chat` tab loads.
2. Verify the active scope before sending a turn.
3. Send normal prompts through Chat.
4. Expand intermediate baton cards when a turn uses multiple roles or skills.
5. Use `/new`, `/providers`, `/models`, and `/set_models <model>` only when command-style control is needed.
6. If a turn behaves unexpectedly, inspect System diagnostics and recent Memory recall results before changing settings.

## Memory Read Flow

1. Open the `Memory` tab.
2. Check summary cards for total records, scope count, kind distribution, embedding coverage, and maintenance-readiness counts.
3. Filter records by scope or kind when looking for a specific memory family.
4. Use Recall query for read-only recall inspection.
5. Review each recall hit's score explanation: lexical, semantic, decay, importance, recall bonus, age, recall count, and source signals.
6. Do not use Web Console for memory mutation in v1.

## Skill Preference Flow

1. Open the `Skills` tab to inspect available roles and Superpowers.
2. Mark preferred roles and preferred skills as global planner guidance.
3. Keep preferences enabled only when you want the planner prompt to mention them.
4. Treat preferences as guidance, not hard routing constraints.
5. Confirm Chat baton labels when you need to see which role and skill were actually selected.

## Settings Flow

1. Open the `Settings` tab.
2. Review the current global model.
3. Change the global model only when future Web, Telegram, Scheduler, and CLI turns should share the new default.
4. Submit only non-empty model IDs.
5. If an environment-controlled value is shown, treat persisted changes as lower precedence than environment configuration.

## Schedules Flow

1. Open the `Schedules` tab.
2. Review existing scheduled jobs and enabled state.
3. Use existing schedule controls for management.
4. Use System diagnostics to confirm total schedules, enabled schedules, and nearest next run.

## System Diagnostics Flow

1. Open the `System` tab.
2. Confirm summary cards: scope, token, memory DB, schedule counts, and nearest next run.
3. Review diagnostic groups:
   - Runtime
   - Environment
   - Schedules
   - Providers
   - Models
   - Tools
4. Treat `ok` as healthy, `warn` as actionable but non-fatal, and `error` as requiring investigation.
5. Use the refresh button after changing local CLI auth, provider setup, or environment configuration.

## Release Smoke Checklist

Use this checklist before calling Web Console v1 ready in a local environment:

- Chat tab loads and can send a prompt.
- Intermediate baton cards render when a multi-role turn occurs.
- Memory tab loads on empty and populated databases.
- Memory records can be filtered by scope or kind.
- Recall preview returns hits for a meaningful query.
- Recall explanations show score components and source signals.
- Skills preferences can be saved, reloaded, and reflected in Chat context.
- Settings can read and update the global model.
- Schedules tab loads existing schedule state.
- System tab loads grouped diagnostics without failing the whole page when one check warns.
- Web token protection remains active when configured.

## Verification Commands

Run these commands for release-level verification:

```bash
cargo test -p wukong-web
cargo test
cargo clippy --all-targets -- -D warnings
node --check crates/wukong-web/static/components/wukong-chat.js
node --check crates/wukong-web/static/components/wukong-memory.js
node --check crates/wukong-web/static/components/wukong-skills.js
node --check crates/wukong-web/static/components/wukong-settings.js
node --check crates/wukong-web/static/components/wukong-schedules.js
node --check crates/wukong-web/static/components/wukong-system.js
```

## Rollback Notes

- SOP documentation changes are safe to revert independently.
- Web Console runtime behavior is covered by crate-level and workspace-level tests.
- Memory maintenance remains deferred, so rollback does not need to account for Web-originated memory mutations.
