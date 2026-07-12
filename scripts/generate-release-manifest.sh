#!/usr/bin/env bash
set -euo pipefail

declare -A values=()
while (($#)); do
  case "$1" in
    --tag|--commit|--channel|--promoted-from|--image-reference|--image-digest|--platform|--runtime-inputs|--output)
      values["${1#--}"]="${2:?missing value}"; shift 2 ;;
    *) printf 'manifest: unknown option %s\n' "$1" >&2; exit 1 ;;
  esac
done

python3 - "${values[tag]:?missing --tag}" "${values[commit]:?missing --commit}" "${values[channel]:?missing --channel}" "${values[promoted-from]:-}" "${values[image-reference]:?missing --image-reference}" "${values[image-digest]:?missing --image-digest}" "${values[platform]:?missing --platform}" "${values[runtime-inputs]:?missing --runtime-inputs}" "${values[output]:?missing --output}" <<'PY'
import json, os, re, sys
tag, commit, channel, promoted, image_ref, digest, platform, runtime, output = sys.argv[1:]
if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-rc\.[1-9]\d*)?", tag): raise SystemExit("manifest: invalid tag")
if not re.fullmatch(r"[0-9a-f]{40}", commit): raise SystemExit("manifest: invalid commit")
if channel not in ("rc", "stable"): raise SystemExit("manifest: invalid channel")
if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest): raise SystemExit("manifest: invalid image digest")
runtime_inputs = json.loads(runtime)
required_runtime_inputs = {"BASE_IMAGE", "DEBIAN_SNAPSHOT", "OPENCODE_VERSION", "OPENCODE_INTEGRITY", "AGENT_REACH_REF", "AGENT_REACH_ARCHIVE_SHA256"}
if set(runtime_inputs) != required_runtime_inputs: raise SystemExit("manifest: incomplete runtime inputs")
if not re.fullmatch(r"debian:bookworm-slim@sha256:[0-9a-f]{64}", runtime_inputs["BASE_IMAGE"]): raise SystemExit("manifest: invalid base image")
if not re.fullmatch(r"[0-9]{8}T[0-9]{6}Z", runtime_inputs["DEBIAN_SNAPSHOT"]): raise SystemExit("manifest: invalid Debian snapshot")
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", runtime_inputs["OPENCODE_VERSION"]): raise SystemExit("manifest: invalid OpenCode version")
if not runtime_inputs["OPENCODE_INTEGRITY"].startswith("sha512-"): raise SystemExit("manifest: invalid OpenCode integrity")
if not re.fullmatch(r"[0-9a-f]{40}", runtime_inputs["AGENT_REACH_REF"]): raise SystemExit("manifest: invalid Agent Reach ref")
if not re.fullmatch(r"[0-9a-f]{64}", runtime_inputs["AGENT_REACH_ARCHIVE_SHA256"]): raise SystemExit("manifest: invalid Agent Reach checksum")
if channel == "rc" and promoted: raise SystemExit("manifest: RC cannot be promoted")
if channel == "stable" and not re.fullmatch(r"v\d+\.\d+\.\d+-rc\.[1-9]\d*", promoted): raise SystemExit("manifest: stable requires promoted RC")
data = {"schemaVersion": 1, "productTag": tag, "commit": commit, "channel": channel, "promotedFrom": promoted or None, "image": {"reference": image_ref, "digest": digest, "platform": platform, "buildOriginTag": promoted or tag}, "binaryTargets": ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"], "runtimeInputs": runtime_inputs}
temp = output + ".tmp"
with open(temp, "w", encoding="utf-8") as handle: json.dump(data, handle, sort_keys=True, separators=(",", ":")); handle.write("\n")
os.replace(temp, output)
PY
