#!/usr/bin/env bash
set -euo pipefail

report="${1:?usage: validate-rehearsal-report.sh REPORT RC_TAG COMMIT IMAGE_DIGEST}"
rc_tag="${2:?missing RC tag}"
commit="${3:?missing commit}"
digest="${4:?missing image digest}"

python3 - "$report" "$rc_tag" "$commit" "$digest" <<'PY'
import json, re, sys
path, rc_tag, commit, digest = sys.argv[1:]
try:
    data = json.load(open(path, encoding="utf-8"))
    assert data["schemaVersion"] == 1
    assert data["to"] == rc_tag and data["commit"] == commit and data["imageDigest"] == digest
    assert data["compatibility"]["status"] == "PASS"
    required = {"binary-clean", "binary-upgrade", "binary-rerun", "binary-scheduler", "binary-rollback", "docker-clean", "docker-upgrade", "docker-rollback", "web-health", "telegram", "scheduler", "credentials", "state-preservation"}
    rows = {item["name"]: item for item in data["matrix"]}
    assert required <= rows.keys()
    for name in required:
        assert rows[name]["status"] == "PASS"
        assert rows[name].get("stateSha256")
    for name in ("binary-rollback", "docker-rollback"):
        assert isinstance(rows[name].get("durationSeconds"), (int, float))
except (AssertionError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
    raise SystemExit("invalid rehearsal evidence: " + str(error))
PY
