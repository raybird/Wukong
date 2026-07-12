#!/usr/bin/env bash
set -euo pipefail

workflow=".github/workflows/release.yml"

if [[ ! -f "$workflow" ]]; then
  echo "missing workflow: $workflow" >&2
  exit 1
fi

require_text() {
  local text="$1"
  grep -Fq "$text" "$workflow" || { echo "missing workflow contract: $text" >&2; exit 1; }
}

# The release uploads were consolidated into a single `publish` job with one
# upload step (see v0.17.0 "serialize release uploads"), so each RC-gating
# expression must appear exactly once — in that step.
prerelease_count=$(grep -F -c "prerelease: \${{ contains(github.ref_name, '-rc.') }}" "$workflow" || true)
make_latest_count=$(grep -F -c "make_latest: \${{ contains(github.ref_name, '-rc.') && 'false' || 'true' }}" "$workflow" || true)

if [[ "$prerelease_count" != "1" ]]; then
  echo "expected prerelease expression in the single publish upload step, found $prerelease_count" >&2
  exit 1
fi

if [[ "$make_latest_count" != "1" ]]; then
  echo "expected make_latest expression in the single publish upload step, found $make_latest_count" >&2
  exit 1
fi

require_text "validate:"
require_text "fetch-depth: 0"
require_text "git for-each-ref \"refs/tags/\${GITHUB_REF_NAME}\" --format='%(contents)'"
require_text "cargo metadata --locked"
require_text "needs: validate"
require_text "cargo build --release --locked"

echo "release workflow checks passed"
