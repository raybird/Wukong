#!/usr/bin/env bash
set -euo pipefail

workflow=".github/workflows/release.yml"

if [[ ! -f "$workflow" ]]; then
  echo "missing workflow: $workflow" >&2
  exit 1
fi

prerelease_count=$(grep -F -c "prerelease: \${{ contains(github.ref_name, '-rc.') }}" "$workflow" || true)
make_latest_count=$(grep -F -c "make_latest: \${{ contains(github.ref_name, '-rc.') && 'false' || 'true' }}" "$workflow" || true)

if [[ "$prerelease_count" != "2" ]]; then
  echo "expected prerelease expression in both release upload steps, found $prerelease_count" >&2
  exit 1
fi

if [[ "$make_latest_count" != "2" ]]; then
  echo "expected make_latest expression in both release upload steps, found $make_latest_count" >&2
  exit 1
fi
