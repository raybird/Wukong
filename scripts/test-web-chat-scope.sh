#!/usr/bin/env bash
set -euo pipefail

chat="crates/wukong-web/static/components/wukong-chat.js"

if ! grep -q "/api/chat/scopes" "$chat"; then
    echo "FAIL: chat component should fetch available chat scopes" >&2
    exit 1
fi

if ! grep -q "selectedScope" "$chat"; then
    echo "FAIL: chat component should track selectedScope" >&2
    exit 1
fi

if ! grep -q "scope=" "$chat"; then
    echo "FAIL: chat API calls should include selected scope" >&2
    exit 1
fi

if ! grep -q "chat-source" "$chat"; then
    echo "FAIL: chat component should render a source selector" >&2
    exit 1
fi

echo "web chat scope checks passed"
