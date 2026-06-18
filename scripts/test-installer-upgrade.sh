#!/usr/bin/env bash
set -euo pipefail

output="$(bash scripts/install.sh --upgrade --version v0.14.1 --dry-run)"

if [[ "$output" != *"wukong-docker-v0.14.1.tar.gz"* ]]; then
    echo "FAIL: --upgrade should use the Docker bundle for the requested version" >&2
    exit 1
fi

if [[ "$output" != *"dry-run: 會下載"* ]]; then
    echo "FAIL: --upgrade dry-run should exercise Docker bundle installation" >&2
    exit 1
fi

if ! grep -q "docker compose build --no-cache" scripts/install.sh; then
    echo "FAIL: --upgrade guidance should rebuild without Docker cache" >&2
    exit 1
fi

if ! grep -q "docker compose up -d --force-recreate" scripts/install.sh; then
    echo "FAIL: --upgrade guidance should recreate the running container" >&2
    exit 1
fi

echo "installer upgrade checks passed"
