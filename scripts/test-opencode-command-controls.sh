#!/usr/bin/env bash
set -euo pipefail

grep -q 'SessionCommand::Providers' crates/wukong-cli/src/command.rs
grep -q 'SessionCommand::Models' crates/wukong-cli/src/command.rs
grep -q 'SessionCommand::SetModels' crates/wukong-cli/src/command.rs
grep -q 'providers", "list' crates/wukong-cli/src/command.rs
grep -q 'default_model' crates/wukong-settings/src/lib.rs
grep -q 'run_turn_session_passthrough(backend, &id, "/compact"' crates/wukong-cli/src/command.rs
grep -q '/set_models' README.md

echo "opencode command control checks passed"
