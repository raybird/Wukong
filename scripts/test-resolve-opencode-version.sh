#!/usr/bin/env bash
# issue-0044 Task 4.1（SCN-012）：release 映像的 OpenCode 固定在 release/ 釘選的 V2 版本。
#
# 在暫存目錄複製 release/ 與相關腳本執行 resolve-opencode-version.sh，npm 換成假的：
# `npm view @opencode/cli version` 回報比釘選更新的版本，確認解析結果不跟著跑；指定 V1 版本
# 時拒絕（Wukong 只支援 V2）。不連 npm registry。
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { printf 'resolve-opencode-version test: %s\n' "$*" >&2; exit 1; }

mkdir -p "$tmp/repo/scripts" "$tmp/bin"
cp -R "$root/release" "$tmp/repo/"
cp "$root/scripts/resolve-opencode-version.sh" "$root/scripts/test-release-image.sh" "$tmp/repo/scripts/"
pinned="$(sed -n 's/^OPENCODE_VERSION=//p' "$root/release/runtime-inputs.env")"
integrity="$(sed -n 's/^OPENCODE_INTEGRITY=//p' "$root/release/runtime-inputs.env")"

# 假 npm：latest 是 2.9.9；只認得釘選版本的 integrity；--package-lock-only 什麼都不做
# （lockfile 已是釘選版本的內容）。每次呼叫記到 calls。
cat > "$tmp/bin/npm" <<EOF
#!/bin/sh
echo "\$*" >> "$tmp/calls"
case "\$*" in
    "view @opencode/cli version") echo 2.9.9 ;;
    "view @opencode/cli@$pinned dist.integrity") echo "$integrity" ;;
    "view @opencode/cli@"*" dist.integrity") echo "sha512-other" ;;
    "install --package-lock-only --omit=dev") ;;
    *) echo "unexpected npm call: \$*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/npm"
resolve() { PATH="$tmp/bin:$PATH" bash "$tmp/repo/scripts/resolve-opencode-version.sh"; }

# 1. 沒有指定時維持釘選版本，即使 npm 上有更新版。
out="$(resolve 2>&1)" || fail "default resolution failed: $out"
grep -qx "OPENCODE_VERSION=$pinned" "$tmp/repo/release/runtime-inputs.env" ||
    fail "pin moved off $pinned: $(grep OPENCODE_VERSION "$tmp/repo/release/runtime-inputs.env")"
grep -Fq "\"@opencode/cli\": \"$pinned\"" "$tmp/repo/release/package.json" || fail "package.json no longer pins $pinned"
cmp -s "$root/release/runtime-inputs.env" "$tmp/repo/release/runtime-inputs.env" ||
    fail "default resolution rewrote runtime-inputs.env"

# 2. OPENCODE_VERSION_PIN 指定 V1 版本時拒絕，不改任何檔案。
cp "$tmp/repo/release/runtime-inputs.env" "$tmp/before.env"
if out="$(OPENCODE_VERSION_PIN=1.19.6 resolve 2>&1)"; then fail "a V1 pin was accepted: $out"; fi
grep -q "V2" <<< "$out" || fail "V1 pin rejection does not say V2 is required: $out"
cmp -s "$tmp/before.env" "$tmp/repo/release/runtime-inputs.env" || fail "a rejected pin rewrote runtime-inputs.env"

# 3. 指定釘選版本本身照常通過（release CI 第二次呼叫就是這樣）。
OPENCODE_VERSION_PIN="$pinned" resolve > /dev/null 2>&1 || fail "pinning the pinned version failed"

echo "resolve-opencode-version checks passed (pinned $pinned)"
