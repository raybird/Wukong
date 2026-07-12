#!/usr/bin/env bash
set -euo pipefail

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

runtime_inputs='{"AGENT_REACH_ARCHIVE_SHA256":"0123456789012345678901234567890123456789012345678901234567890123","AGENT_REACH_REF":"0123456789012345678901234567890123456789","BASE_IMAGE":"debian:bookworm-slim@sha256:0123456789012345678901234567890123456789012345678901234567890123","DEBIAN_SNAPSHOT":"20260712T000000Z","OPENCODE_INTEGRITY":"sha512-example","OPENCODE_VERSION":"1.2.3"}'
scripts/generate-release-manifest.sh --tag v0.18.0-rc.1 --commit 0123456789012345678901234567890123456789 --channel rc --image-reference ghcr.io/raybird/wukong:v0.18.0-rc.1 --image-digest sha256:0123456789012345678901234567890123456789012345678901234567890123 --platform linux/amd64 --runtime-inputs "$runtime_inputs" --output "$tmp/release-manifest.json"
python3 -c 'import json,sys; data=json.load(open(sys.argv[1])); assert data["productTag"] == "v0.18.0-rc.1"; assert data["promotedFrom"] is None; assert data["runtimeInputs"]["BASE_IMAGE"].startswith("debian:")' "$tmp/release-manifest.json"
if scripts/generate-release-manifest.sh --tag v0.18.0 --commit 0123456789012345678901234567890123456789 --channel stable --image-reference ghcr.io/raybird/wukong:v0.18.0 --image-digest sha256:0123456789012345678901234567890123456789012345678901234567890123 --platform linux/amd64 --runtime-inputs '{}' --output "$tmp/invalid.json"; then
    echo "manifest accepted incomplete runtime inputs" >&2
    exit 1
fi

printf 'one\n' > "$tmp/one"
printf 'two\n' > "$tmp/two"
scripts/generate-sha256sums.sh "$tmp"
grep -Fq 'one' "$tmp/SHA256SUMS"
grep -Fq 'two' "$tmp/SHA256SUMS"
! grep -Fq 'SHA256SUMS' "$tmp/SHA256SUMS"

echo "release manifest checks passed"
