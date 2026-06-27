# Runtime Skill Assets Design

## Goal

Make Wukong reliably expose Superpowers skill files to the underlying opencode
agent in both Docker and binary-installed runtimes, even when the active working
directory is not the Wukong repository root.

The release should carry or materialize the same skill assets that are compiled
into `wukong-skills`, seed them into the active workspace, and point skill
prompts at a stable runtime path.

## Context

Wukong's skill routing currently uses `wukong-skills` as the canonical local
skill catalog. The crate embeds selected Superpowers files with `include_str!`
from:

```text
crates/wukong-skills/assets/superpowers/
```

However, the execution prompt currently asks opencode to read a source-relative
path such as:

```text
crates/wukong-skills/assets/superpowers/brainstorming/SKILL.md
```

That path only works when opencode's current working directory is the Wukong repo
root. In Docker mode, compose mounts `WUKONG_HOST_WORKSPACE` to `/workspace`, and
that workspace may be a general runtime directory containing many projects, not
the Wukong source tree. In binary-installed mode, users usually run `wukong` from
their own project directories without any Wukong source tree nearby. In both
cases opencode sees the skill instruction but cannot read the requested file.

## Requirements

- Docker images built from a release must include the selected Superpowers skill
  assets.
- Binary-installed runtimes must materialize the embedded selected Superpowers
  skill assets into the active workspace before executing skill-backed turns.
- Runtime services must expose those assets at a stable path under the active
  workspace so opencode can read them with normal file tools.
- The source of truth remains `crates/wukong-skills/assets/superpowers/`.
- `scripts/sync-superpowers.sh` continues updating the canonical source tree and
  `SOURCE.md`; Docker runtime syncing is downstream of that source.
- The runtime sync must work for CLI, Web, Telegram, and Scheduler services.
- Startup must avoid unnecessary overwrites when the workspace copy is already
  current.
- Skill asset resolution must use `WUKONG_WORKSPACE` when set; otherwise binary
  mode must use the current working directory.
- Prompt tests must prevent regressions back to source-relative `crates/...`
  paths.

## Recommended Approach

Use a shared runtime asset mirror:

1. Docker build copies canonical assets into the image.
2. Entrypoint seeds or refreshes a workspace copy from the image.
3. Binary-installed runtimes materialize embedded `wukong-skills` content into
   the active workspace before executing skill-backed turns.
4. Skill prompts point opencode to the resolved workspace copy.

### Canonical Source

Keep the existing source tree as the only editable asset source:

```text
crates/wukong-skills/assets/superpowers/
```

The existing sync script remains responsible for updating this directory from
upstream Superpowers and recording provenance in `SOURCE.md`.

### Image Asset Path

During Docker build, copy the canonical assets to:

```text
/usr/local/share/wukong/skills/superpowers/
```

This makes the selected skills part of the release artifact without requiring the
full repository to exist in the runtime workspace.

### Workspace Asset Path

During entrypoint startup, mirror the image assets into:

```text
<workspace>/.wukong/skills/superpowers/
```

In Docker, `<workspace>` is normally `/workspace`. In binary mode, `<workspace>`
is resolved as follows:

- If `WUKONG_WORKSPACE` is set and non-empty, use that path.
- Otherwise use `std::env::current_dir()`.

This path is intentionally workspace-local because opencode is authorized to
read and work inside the active project. It also keeps runtime-visible support
files separate from user project files under `.wukong/`.

### Binary Materialization

Binary-installed runtimes do not have the Docker image asset directory or
entrypoint. Before a skill-backed turn is executed, Wukong should materialize the
embedded catalog into the resolved workspace path:

```text
<workspace>/.wukong/skills/superpowers/{skill}/SKILL.md
<workspace>/.wukong/skills/superpowers/SOURCE.md
```

The materializer should write each `SkillSpec.content` from `wukong_skills::all()`
to its matching `SKILL.md`. `SOURCE.md` should come from the embedded canonical
asset if available, or from a generated runtime attribution string if the source
file is unavailable.

### Sync Policy

Docker entrypoint sync should compare the image copy and workspace copy using
`SOURCE.md`:

- If `/workspace/.wukong/skills/superpowers/SOURCE.md` is missing, initialize the
  workspace copy.
- If image `SOURCE.md` differs from workspace `SOURCE.md`, refresh the workspace
  copy.
- If the files match, leave the workspace copy untouched.

The refresh can replace the `superpowers` directory as a unit. This keeps the
implementation simple and ensures removed upstream files do not linger.

Binary materialization can use the same policy: if workspace `SOURCE.md` matches
the embedded source attribution, leave the workspace copy untouched; otherwise
refresh the directory as a unit.

### Prompt Path

Change the skill prompt path from the source-relative path to the resolved
runtime workspace path:

```text
<workspace>/.wukong/skills/superpowers/{skill}/SKILL.md
```

The prompt should continue instructing opencode to read the file first and follow
the skill process. Using an absolute path avoids ambiguity if opencode's current
working directory is a nested project.

## Alternatives Considered

### Mount the Wukong Repository as `/workspace`

This works only when the user is developing Wukong itself. It fails for the
normal runtime case where `/workspace` is a general working directory containing
multiple projects.

Rejected.

### Point Prompts at `/usr/local/share/wukong/skills`

This avoids copying into `/workspace`, but the opencode runtime guidance and
permissions focus on workspace-local files. Keeping readable support assets under
`/workspace/.wukong` is more predictable for the agent and easier to inspect from
the host.

Rejected for the first implementation.

### Inject Full Skill Content Directly Into the Prompt

This would eliminate file reads, but it increases prompt size and bypasses the
pull-on-demand skill loading behavior. The desired behavior is to keep skills as
runtime-readable files while making their location reliable.

Rejected for this requirement.

### Store Binary Skills Under the User Home Directory

This avoids writing `.wukong/` under each project, but it makes the prompt point
outside the active workspace and is less visible to opencode and the user.

Rejected. Workspace-local files are easier to inspect and match Docker behavior.

## Error Handling

- If the image asset directory is missing, entrypoint should continue startup but
  print a clear warning.
- If `/workspace` is unavailable or unwritable, entrypoint should continue with a
  warning because services may still need to start for diagnostics.
- If sync fails after partially preparing a temporary directory, entrypoint should
  leave the existing workspace copy intact when possible.
- If binary materialization fails, Wukong should surface a clear error before
  calling opencode, because sending a prompt that points at a missing skill file
  recreates the original failure mode.
- The prompt should not mention source-relative fallback paths, because that
  would recreate the current failure mode.

## Testing

Implementation should verify:

- Docker image contains
  `/usr/local/share/wukong/skills/superpowers/brainstorming/SKILL.md`.
- `docker compose run --rm wukong` seeds
  `/workspace/.wukong/skills/superpowers/brainstorming/SKILL.md`.
- Binary mode without `WUKONG_WORKSPACE` materializes
  `<current-dir>/.wukong/skills/superpowers/brainstorming/SKILL.md`.
- Binary mode with `WUKONG_WORKSPACE` materializes
  `$WUKONG_WORKSPACE/.wukong/skills/superpowers/brainstorming/SKILL.md`.
- Entrypoint does not rewrite the workspace copy when `SOURCE.md` matches.
- Entrypoint refreshes the workspace copy when `SOURCE.md` differs.
- Runtime prompt construction points at the resolved absolute workspace skill
  path and does not hard-code `/workspace` or Docker-specific wording.
- Existing `wukong-skills` catalog tests still confirm embedded content is
  present.
- `scripts/sync-superpowers.sh --dry-run` and normal sync behavior remain scoped
  to `crates/wukong-skills/assets/superpowers/`.

## Scope Boundaries

This design does not add dynamic skill installation from inside Docker or binary
runtime.
It also does not change the selected skill catalog, the upstream Superpowers sync
policy, or the skill planner's routing behavior.
