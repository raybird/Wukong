#!/usr/bin/env bash
set -euo pipefail

if [[ $# == 0 ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  WUKONG_REHEARSAL_TESTING=1 scripts/rehearse-rc.sh --from v0.17.1 --to v0.18.0-rc.1 --binary-home "$tmp/binary" --docker-dir "$tmp/docker" --evidence "$tmp/evidence.json" >/dev/null
  report="$tmp/evidence.json"
else
  report="$1"
fi

python3 - "$report" <<'PY'
import json, re, sys
path = sys.argv[1]
try:
    report = json.load(open(path, encoding="utf-8"))
    assert report["schemaVersion"] == 1
    for key in ("from", "to", "commit", "workflowUrl", "releaseUrl", "manifestSha256", "imageDigest", "startedAt", "completedAt", "environment", "compatibility"):
        assert report[key] is not None
    assert re.fullmatch(r"v\d+\.\d+\.\d+(?:-rc\.\d+)?", report["from"])
    assert re.fullmatch(r"v\d+\.\d+\.\d+-rc\.\d+", report["to"])
    assert re.fullmatch(r"[0-9a-f]{40}", report["commit"])
    assert re.fullmatch(r"sha256:[0-9a-f]{64}", report["imageDigest"])
    assert report["compatibility"]["status"] == "PASS"
    rows = report["matrix"]
    required = {"binary-clean", "binary-upgrade", "binary-rerun", "binary-scheduler", "binary-rollback", "docker-clean", "docker-upgrade", "docker-rollback", "web-health", "telegram", "scheduler", "credentials", "state-preservation"}
    seen = {row["name"] for row in rows}
    assert required <= seen
    for row in rows:
        assert row["status"] == "PASS"
        assert row["stateSha256"]
        if "rollback" in row["name"]: assert isinstance(row["durationSeconds"], (int, float))
except (AssertionError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
    raise SystemExit("invalid rehearsal evidence: " + str(error))
PY

echo "RC rehearsal evidence checks passed"
