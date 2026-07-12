# Wukong 發布與安裝演進設計

**日期：** 2026-07-10
**狀態：** Design approved
**目標版本：** 下一個 minor release，先以 RC 驗證

## Goal

下一版 Wukong 吸收 TeleNexus 的一鍵 release、預建 OCI image 與明確升級狀態契約，同時保留 Binary-first、多專案工作流：

1. 維護者只使用單一 release command，完成前置驗證、annotated tag、workflow watch 與發布後驗證。
2. Binary mode 保持本機 coding assistant 的主要安裝方式，不改變依工作目錄建立 project scope 的行為。
3. CI 發布 tag-pinned GHCR image，Docker installer 改為 pull，不在使用者端 build。
4. Docker 與 Binary install、upgrade、rollback 都遵守 release-owned/user-owned state contract。
5. Binary installer 提供冪等 `--upgrade`、保留 `config.env`、維持已選元件集合，並可選 `wukong-schedulerd`。
6. Binary、Docker bundle 與 image digest 都能追溯到同一產品 tag。

產品版本唯一使用 Git tag，例如 `v0.18.0`。`Cargo.toml` 的 workspace `0.1.0` 是內部 package version，不參與產品 release tag 的產生或驗證。

### Borrowed contracts and intentional differences

| 主題 | 向 TeleNexus 借鏡 | Wukong 的刻意差異 |
|---|---|---|
| Release command | 單一命令負責 preflight、tag、push與驗證 | 不自動修改 source/version commit；產品版本仍是 Git tag |
| Docker delivery | CI 預建 OCI image，installer只 pull | Binary mode仍是多專案 coding的主要入口 |
| State ownership | Deployment檔可替換，設定/data/workspace永不覆寫 | Binary元件集合與已啟用 services也納入 user選擇狀態 |
| Upgrade/rollback | 指定版本、原子替換、health rollback | Cargo workspace `0.1.0` 不參與產品版本 |
| Latest/RC | RC 不更新 latest | Installer與release bundle完全不依賴 floating latest image |

## Current State

- `.github/workflows/release.yml` 由 `v*` tag 觸發，平行建立 Linux GNU、Linux musl、Apple Silicon 的四個 binaries，產生 per-target checksums，再由單一 publish job 建立 GitHub Release。
- RC 已正確標記 prerelease，且不成為 latest。
- 發布主要依賴維護者依 `.claude/skills/wukong-release/SKILL.md` 手動檢查、tag、push、監看與驗證，沒有唯一可執行 gate。
- Docker bundle 仍包含 Dockerfile；使用者需 `docker compose build --no-cache`。Dockerfile再從 Release 下載 binaries，因此沒有編譯 Rust，但同一 Wukong tag 仍可能因外部 package 變動得到不同 image。
- Binary installer 驗證 tarball SHA256，支援 GNU、musl 與 Apple Silicon，也可安裝 Telegram/Web user services。
- `--upgrade` 目前只適用 Docker。重跑 Binary installer 會重新詢問並重寫 `~/.wukong/config.env`。
- Binary installer 尚未安裝或管理 `wukong-schedulerd`。
- Docker bundle、workspace template 與 user state 的所有權界線尚未集中成可測試契約。

## Decisions

### Product tag is the release version

合法格式：

```text
vX.Y.Z
vX.Y.Z-rc.N
```

Workflow 不使用 Cargo package version、Dockerfile `ARG VERSION`、README badge 或 floating `latest` 推導產品版本。

### Single release command

新增：

```bash
./scripts/release.sh v0.18.0-rc.1
./scripts/release.sh v0.18.0 --promote-from v0.18.0-rc.2
./scripts/release.sh v0.18.0-rc.1 --dry-run
```

Release command 不修改 source、Cargo version、CHANGELOG 或 commit。候選 commit 必須先準備完成。

成功定義為：preflight 通過、annotated tag 已 push、Release workflow 成功、GitHub Release assets 完整、GHCR digest 與 release manifest 一致。

### Stable promotion from RC

Stable 必須提供 `--promote-from`，來源 RC 與 stable 指向同一 commit。Stable annotated tag message固定包含 workflow可解析的 metadata：

```text
v0.18.0
promote-from: v0.18.0-rc.2
```

Release command 在 push 前驗證 annotation；tag workflow以 `git for-each-ref refs/tags/${GITHUB_REF_NAME} --format='%(contents)'` 讀取並驗證 `promote-from:`。缺少、重複或格式錯誤時 `validate` job失敗。Stable GHCR tag直接指向已驗證的 RC digest，不重新 build image。

### Development and release Compose separation

- 根目錄 `docker-compose.yml` 保留 `build:`，服務 repository development。
- 新增 `docker-compose.release.yml`，只含 tag-pinned `image:`。
- Release bundle 把 release template命名為 `docker-compose.yml`，維持使用者指令相容。

### Pull-only Docker installation

Release services使用：

```yaml
image: ghcr.io/raybird/wukong:<product-tag>
```

安裝與升級只執行：

```bash
docker compose pull
docker compose up -d --force-recreate
```

Bundle 不再包含 Dockerfile、source、entrypoint、workspace templates 或 runtime skills；這些全部由 versioned image 擁有。

### Idempotent Binary upgrade

```bash
bash scripts/install.sh --mode binary --upgrade
bash scripts/install.sh --mode binary --upgrade --version v0.18.0-rc.1
bash scripts/install.sh --mode binary --upgrade --with-schedulerd
```

Upgrade 規則：

- 一律升級 `wukong`。
- 只自動升級已存在的 Telegram、Web、Scheduler binaries。
- 不顯示 component/settings prompts。
- 不覆寫 `~/.wukong/config.env`。
- 不自動安裝新元件；`--with-schedulerd` 是明確 opt-in。
- Linux 只重啟升級前已啟用的 user services。
- 同版本重跑仍驗證 assets，但不改變設定或元件集合。

## Release Pipeline

### Preflight

`scripts/release.sh` 在建立 tag 前依序驗證：

1. 位於正確 repository，branch 是 `main` 或 `release/*`。
2. Version/tag 格式合法，local/remote 尚無同名 tag。
3. Tracked/untracked worktree乾淨。
4. HEAD 已存在於 origin，branch 與 upstream 沒有 ahead/behind。
5. `CHANGELOG.md` 有目標版本；stable 已填發布日期。
6. Stable 有合法 `--promote-from`，且 RC 與 HEAD 是同一 commit。
7. `Cargo.lock` 存在且 `cargo metadata --locked` 成功。
8. Release Compose 不含 `build:`，並使用 GHCR placeholder。
9. Breaking/default env 變更已同步 `.env.example`、兩份 Compose、`docs/docker.md` 與 CHANGELOG。
10. `gh auth status` 成功。
11. 執行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/test-release-workflow.sh
bash scripts/test-release-command.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
```

任一失敗時不得建立 tag。`--dry-run` 執行全部 preflight 並列出預計 tag/assets，但不建立或推送 tag。

### Workflow jobs

1. `validate`：驗證 tag、lockfile、RC/stable annotation 與 release Compose。
2. `build-binaries`：沿用三 target matrix，建立四個 binaries 與 checksums。
3. `publish-image`：以 CI 已建好的 musl binaries建立 `linux/amd64` GHCR image；stable 只 promote RC digest。
4. `package-release`：產生 tag-pinned Docker bundle、`release-manifest.json` 與涵蓋全部 assets 的 `SHA256SUMS`。
5. `publish`：單一 serialized job 上傳 GitHub Release assets，避免 race。

### GHCR tags

```text
ghcr.io/raybird/wukong:v0.18.0-rc.1
ghcr.io/raybird/wukong:v0.18.0
ghcr.io/raybird/wukong:sha-<full-commit-sha>
```

第一階段只支援 `linux/amd64`。產品 tag 與 commit tag 發布後不可被不同 digest 覆寫。

Image 必須有 OCI source、revision、version、created labels。RC build的 `org.opencontainers.image.version` 使用實際 build origin RC tag，不使用 Cargo version。Stable promotion不重建 image，因此保留來源 RC version label；stable產品版本、channel與 `promotedFrom` 以 stable release manifest和registry tag表達。這是維持 RC/stable digest完全相同的刻意取捨。

`Dockerfile.release` 的可重現輸入必須固定：

- Base image使用 immutable digest。
- Opencode使用明確版本，不使用 `latest`。
- Agent Reach使用明確 release或commit SHA，不使用 `main`。
- GitHub CLI與系統 packages透過固定 base snapshot或將解析版本記錄於 manifest。
- Node/npm-installed runtime packages使用 lockfile或明確版本。

上述版本與 digest都寫入 `release-manifest.json`。同一 RC workflow rerun若現有 GHCR tag digest不同，必須失敗而非覆寫。

### Release manifest

`release-manifest.json` 至少記錄產品 tag、commit、channel、來源 RC、GHCR reference/digest、Docker platform、binary targets與 pinned external runtime inputs。Installer 以此驗證拉取 image 的 RepoDigest。

生成順序固定為：先完成 binaries、Docker bundle與 manifest，再對這些檔案產生 `SHA256SUMS`。`SHA256SUMS` 本身不列入 checksum；manifest不記錄個別 artifact checksum，避免循環雜湊。

## Installer UX

### Docker install/upgrade

```bash
bash scripts/install.sh --mode docker
bash scripts/install.sh --mode docker --upgrade
bash scripts/install.sh --mode docker --upgrade --version v0.18.0-rc.1
```

Bare `--upgrade` 維持 Docker mode，確保既有文件與自動化相容。

流程：download bundle、`SHA256SUMS`、manifest → verify checksum/archive → stage release-owned files → pull image → verify digest → atomic replace → recreate → healthcheck。失敗時恢復舊 deployment files 與 image tag。

### Binary install/upgrade/rollback

首次安裝保留 CLI/Telegram/Web 選擇，新增 Scheduler 選項。若 `config.env` 已存在，即使不是 upgrade 也不得覆寫。

Rollback：

```bash
bash scripts/install.sh --mode binary --rollback
bash scripts/install.sh --mode binary --rollback --version v0.17.1
```

Installer 先下載並驗證所有 selected components，再以同 filesystem staging/backup 執行原子替換。替換或 service restart 失敗時恢復 binaries 與 unit files；`config.env` 與 data 不參與 transaction。

macOS 可安裝 schedulerd binary，但本次不建立 launchd service。

### Persistent install metadata

Docker mode在部署目錄使用 `.wukong-release`；Binary mode使用 `~/.wukong/install.json`。兩者 schema version 1：

```json
{
  "schemaVersion": 1,
  "mode": "binary",
  "target": "x86_64-unknown-linux-musl",
  "currentVersion": "v0.18.0-rc.1",
  "previousVersion": "v0.17.1",
  "components": ["wukong", "wukong-web", "wukong-schedulerd"],
  "enabledUserServices": ["wukong-web", "wukong-schedulerd"],
  "installedAt": "<UTC ISO-8601>",
  "updatedAt": "<UTC ISO-8601>"
}
```

Docker metadata另記錄 image reference/digest；Binary metadata另記錄 target與components。Installer每次成功 transaction後原子更新 metadata。Bare `--rollback` 只選 `previousVersion`；缺少或無法驗證時拒絕並要求 `--version`。只保留 current與previous的自動 rollback metadata，任意更舊版本必須明確指定。

Legacy Binary installation第一次使用新式 `--upgrade` 時，因既有 binary只回報Cargo version而無法可信推導產品 tag，installer不得猜測版本。它會：

1. 在替換前計算現有 components的SHA256。
2. 原樣備份到 `~/.wukong/backups/legacy-<UTC timestamp>/`。
3. 建立新 metadata，`currentVersion` 設為新安裝tag、`previousVersion` 設為 `null`、`previousBackupPath` 指向legacy backup。
4. 第一次 bare `--rollback` 直接原子恢復該 local backup；不從GitHub下載未知版本。
5. 下一次成功upgrade後，`previousVersion` 才記錄已知產品tag，並清除已不需要的legacy backup pointer。

Schema因此允許 `previousVersion: null` 與 optional `previousBackupPath`，但兩者不得同時缺少於可bare rollback的狀態。

## State Ownership

### Docker release-owned

- `docker-compose.yml`
- `.env.example`
- `LICENSE`
- `scripts/install.sh`
- `.wukong-release`
- GHCR image filesystem與 canonical runtime skill assets

### Docker user-owned

- `.env`
- `workspace/**` 與自訂 host workspace
- `wukong-data`、`opencode-config`、`opencode-state`、`agent-reach-state`、`gh-config` volumes
- Compose overrides
- 不在 release-owned allowlist 內的其他檔案

### Binary release-owned

- `~/.local/bin/wukong*`
- Installer-managed systemd user units
- `~/.wukong/install.json`

### Binary user-owned

- `~/.wukong/config.env`
- Memory database與 Markdown mirror
- `~/.wukong/workspace/**`
- `SOUL.md`、`AGENTS.md`
- User scripts、skills與非 installer-managed service files

Workspace templates 只在缺檔時初始化，之後永遠視為 user-owned。Installer 可以警告既有 env 缺少新 key，但不得自動改寫值。

## Security

- `SHA256SUMS` 涵蓋所有 binary tarballs、Docker bundle與 `release-manifest.json`；它不包含自身。解壓或替換前強制驗證。
- Docker pull 後驗證 RepoDigest 與 release manifest。
- 拒絕 absolute path、`..` traversal 與契約外 archive entries。
- Workflow 權限限於 `contents: write`、`packages: write`。
- Release logs 不輸出 env、tokens、cookies或 credentials。
- Web host bind 預設保持 `127.0.0.1`，認證保留於 user-owned volumes。
- GHCR image 沿用 UID/GID 對齊後降權執行。
- SHA256/digest 不等於簽章；cosign、Sigstore、SLSA 不在本次範圍。

## Failure Handling

- Preflight failure：不建立 tag、不修改版本或 CHANGELOG，顯示失敗命令。
- Workflow failure after tag push：source 需要修改時發布新 RC/patch，不重用公開 tag；純暫時性 upload 問題只允許同 commit rerun。
- Docker checksum/pull/digest failure：不修改目前 deployment。
- Docker health failure：恢復舊 Compose 與舊 image tag，不執行 `down -v`。
- Binary component download/checksum failure：不替換任何 binary。
- Binary replacement/restart failure：恢復本次 binaries 與 units；保留 config/data。
- Image/binary rollback 不自動回退 database。不可逆 migration 必須在 CHANGELOG/release notes 提供備份與資料復原程序。

## Testing

### Release command/workflow

- Temporary git repo、bare remote 與 fake `gh` 驗證 clean/upstream/tag/RC promotion/preflight/dry-run/workflow failure。
- 拒絕把 Cargo `0.1.0` 當產品 tag。
- 驗證單一 GitHub Release upload、packages permission、GHCR tags、stable digest promotion、checksums、manifest 與 `--locked` builds。

### Installer

- Temporary `HOME`、mock release server、fake Docker/systemctl。
- Docker path 不出現 `docker compose build`。
- Checksum/archive/digest failure 不修改部署。
- `.env`、workspace、volumes不在 overwrite allowlist。
- Binary upgrade 不進 prompts，只更新既有元件，`config.env` byte-for-byte 保留。
- `--with-schedulerd`、same-version rerun、atomic failure recovery 與 previous-version rollback。

### Docker runtime

- Development Compose 可含 `build:`；Release Compose 禁止 `build:`。
- Release services使用相同 GHCR tag並保留既有 volumes與 loopback Web default。
- Image 內四個 binaries可執行，labels正確，服務以非 root 執行，healthcheck通過。

### RC end-to-end

每個 RC 至少驗證 Binary clean install/upgrade/rerun/rollback、Scheduler opt-in、Docker clean install/upgrade/rollback、Web health、Telegram、Scheduler jobs與所有認證/data volumes保留。

## RC Rollout And Rollback

1. 以 `./scripts/release.sh v0.18.0-rc.1` 發布；installer 不得自動解析到 RC。
2. 發現需要 source 修正時發布新 RC，不移動或重建既有 RC tag。
3. 記錄 workflow/release URL、image digest、checksums與 install/upgrade/rollback smoke results。
4. Stable promotion 前必須完成 Docker/Binary upgrade與 rollback rehearsal、CHANGELOG與 breaking env upgrade notes。
5. `./scripts/release.sh v0.18.0 --promote-from v0.18.0-rc.2` 只能在相同 commit執行，stable digest 必須等於 RC digest。
6. 發現 stable 問題時不覆寫 tag/image；將有問題 Release標示撤回，修正後走新 patch RC。

## Success Metrics

- 100% 對外 releases 由 `scripts/release.sh` 建立。
- 100% stable releases 由同 commit RC promotion。
- Stable GHCR digest 與來源 RC digest 完全一致。
- 除 `SHA256SUMS` 自身外，100% GitHub Release assets 被其涵蓋。
- Docker install/upgrade/rollback 執行 `docker compose build` 次數為 0。
- Binary/Docker same-version rerun成功且不改變 user-owned state。
- Upgrade/rollback 前後 `config.env`、`.env`、workspace與 data hashes不變。
- 每個 RC 至少完成一次上一 stable → RC upgrade與 rollback。
- 一般 Docker rollback在 image已快取時 10 分鐘內完成。

## Out Of Scope

- 將 Wukong 改成 Docker-first或改變多專案 scope
- Linux ARM64 GHCR、Intel Mac binary、macOS launchd
- 同步 Cargo workspace/crate version與產品 tag
- 自動修改 source、Cargo version、CHANGELOG或產生 release notes
- 改變 runtime、memory、scheduler或 backend行為
- 自動回退 user-owned database
- Cosign、Sigstore、SLSA、SBOM發布
- 對缺少 manifest/checksum的舊 Docker release提供未驗證自動 rollback

## Expected File Changes

### New files

- `scripts/release.sh`
- `scripts/test-release-command.sh`
- `docker-compose.release.yml`
- `Dockerfile.release`
- `release-manifest.json` template or generator input

### Modified files

- `.github/workflows/release.yml`
- `scripts/install.sh`
- `scripts/test-release-workflow.sh`
- `scripts/test-installer-upgrade.sh`
- `scripts/test-docker-runtime.sh`
- `docker-compose.yml`
- `Dockerfile`
- `.env.example`
- `README.md`
- `docs/installation.md`
- `docs/docker.md`
- `CHANGELOG.md`
- `.claude/skills/wukong-release/SKILL.md`

## Delivery Phases

### Phase 1: Release foundation

Entry：現有 tag workflow與binary assets正常。
內容：`scripts/release.sh`、annotated promotion metadata、preflight tests、manifest/checksum生成規則。
Exit：RC dry-run、fake remote tag flow、workflow static tests及既有 binary assets通過；尚不改 Docker installer。

### Phase 2: Reproducible GHCR publication

Entry：Phase 1可安全產生 RC tag。
內容：`Dockerfile.release`、pinned external inputs、GHCR RC image、stable digest promotion、release Compose與minimal bundle。
Exit：RC image smoke test、digest/manifest一致、stable promotion不 rebuild且使用相同 digest。

### Phase 3: Installer migration

Entry：Tag-pinned GHCR image與verified bundle可用。
內容：Docker pull-only流程、Binary idempotent upgrade、Scheduler opt-in、state ownership與install metadata。
Exit：Docker/Binary clean install、same-version rerun、upgrade與user-state hash preservation通過。

### Phase 4: Rollback and RC rehearsal

Entry：兩種 mode都有transactional upgrade與health checks。
內容：previousVersion rollback、failure recovery、migration compatibility guard與完整RC matrix。
Exit：上一 stable到 RC再 rollback在Docker/Binary都成功，才允許 stable promotion。
