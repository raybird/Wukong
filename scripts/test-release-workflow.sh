#!/usr/bin/env bash
set -euo pipefail

workflow=.github/workflows/release.yml
[[ -f "$workflow" ]] || { echo "missing workflow: $workflow" >&2; exit 1; }

require_text() { grep -Fq -- "$1" "$workflow" || { echo "missing workflow contract: $1" >&2; exit 1; }; }
for job in validate build-binaries publish-image package-release publish; do require_text "$job:"; done
for contract in \
  'contents: write' 'packages: write' 'cancel-in-progress: false' 'needs: validate' \
  'needs: [validate, build-binaries]' \
  'needs: [validate, build-binaries, publish-image]' 'needs: package-release' \
  'cargo build --release --locked' 'musl-binaries' 'REGCTL_VERSION=v0.8.1' 'regctl image digest' 'regctl image copy' \
  'docker buildx build --platform linux/amd64 --push' 'regctl image copy "$image@$image_digest" "$image:latest"' 'git show -s --format=%cI "$COMMIT"' \
  'release-manifest.json' 'scripts/generate-sha256sums.sh dist' \
   'name: release-assets' 'files: dist/*' \
   'cp release/package.json release/package-lock.json release-context/release/' '--data-compatibility release/data-compatibility.json' \
   'run: scripts/resolve-opencode-version.sh' \
   'opencode_version: ${{ steps.opencode.outputs.version }}' \
   'OPENCODE_VERSION_PIN: ${{ vars.OPENCODE_VERSION_PIN }}' \
   'OPENCODE_VERSION_PIN: ${{ needs.publish-image.outputs.opencode_version }}' \
   "printf 'validate: tag=%q\\n' \"\$tag\"" 'validate: annotation=' 'validate: channel=rc' \
   'refs/tags/release-source/$tag' 'uses: docker/login-action@v3'; do
  require_text "$contract"
done

for forbidden in 'rehearsal-report:' 'scripts/validate-rehearsal-report.sh' 'promote-from:' 'gh release download "$PROMOTE_FROM"'; do
  ! grep -Fq -- "$forbidden" "$workflow" || { echo "obsolete workflow contract remains: $forbidden" >&2; exit 1; }
done

[[ "$(grep -Fc 'uses: softprops/action-gh-release@v2' "$workflow")" == 1 ]] || { echo "expected one release upload" >&2; exit 1; }
[[ "$(grep -Fc 'tar -czf "../../../dist/public/$bin-${{ matrix.target }}.tar.gz"' "$workflow")" == 1 ]] || { echo "expected target-suffixed archives only" >&2; exit 1; }
! grep -Fq 'cp "$bin" ../../../dist/' "$workflow" || { echo "bare binaries must not enter public artifacts" >&2; exit 1; }
! grep -Fq 'releases/latest/download/regctl' "$workflow" || { echo "registry client must use a pinned release" >&2; exit 1; }
! grep -Fq 'checksums-${{ matrix.target }}.txt' "$workflow" || { echo "legacy per-target checksum assets must not be published" >&2; exit 1; }
! grep -Fq "'checksums-*.txt'" "$workflow" || { echo "stable promotion must consume global SHA256SUMS only" >&2; exit 1; }
! grep -Fq 'attach_immutable latest' "$workflow" || { echo "latest must remain a mutable stable pointer" >&2; exit 1; }

# ── The bundle and the installer are two curated lists that must agree ──
# v0.21.0 shipped `docker-compose.memoria.yml` and `docker/memoria-runtime/` inside
# the bundle but never added them to install.sh's DOCKER_RELEASE_OWNED, so the
# installer wrote neither to disk. The shipped `.env.example` still told users to
# set COMPOSE_FILE to that overlay — and COMPOSE_FILE applies to every compose
# invocation, so following the documented instructions broke the entire deployment,
# not just the optional feature.
#
# The same shape as the v0.20.0 incident (a file silently absent from a curated
# list), one list further down the pipeline. Anything release.yml puts in the
# bundle must be either installed or explicitly declared as not-installed.
installer="$(dirname "${BASH_SOURCE[0]}")/install.sh"

# Deliberately shipped but never written into the deployment: consumed from the
# release directory during install, not needed at runtime.
bundle_not_installed=(data-compatibility.json release-manifest.json)

bundle_files="$(sed -n '/mkdir -p dist\/wukong-docker/,/tar -C dist/p' "$workflow" |
  sed -n 's|.*dist/wukong-docker/\([A-Za-z0-9._/-]*\).*|\1|p;s|^ *cp \([A-Za-z0-9._/-]*\) dist/wukong-docker/$|\1|p' |
  sed 's|.*/||' | sort -u | grep -v '^$')"
[[ -n "$bundle_files" ]] || { echo "could not read the bundle assembly block from $workflow" >&2; exit 1; }

for f in $bundle_files; do
  grep -Fq "$f" "$installer" && continue
  printf '%s\n' "${bundle_not_installed[@]}" | grep -Fqx "$f" && continue
  echo "bundle ships '$f' but install.sh neither installs it nor declares it excluded" >&2
  echo "add it to DOCKER_RELEASE_OWNED, or to bundle_not_installed in this test" >&2
  exit 1
done

# The overlay specifically: named in the shipped .env.example, so it must install.
for owned in docker-compose.memoria.yml docker/memoria-runtime/Dockerfile \
             docker/memoria-runtime/publish.sh docker/memoria-runtime/memoria-wrapper.sh \
             docker/memoria-runtime/memoria-vector-sync.sh; do
  grep -Fq "    $owned" "$installer" ||
    { echo "install.sh must own the Memoria overlay file: $owned" >&2; exit 1; }
done

echo "release workflow checks passed"
