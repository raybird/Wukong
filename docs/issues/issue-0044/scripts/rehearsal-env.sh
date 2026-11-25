#!/usr/bin/env bash
# Task 6.1：在獨立的 Docker daemon（docker:27-dind）裡準備端到端演練的環境。
# 用法：rehearsal-env.sh WORK_DIR IMAGE [TAG]
#   WORK_DIR：演練目錄（之後以 /reh 掛進 dind），內含 deploy/、assets/、bin/
#   IMAGE：主機上內建 V2 的映像（例如 wukong-issue44:v2-local）
#   TAG：假的發版 tag，預設 v0.26.0-rc.44
# 在 repo 根目錄執行。
#
# 為什麼要獨立的 daemon：installer 依固定的容器名稱（wukong-telegram 等）找既有部署的
# Compose project。主機上正式部署正用這些名稱，演練若跑在主機 daemon 上，installer 會認出
# 正式部署的 project，對它 stop／up。dind 裡沒有正式部署的任何容器。
#
# 與真實發版只差兩處，其餘照 release.yml 與 installer 原樣：
# - 映像來源：本版映像推進 dind 裡的 registry:2 再拉回，取得真實的 registry digest，
#   再標成 ghcr.io/raybird/wukong:TAG。installer 的 `compose pull` 對這個 tag 不連 GHCR
#   （bin/docker 只跳過這一種呼叫），digest 比對照常執行
# - 發版檔案來源：bin/curl 把 GitHub Release 的 TAG 下載網址對應到 assets/，其他網址照常連網
set -euo pipefail
work="$(realpath "${1:?usage: rehearsal-env.sh WORK_DIR IMAGE [TAG]}")"
image="${2:?}"
tag="${3:-v0.26.0-rc.44}"
repo="$(pwd)"
dind=wk44reh-dind
mkdir -p "$work/deploy" "$work/assets" "$work/bin"

if ! docker inspect "$dind" > /dev/null 2>&1; then
    docker run -d --privileged --name "$dind" -v wk44reh-dind-data:/var/lib/docker \
        -v "$repo":/repo:ro -v "$work":/reh docker:27-dind > /dev/null
    for _ in $(seq 1 30); do docker exec "$dind" docker info > /dev/null 2>&1 && break; sleep 1; done
    docker exec "$dind" apk add -q --no-cache bash python3 curl sqlite coreutils tar findutils jq
fi
in_dind() { docker exec -i "$dind" bash -euo pipefail -c "$1"; }

# 映像：前一版從 GHCR 拉（真實 digest）；本版經 dind 內的 registry 取得 digest
in_dind 'docker pull -q ghcr.io/raybird/wukong:v0.25.1 > /dev/null'
if ! in_dind "docker image inspect ghcr.io/raybird/wukong:$tag > /dev/null 2>&1"; then
    docker save "$image" registry:2 | docker exec -i "$dind" docker load > /dev/null
    in_dind "docker inspect reg > /dev/null 2>&1 || docker run -d --name reg -p 127.0.0.1:5000:5000 registry:2 > /dev/null
        sleep 2
        docker tag '$image' 127.0.0.1:5000/raybird/wukong:$tag
        docker push -q 127.0.0.1:5000/raybird/wukong:$tag > /dev/null
        docker rmi -f 127.0.0.1:5000/raybird/wukong:$tag '$image' > /dev/null
        docker pull -q 127.0.0.1:5000/raybird/wukong:$tag > /dev/null
        docker tag 127.0.0.1:5000/raybird/wukong:$tag ghcr.io/raybird/wukong:$tag"
fi
digest="$(in_dind "docker image inspect ghcr.io/raybird/wukong:$tag --format '{{index .RepoDigests 0}}'" | sed 's/.*@//')"

# 發版檔案：照 release.yml 的 Assemble public artifacts（只做 Docker 模式用得到的部分）
dist="$work/assets/$tag"
rm -rf "$dist" && mkdir -p "$dist/wukong-docker/scripts" "$dist/wukong-docker/docker"
sed "s/__WUKONG_VERSION__/$tag/g" docker-compose.release.yml > "$dist/wukong-docker/docker-compose.yml"
cp .env.example LICENSE docker-compose.memoria.yml release/data-compatibility.json "$dist/wukong-docker/"
cp -R docker/memoria-runtime "$dist/wukong-docker/docker/"
cp scripts/install.sh "$dist/wukong-docker/scripts/install.sh"
runtime_inputs="$(python3 -c 'import json; d={}; [d.__setitem__(*line.strip().split("=", 1)) for line in open("release/runtime-inputs.env") if line.strip() and not line.startswith("#")]; print(json.dumps(d, sort_keys=True))')"
scripts/generate-release-manifest.sh --tag "$tag" --commit "$(git rev-parse HEAD)" --channel rc \
    --image-reference "ghcr.io/raybird/wukong:$tag" --image-digest "$digest" --platform linux/amd64 \
    --runtime-inputs "$runtime_inputs" --data-compatibility release/data-compatibility.json \
    --output "$dist/wukong-docker/release-manifest.json"
cp "$dist/wukong-docker/release-manifest.json" "$dist/release-manifest.json"
tar -C "$dist" -czf "$dist/wukong-docker-$tag.tar.gz" wukong-docker
rm -rf "$dist/wukong-docker"
scripts/generate-sha256sums.sh "$dist"

# shim：只攔演練 tag 的下載與 pull
cat > "$work/bin/curl" <<SH
#!/usr/bin/env bash
prefix="https://github.com/raybird/Wukong/releases/download/$tag/"
args=("\$@")
for i in "\${!args[@]}"; do
    if [[ "\${args[\$i]}" == "\$prefix"* ]]; then
        args[\$i]="file:///reh/assets/$tag/\${args[\$i]#"\$prefix"}"
        echo "[rehearsal curl] \${args[\$i]}" >&2
    fi
done
exec /usr/bin/curl "\${args[@]}"
SH
cat > "$work/bin/docker" <<SH
#!/usr/bin/env bash
if [[ " \$* " == *" compose "* && " \$* " == *" pull"* ]]; then
    for f in "\$@"; do
        if [[ -f "\$f" ]] && grep -q "ghcr.io/raybird/wukong:$tag" "\$f"; then
            echo "[rehearsal docker] skip GHCR pull for $tag (image is local, digest is checked next)" >&2
            exit 0
        fi
    done
    if [[ " \$* " != *" -f "* ]] && grep -q "ghcr.io/raybird/wukong:$tag" docker-compose.yml 2>/dev/null; then
        echo "[rehearsal docker] skip GHCR pull for $tag (image is local, digest is checked next)" >&2
        exit 0
    fi
fi
exec /usr/local/bin/docker "\$@"
SH
chmod +x "$work/bin/curl" "$work/bin/docker"
echo "image digest: $digest"
echo "assets: $(ls "$dist" | tr '\n' ' ')"
