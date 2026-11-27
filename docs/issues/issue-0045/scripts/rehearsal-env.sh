#!/usr/bin/env bash
# #45 Task 6.3：在獨立的 Docker daemon（docker:27-dind）裡準備一個假的發版，供端到端演練使用。
# 改寫自 docs/issues/issue-0044/scripts/rehearsal-env.sh（理由與差異見該檔與 issue-0044/rehearsal.md）。
# 用法：rehearsal-env.sh WORK_DIR TAG IMAGE SRC_DIR
#   WORK_DIR：演練目錄（以 /reh 掛進 dind），內含 deploy/、assets/、bin/
#   TAG：假的發版 tag（例如 v0.26.0-rc.44）
#   IMAGE：主機上的映像（scripts/build-image.sh 建的，例如 wukong-issue45:b65b487）
#   SRC_DIR：該映像那個提交的原始碼（build-image.sh 匯出的 target/issue45-image/src-<sha>），
#            發版檔案（compose、installer、manifest）都取自這裡，不取自目前的工作區
# 在 repo 根目錄執行；同一個 WORK_DIR 可以對不同的 TAG 各跑一次。
#
# 與 #44 的差異：
# - 升級的起點與終點都是假的發版：v0.25.1 之後還沒有 V2 的正式版本（b65b487 沒有 tag）
# - shim 對 assets/ 底下的每個 tag 都生效，不只一個
set -euo pipefail
work="$(realpath "${1:?usage: rehearsal-env.sh WORK_DIR TAG IMAGE SRC_DIR}")"
tag="${2:?}" image="${3:?}" src="$(realpath "${4:?}")"
dind=wk45reh-dind
mkdir -p "$work/deploy" "$work/assets" "$work/bin"
commit="$(docker image inspect "$image" --format '{{index .Config.Labels "org.opencontainers.image.revision"}}')"

if ! docker inspect "$dind" > /dev/null 2>&1; then
    docker run -d --privileged --name "$dind" -v wk45reh-dind-data:/var/lib/docker -v "$work":/reh docker:27-dind > /dev/null
    for _ in $(seq 1 30); do docker exec "$dind" docker info > /dev/null 2>&1 && break; sleep 1; done
    docker exec "$dind" apk add -q --no-cache bash python3 curl sqlite coreutils tar findutils jq
fi
in_dind() { docker exec -i "$dind" bash -euo pipefail -c "$1"; }

# 映像：經 dind 內的 registry 取得真實的 registry digest，再標成 ghcr.io/raybird/wukong:TAG
if ! in_dind "docker image inspect ghcr.io/raybird/wukong:$tag > /dev/null 2>&1"; then
    docker save "$image" registry:2 | docker exec -i "$dind" docker load > /dev/null
    in_dind "docker inspect reg > /dev/null 2>&1 || { docker run -d --name reg -p 127.0.0.1:5000:5000 registry:2 > /dev/null; sleep 2; }
        docker tag '$image' 127.0.0.1:5000/raybird/wukong:$tag
        docker push -q 127.0.0.1:5000/raybird/wukong:$tag > /dev/null
        docker rmi -f 127.0.0.1:5000/raybird/wukong:$tag '$image' > /dev/null
        docker pull -q 127.0.0.1:5000/raybird/wukong:$tag > /dev/null
        docker tag 127.0.0.1:5000/raybird/wukong:$tag ghcr.io/raybird/wukong:$tag"
fi
digest="$(in_dind "docker image inspect ghcr.io/raybird/wukong:$tag --format '{{index .RepoDigests 0}}'" | sed 's/.*@//')"

# 發版檔案：照該提交 release.yml 的 Assemble public artifacts（只做 Docker 模式用得到的部分）
dist="$work/assets/$tag"
mkdir -p "$dist"
stage="$(mktemp -d "$work/stage.XXXX")"
mkdir -p "$stage/wukong-docker/scripts" "$stage/wukong-docker/docker"
sed "s/__WUKONG_VERSION__/$tag/g" "$src/docker-compose.release.yml" > "$stage/wukong-docker/docker-compose.yml"
cp "$src/.env.example" "$src/LICENSE" "$src/docker-compose.memoria.yml" "$src/release/data-compatibility.json" "$stage/wukong-docker/"
cp -R "$src/docker/memoria-runtime" "$stage/wukong-docker/docker/"
cp "$src/scripts/install.sh" "$stage/wukong-docker/scripts/install.sh"
runtime_inputs="$(python3 -I -c 'import json,sys; d={}; [d.__setitem__(*line.strip().split("=", 1)) for line in open(sys.argv[1]) if line.strip() and not line.startswith("#")]; print(json.dumps(d, sort_keys=True))' "$src/release/runtime-inputs.env")"
(cd "$src" && scripts/generate-release-manifest.sh --tag "$tag" --commit "$commit" --channel rc \
    --image-reference "ghcr.io/raybird/wukong:$tag" --image-digest "$digest" --platform linux/amd64 \
    --runtime-inputs "$runtime_inputs" --data-compatibility release/data-compatibility.json \
    --output "$stage/wukong-docker/release-manifest.json")
cp "$stage/wukong-docker/release-manifest.json" "$dist/release-manifest.json"
tar -C "$stage" -czf "$dist/wukong-docker-$tag.tar.gz" wukong-docker
cp "$src/scripts/install.sh" "$work/install-$tag.sh"
(cd "$src" && scripts/generate-sha256sums.sh "$dist")

# shim：只攔 assets/ 底下各個演練 tag 的下載與 pull，其他照常
cat > "$work/bin/curl" <<'SH'
#!/usr/bin/env bash
args=("$@")
for i in "${!args[@]}"; do
    for dir in /reh/assets/*/; do
        t="$(basename "$dir")" prefix="https://github.com/raybird/Wukong/releases/download/$t/"
        if [[ "${args[$i]}" == "$prefix"* ]]; then
            args[$i]="file:///reh/assets/$t/${args[$i]#"$prefix"}"
            echo "[rehearsal curl] ${args[$i]}" >&2
        fi
    done
done
exec /usr/bin/curl "${args[@]}"
SH
cat > "$work/bin/docker" <<'SH'
#!/usr/bin/env bash
if [[ " $* " == *" compose "* && " $* " == *" pull"* ]]; then
    files=()
    for f in "$@"; do [[ -f "$f" ]] && files+=("$f"); done
    [[ " $* " != *" -f "* && -f docker-compose.yml ]] && files+=(docker-compose.yml)
    for dir in /reh/assets/*/; do
        t="$(basename "$dir")"
        for f in "${files[@]}"; do
            if grep -q "ghcr.io/raybird/wukong:$t" "$f"; then
                echo "[rehearsal docker] skip GHCR pull for $t (image is local, digest is checked next)" >&2
                exit 0
            fi
        done
    done
fi
exec /usr/local/bin/docker "$@"
SH
chmod +x "$work/bin/curl" "$work/bin/docker"
echo "$tag image digest: $digest (commit $commit)"
echo "assets: $(ls "$dist" | tr '\n' ' ')"
