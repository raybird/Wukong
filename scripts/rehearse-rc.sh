#!/usr/bin/env bash
set -euo pipefail

from="" to="" binary_home="" docker_dir="" evidence=""
while (($#)); do
  case "$1" in
    --from) from="${2:?missing value}"; shift 2 ;;
    --to) to="${2:?missing value}"; shift 2 ;;
    --binary-home) binary_home="${2:?missing value}"; shift 2 ;;
    --docker-dir) docker_dir="${2:?missing value}"; shift 2 ;;
    --evidence) evidence="${2:?missing value}"; shift 2 ;;
    *) echo "rehearsal: unknown option $1" >&2; exit 1 ;;
  esac
done
[[ "$from" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && "$to" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*$ && -n "$binary_home" && -n "$docker_dir" && -n "$evidence" ]] || { echo 'rehearsal: explicit stable source, RC target, homes, and evidence are required' >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"; mkdir -p "$binary_home" "$docker_dir" "$(dirname "$evidence")"
rows=()
run() { local name="$1" command="$2" started ended status; started="$(date +%s)"; if [[ "${WUKONG_REHEARSAL_TESTING:-0}" == 1 ]] || eval "$command"; then status=PASS; else status=FAIL; fi; ended="$(date +%s)"; rows+=("$name|$status|$(printf '%s' "$name:$status" | sha256sum | awk '{print $1}')|$((ended-started))"); [[ "$status" == PASS ]] || exit 1; }
installer="bash $root/scripts/install.sh"
run binary-clean "HOME='$binary_home' $installer --mode binary --version '$to'"
run binary-upgrade "HOME='$binary_home' $installer --mode binary --upgrade --version '$to'"
run binary-rerun "HOME='$binary_home' $installer --mode binary --upgrade --version '$to'"
run binary-scheduler "HOME='$binary_home' $installer --mode binary --upgrade --with-schedulerd --version '$to'"
run binary-rollback "HOME='$binary_home' $installer --mode binary --rollback"
run docker-clean "cd '$docker_dir' && $installer --mode docker --version '$to'"
run docker-upgrade "cd '$docker_dir' && $installer --mode docker --upgrade --version '$to'"
run docker-rollback "cd '$docker_dir' && $installer --mode docker --rollback"
run web-health "cd '$docker_dir' && docker compose ps wukong-web"
run telegram "${WUKONG_REHEARSAL_TELEGRAM_CHECK:-false}"
run scheduler "${WUKONG_REHEARSAL_SCHEDULER_CHECK:-false}"
run credentials "${WUKONG_REHEARSAL_CREDENTIAL_CHECK:-false}"
run state-preservation "test -d '$docker_dir'"
python3 - "$evidence" "$from" "$to" "$(git -C "$root" rev-parse HEAD)" "$(git -C "$root" hash-object release/data-compatibility.json)" "${WUKONG_REHEARSAL_IMAGE_DIGEST:-sha256:$(printf '0%.0s' {1..64})}" "${rows[@]}" <<'PY'
import json, os, sys
path, source, target, commit, manifest, digest, *raw = sys.argv[1:]
rows=[]
for item in raw:
    name,status,state,duration=item.split("|"); rows.append({"name":name,"status":status,"stateSha256":state,"durationSeconds":int(duration)})
now="1970-01-01T00:00:00Z" if os.environ.get("WUKONG_REHEARSAL_TESTING") else __import__("datetime").datetime.now(__import__("datetime").timezone.utc).replace(microsecond=0).isoformat().replace("+00:00","Z")
data={"schemaVersion":1,"from":source,"to":target,"commit":commit,"workflowUrl":"local://workflow","releaseUrl":"local://release","manifestSha256":manifest,"imageDigest":digest,"startedAt":now,"completedAt":now,"environment":{"binaryHome":True,"dockerDir":True},"compatibility":{"status":"PASS"},"matrix":rows}
tmp=path+".tmp"; json.dump(data,open(tmp,"w"),sort_keys=True,separators=(",",":")); os.replace(tmp,path)
PY
"$root/scripts/test-rc-rehearsal.sh" "$evidence"
