#!/usr/bin/env bash
# #45 Task 6.2：照 release workflow 的方式在本機建某個提交的映像（#44 Task 6 也是這樣建 wukong-issue44:v2-local）。
# 用法：build-image.sh REV TAG
#   REV：要建的提交（例如 b65b487）；以 git archive 匯出，不切換目前的分支
#   TAG：本機映像名稱（例如 wukong-issue45:b65b487）
# 在 repo 根目錄執行。建置目錄在 target/issue45-image/（已被 .gitignore 排除、在磁碟上而不是 tmpfs）；
# 不同提交共用同一個 cargo target 與 cargo home，相同的依賴不重編。
# musl binaries 在 rust:1.96-bookworm 容器裡建（以 root 安裝 musl-tools，結束時把產物改回目前使用者）；
# release-context 依該提交 Dockerfile.release 的 COPY 來源複製（等同 workflow 的清單），build args 取自該提交的 release/runtime-inputs.env。
set -euo pipefail
rev="${1:?usage: build-image.sh REV TAG}" tag="${2:?}"
repo="$(pwd)"
base="$repo/target/issue45-image"
commit="$(git rev-parse "$rev^{commit}")"
src="$base/src-${commit:0:7}" ctx="$base/ctx-${commit:0:7}"
mkdir -p "$src" "$ctx/binaries" "$base/cargo-target" "$base/cargo-home"
git archive "$commit" | tar -x -C "$src"
# git archive 把檔案時間設成提交時間；兩個提交都掛在 /src、共用 cargo target，較舊的時間會讓 cargo
# 把上一個提交建好的 workspace crate 當成最新而不重編（2026-11-26 建 HEAD 時遇到）。改成現在的時間。
find "$src" -type f -exec touch {} +

docker run --rm -v "$src":/src -v "$base/cargo-target":/target -v "$base/cargo-home":/cargo \
    -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/target -w /src rust:1.96-bookworm bash -c "
    set -e
    apt-get update -qq > /dev/null && apt-get install -y -qq musl-tools > /dev/null
    rustup target add x86_64-unknown-linux-musl > /dev/null
    trap 'chown -R $(id -u):$(id -g) /target /cargo /src' EXIT
    cargo build --release --locked --target x86_64-unknown-linux-musl \
        --bin wukong --bin wukong-telegram --bin wukong-web --bin wukong-schedulerd"
for bin in wukong wukong-telegram wukong-web wukong-schedulerd; do
    cp "$base/cargo-target/x86_64-unknown-linux-musl/release/$bin" "$ctx/binaries/$bin"
done
cp "$src/Dockerfile.release" "$ctx/"
# 依該提交 Dockerfile.release 的 COPY 來源組 context（與該提交 workflow 的清單相同；b65b487 還複製技能檔）
grep '^COPY ' "$src/Dockerfile.release" | grep -v -- '--from' | while read -r _ args; do
    set -- $args
    while (( $# > 1 )); do
        [[ "$1" == binaries/* ]] || { mkdir -p "$ctx/$(dirname "$1")"; cp -R "$src/$1" "$ctx/$(dirname "$1")/"; }
        shift
    done
done
set -a; source "$src/release/runtime-inputs.env"; set +a
docker build -q --file "$ctx/Dockerfile.release" --tag "$tag" \
    --build-arg BASE_IMAGE="$BASE_IMAGE" --build-arg DEBIAN_SNAPSHOT="$DEBIAN_SNAPSHOT" \
    --build-arg OPENCODE_VERSION="$OPENCODE_VERSION" --build-arg AGENT_REACH_REF="$AGENT_REACH_REF" \
    --build-arg AGENT_REACH_ARCHIVE_SHA256="$AGENT_REACH_ARCHIVE_SHA256" --build-arg PRODUCT_TAG="v0.0.0-issue45" \
    --build-arg BUILD_ORIGIN_TAG="v0.0.0-issue45-${commit:0:7}" --build-arg REVISION="$commit" \
    --build-arg CREATED="$(date -Iseconds)" "$ctx"
echo "built $tag from $commit"
