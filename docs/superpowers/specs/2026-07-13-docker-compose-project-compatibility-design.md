# Docker Compose Project Compatibility Design

**Date:** 2026-07-13  
**Status:** Approved  
**Scope:** Docker installer install, upgrade, and rollback

## Goal

Preserve the Compose project identity of existing Docker deployments across installer upgrades. Legacy deployments whose project was derived from a directory name, such as `runwukong`, must continue using their existing containers, networks, and named volumes. Fresh installer deployments continue to default to `wukong`.

The installer must never silently switch an existing deployment to another Compose project. Doing so can cause fixed `container_name` conflicts and can attach services to newly-created, empty volumes.

## Decision

The installer will resolve one Docker Compose project name before any Docker mutation and use it for every Compose command in that transaction. A successful transaction persists the resolved value as `composeProject` in `.wukong-release`.

This design does not rename projects or copy volumes. It preserves existing ownership in place.

## Project Resolution

The resolver gathers ownership evidence from:

1. Optional `composeProject` in `.wukong-release`.
2. The `com.docker.compose.project` label on known Wukong containers:
   - `wukong-cli`
   - `wukong-opencode-server`
   - `wukong-telegram`
   - `wukong-web`
   - `wukong-schedulerd`
3. An explicitly supplied `COMPOSE_PROJECT_NAME`, but only when no existing metadata or labeled Wukong container establishes ownership.
4. The default `wukong` for a fresh deployment.

Resolved names must match `[a-z0-9][a-z0-9_-]*`.

The resolver applies these safety rules:

- Multiple distinct project labels are an error.
- A metadata project that disagrees with container labels is an error.
- An explicit project that disagrees with existing ownership evidence is an error.
- Invalid metadata, labels, or explicit names are errors.
- Resolution errors happen before downloading, backing up, replacing files, pulling images, or recreating containers.

Legacy `.wukong-release` files without `composeProject` remain valid. A unique project label becomes their authoritative project and is persisted after the next successful upgrade or rollback transaction.

## Compose Execution

All installer-owned Compose operations use the resolved project explicitly:

```bash
docker compose -p "$DOCKER_PROJECT_NAME" ...
```

This includes:

- Pulling with the verified staged release Compose file.
- Activating services with `up -d --force-recreate`.
- Health/status checks.
- Restoring the prior deployment after activation failure.
- Rollback pull and activation.

The staged pull continues to use `--project-directory "$PWD"` and `-f <staged-compose>`, so it cannot read a stale development Compose file. Project resolution does not change release artifact or image digest verification.

## Metadata

Docker `.wukong-release` remains schema version 1 and gains one field:

```json
{
  "schemaVersion": 1,
  "productTag": "v0.18.0",
  "imageDigest": "sha256:...",
  "composeProject": "runwukong"
}
```

`composeProject` is required for newly-written Docker metadata. Older schema-1 files may omit it and are upgraded in place after a successful transaction.

Rollback metadata rotation preserves the resolved project. A rollback must not infer a new project from the current directory name.

## Failure Behavior

If ownership cannot be resolved unambiguously, the installer stops with a message that reports the conflicting metadata, labels, or explicit project. It must not suggest deleting containers or volumes.

Activation failure keeps the existing transaction behavior: restore release-owned files, recreate the previous services using the same resolved project, and leave user-owned `.env`, overrides, workspace, and named volumes untouched.

No installer path uses `docker compose down -v`, removes named volumes, or migrates volume data.

## Tests

Installer fixtures will cover:

- Fresh Docker install defaults to project `wukong`.
- Explicit project selection works for a fresh install.
- A legacy `runwukong` container label is detected during upgrade.
- A successful legacy upgrade writes `composeProject: runwukong`.
- Subsequent upgrade and rollback use persisted metadata.
- Metadata and container-label disagreement aborts before Docker mutation.
- Multiple labeled projects abort before Docker mutation.
- Explicit project disagreement with legacy ownership aborts.
- Staged release Compose is used for pull.
- Activation recovery uses the same resolved project.
- Existing named volume names are not changed or removed.

The full `scripts/test-installer-upgrade.sh all` suite and shell syntax checks must pass.

## Documentation

`docs/installation.md` and `docs/docker.md` will state that the installer preserves the Compose project of existing deployments. `COMPOSE_PROJECT_NAME` is supported for fresh installs only; it is not a migration switch for an existing deployment.

## Out of Scope

- Renaming a legacy Compose project to `wukong`.
- Copying, renaming, or deleting Docker volumes.
- Removing fixed container names from the release Compose file.
- Changing Binary-mode metadata or behavior.
- Automatically merging deployments when Wukong containers have conflicting project labels.
