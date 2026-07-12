#!/usr/bin/env bash
set -euo pipefail

workflow=.github/workflows/release.yml
[[ -f "$workflow" ]] || { echo "missing workflow: $workflow" >&2; exit 1; }

require_text() { grep -Fq -- "$1" "$workflow" || { echo "missing workflow contract: $1" >&2; exit 1; }; }
for job in validate build-binaries publish-image package-release publish; do require_text "$job:"; done
for contract in \
  'contents: write' 'packages: write' 'cancel-in-progress: false' 'needs: validate' \
  "if: needs.validate.outputs.channel == 'rc'" 'needs: [validate, build-binaries]' \
  'needs: [validate, build-binaries, publish-image]' 'needs: package-release' \
  'cargo build --release --locked' 'musl-binaries' 'REGCTL_VERSION=v0.8.1' 'regctl image digest' 'regctl image copy' \
  'docker buildx build --platform linux/amd64 --push' 'gh release download "$PROMOTE_FROM"' \
  'SHA256SUMS' 'release-manifest.json' 'scripts/generate-sha256sums.sh dist' \
   'name: release-assets' 'files: dist/*' \
   'cp release/package.json release/package-lock.json release-context/release/' 'rehearsal-report:' 'scripts/validate-rehearsal-report.sh' '--data-compatibility release/data-compatibility.json' \
   "printf 'validate: tag=%q\\n' \"\$tag\"" 'validate: annotation=' 'validate: channel=rc'; do
  require_text "$contract"
done

[[ "$(grep -Fc 'uses: softprops/action-gh-release@v2' "$workflow")" == 1 ]] || { echo "expected one release upload" >&2; exit 1; }
[[ "$(grep -Fc 'tar -czf "../../../dist/public/$bin-${{ matrix.target }}.tar.gz"' "$workflow")" == 1 ]] || { echo "expected target-suffixed archives only" >&2; exit 1; }
! grep -Fq 'cp "$bin" ../../../dist/' "$workflow" || { echo "bare binaries must not enter public artifacts" >&2; exit 1; }
! grep -Fq 'releases/latest/download/regctl' "$workflow" || { echo "registry client must use a pinned release" >&2; exit 1; }
! grep -Fq 'checksums-${{ matrix.target }}.txt' "$workflow" || { echo "legacy per-target checksum assets must not be published" >&2; exit 1; }
! grep -Fq "'checksums-*.txt'" "$workflow" || { echo "stable promotion must consume global SHA256SUMS only" >&2; exit 1; }

echo "release workflow checks passed"
