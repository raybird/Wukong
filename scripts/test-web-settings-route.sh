#!/usr/bin/env bash
set -euo pipefail

app="crates/wukong-web/static/app.js"

if ! grep -q "import { html, unsafe }" "$app"; then
    echo "FAIL: settings route shell should import unsafe() for trusted custom element tags" >&2
    exit 1
fi

for tag in wukong-settings wukong-system wukong-schedules; do
    if ! grep -q "unsafe('<${tag}></${tag}>')" "$app"; then
        echo "FAIL: ${tag} should be inserted as trusted HTML, not escaped text" >&2
        exit 1
    fi
done

echo "web settings route checks passed"
