#!/usr/bin/env bash
# Re-pin the release image's opencode-ai version.
#
# The release image is built from exact pins (version + npm integrity + a
# lockfile consumed by `npm ci`), and scripts/test-release-image.sh enforces
# that. "Ship the newest opencode" therefore does not mean unpinning: it means
# resolving `latest` to a concrete version and rewriting all three pinned files
# together, so the build stays reproducible and the manifest stays truthful.
#
# Release CI runs this before building the image, and again before generating
# the manifest (with the already-resolved version pinned) so both describe the
# same opencode.
#
# Escape hatch: set OPENCODE_VERSION_PIN=X.Y.Z to hold a known-good release
# instead of taking the newest — the release workflow passes the
# OPENCODE_VERSION_PIN repository variable straight through.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
runtime_inputs="$root/release/runtime-inputs.env"
package_json="$root/release/package.json"

fail() {
    printf 'resolve-opencode-version: %s\n' "$*" >&2
    exit 1
}

command -v npm >/dev/null || fail "npm is required"
[[ -f "$runtime_inputs" ]] || fail "missing $runtime_inputs"
[[ -f "$package_json" ]] || fail "missing $package_json"

previous="$(sed -n 's/^OPENCODE_VERSION=//p' "$runtime_inputs")"

pin="${OPENCODE_VERSION_PIN:-}"
if [[ -n "$pin" ]]; then
    version="$pin"
    printf 'resolve-opencode-version: honouring pin %s\n' "$version"
else
    version="$(npm view opencode-ai version)"
    printf 'resolve-opencode-version: npm latest is %s\n' "$version"
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "resolved version '$version' is not exact"

integrity="$(npm view "opencode-ai@$version" dist.integrity)"
[[ "$integrity" =~ ^sha512- ]] || fail "resolved integrity '$integrity' is not an npm sha512 value"

# runtime-inputs.env drives both the image build and the release manifest.
python3 - "$runtime_inputs" "$version" "$integrity" <<'PY'
import pathlib, sys

path, version, integrity = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
replacements = {"OPENCODE_VERSION": version, "OPENCODE_INTEGRITY": integrity}
lines, seen = [], set()
for line in path.read_text().splitlines():
    key = line.split("=", 1)[0]
    if key in replacements:
        lines.append(f"{key}={replacements[key]}")
        seen.add(key)
    else:
        lines.append(line)
missing = set(replacements) - seen
if missing:
    raise SystemExit(f"runtime inputs missing keys: {sorted(missing)}")
path.write_text("\n".join(lines) + "\n")
PY

python3 - "$package_json" "$version" <<'PY'
import json, pathlib, sys

path, version = pathlib.Path(sys.argv[1]), sys.argv[2]
data = json.loads(path.read_text())
data["dependencies"]["opencode-ai"] = version
path.write_text(json.dumps(data, indent=2) + "\n")
PY

# `npm ci` in Dockerfile.release refuses to run when the lockfile disagrees with
# package.json, so the lockfile has to be regenerated in the same breath.
(cd "$root/release" && npm install --package-lock-only --omit=dev >/dev/null)

# Self-check with the same contract the release tests enforce. Invoked through
# bash because this script is tracked without the executable bit.
bash "$root/scripts/test-release-image.sh" pins >/dev/null \
    || fail "regenerated pins do not satisfy the release image contract"

printf 'resolve-opencode-version: opencode-ai %s -> %s\n' "${previous:-unknown}" "$version"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf 'version=%s\nintegrity=%s\n' "$version" "$integrity" >> "$GITHUB_OUTPUT"
fi
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf 'opencode-ai pinned at `%s` (was `%s`)\n' "$version" "${previous:-unknown}" \
        >> "$GITHUB_STEP_SUMMARY"
fi
