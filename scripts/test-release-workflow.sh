#!/usr/bin/env bash
set -euo pipefail

workflow=".github/workflows/release.yml"

if [[ ! -f "$workflow" ]]; then
  echo "missing workflow: $workflow" >&2
  exit 1
fi

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

echo "release workflow checks passed"
