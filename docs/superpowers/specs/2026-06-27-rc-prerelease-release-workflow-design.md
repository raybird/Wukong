# RC prerelease release workflow design

## Goal

Release candidate tags should be safe to publish for testing without being treated as the stable latest release.

## Scope

This change only affects GitHub release metadata for tags that contain `-rc.` in the tag name. It does not change Rust builds, artifact names, checksum generation, or Docker bundle packaging.

## Behavior

- `v0.16.15-rc.1` creates or updates a GitHub prerelease.
- `v0.16.15-rc.1` is not marked as the latest release.
- `v0.16.15` remains a normal release and can become latest.
- Non-rc prerelease labels such as `alpha` or `beta` are out of scope for this change.

## Implementation Shape

Both `softprops/action-gh-release` upload steps in `.github/workflows/release.yml` should use the same tag-name expression:

- `prerelease` is true when `github.ref_name` contains `-rc.`.
- `make_latest` is false when `github.ref_name` contains `-rc.` and true otherwise.

Keeping this logic directly on the upload steps avoids introducing a separate metadata job for a single release rule.

## Testing

- Add or run a static check that both release upload steps include matching `prerelease` and `make_latest` settings.
- Validate the real workflow by pushing an rc tag such as `v0.16.15-rc.1` and confirming the created release is a prerelease with all expected assets.
