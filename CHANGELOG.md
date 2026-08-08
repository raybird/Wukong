# Changelog

本專案的所有重要變更都會記錄在此檔。

格式依循 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版本號採用 [Semantic Versioning](https://semver.org/lang/zh-TW/)（實際發佈 tag 為 `v0.16.x`）。

> 本 CHANGELOG 自 `v0.16.35` 起維護；更早的版本紀錄請見
> [GitHub Releases](https://github.com/raybird/Wukong/releases) 與 `git log`。
> 維護方式：每次發佈前，把 `Unreleased` 區塊的條目整理為新版本區塊並標上發佈日期。

## [Unreleased]

### Added

- `scripts/collect-opencode-baseline.sh`：蒐集 `opencode-server` 的常駐基線樣本。
  CPU、RSS、thread 與 cgroup 數值全部由 host 端讀 `/proc` 與 cgroup 檔案取得，
  不需要 `docker exec`；並且先靜置量完 CPU 才做 SQLite 等侵入式查詢，避免診斷
  指令自己的 CPU 被計進容器 cgroup 而誤讀為 opencode 的閒置負載。輸出為可直接
  `diff` 的 `key: value` 純文字，同時涵蓋 `memory.events` 與 CPU throttling 欄位。

### Docs

- 新增整機凍結與 opencode 資源調查交接文件
  `docs/2026-08-08-system-freeze-opencode-resource-handover.md`，並以 2026-08-08
  的對照實驗追記修正其主要假設：`opencode serve` 的 14-25% idle CPU 不是 Bun 常駐
  runtime 的固有成本（同版本全新實例僅約 1%），而是隨回合累積且從不釋放的狀態。
  CLI 模式沒有同樣現象，因為 `opencode run` 每回合退出，累積在結構上不可能發生。
  修復規劃見
  `docs/superpowers/specs/2026-08-08-opencode-server-residency-remediation-design.md`。

## [0.19.0] - 2026-08-06

排程任務會停在沒人回答的權限詢問上，直到 20 分鐘 agent timeout 才失敗。本版修掉
造成這個結果的四個環節，並讓容器內的 opencode 設定能隨映像檔升級更新。事故分析見
`docs/2026-08-06-docker-runtime-handover.md`，修復規劃見
`docs/superpowers/specs/2026-08-06-opencode-permission-hang-remediation-design.md`。

### ⚠️ 升級注意（Breaking）

- **容器內的 `opencode.json` 改由 Wukong 管理，每次啟動覆寫。** 自訂設定請改寫在
  同目錄的 `user.json`（opencode 會深度合併兩者，`user.json` 的鍵勝出）。升級時若
  偵測到舊版 seed 出來的設定檔，會自動備份成 `opencode.json.pre-baseline.bak` 並把
  內容複製到 `user.json`，手改過的規則不會消失；想回到純預設就刪掉 `user.json`
  再重啟。這是為了讓新版預設能真的送達既有部署——舊的 seed-if-missing 邏輯導致
  v0.18.7 的 CPU 護欄從未套用到執行中的主機。
- **無人值守排程預設拒絕權限詢問。** 先前是既不允許也不拒絕（等到逾時）。若你的
  排程工作需要存取 `/tmp` 以外的外部目錄，請在 `user.json` 的
  `permission.external_directory` 逐項放行；或在信任 container 隔離的前提下設
  `WUKONG_SCHED_PERMISSION=allow`。

### Added

- 排程新增權限處置策略（`WUKONG_SCHED_PERMISSION`，預設 `reject`）。無人值守回合
  收到權限詢問時當場處置，結果寫入該次 run 訊息的 `[無人值守權限]` 區塊與 log；
  回覆失敗會重試 3 次後中止回合，而不是繼續等到 agent timeout。
- 容器內 opencode 設定改為 baseline（`opencode.json`，隨映像檔升級）+ 使用者層
  （`user.json`，由 `OPENCODE_CONFIG` 指向）兩層。
- 發版映像檔的 opencode 版本改由 CI 在發版當下解析成 npm 最新版並重新釘版（版本、
  integrity、lockfile 同步改寫）。需要卡版時設 repository variable
  `OPENCODE_VERSION_PIN`。

### Fixed

- Web Console 無法回覆 opencode 的權限詢問：帶 `permission-` 前綴的 request id 被
  當成一般 question id 送到 `/api/session/{id}/question/{id}/reply`，正確端點是
  `/permission/{id}/reply`，因此按下允許或拒絕都不會送達，該回合仍會等到逾時。
  端點分派邏輯上收到 `wukong-gateway`，Web、Telegram、CLI 共用同一條路徑。
- CLI 收到權限詢問時完全不顯示，畫面只會停住；改為顯示詢問內容並提示需改用
  Web Console 或 Telegram 回覆。
- 容器內 seed 的 `opencode.json` 補上 `external_directory` 放行 `/tmp`。該項 opencode
  預設為 `ask`，而 compose 走的是 `opencode serve`——`serve` 沒有
  `--dangerously-skip-permissions`，設定檔是 Web／Telegram／Scheduler 唯一的權限控制。
- 串流逾時與中斷的錯誤訊息附上卡住的原因（事件數、最後事件型別與距今秒數、出現過
  的待決 request id）。先前不論卡在哪裡都只回報 `before session became idle`，
  讀起來像模型逾時。

### Changed

- `wukong-scheduler` 的 `ExecutionContext` 新增 `permission_policy` 欄位，泛型參數
  多了 `QuestionResponder` bound（repo 內的使用端已同步；外部使用者需一併調整）。

## [0.18.7] - 2026-08-05

### Fixed

- session compact 失敗後會消耗本次 compaction 額度，下次重試間隔一個完整的
  `WUKONG_SESSION_COMPACT_EVERY_TURNS`。先前失敗不會重置 turn count，導致
  summarize 一旦持續失敗就會在**每一個**後續回合重試，而每次重試都是一次完整
  session 的 LLM 摘要呼叫，長對話下會讓 agent 端 CPU 居高不下。session 本身仍
  保留，暫時性錯誤不影響對話連續性。

### Changed

- 容器內 seed 的 `opencode.json` 新增長對話 CPU 護欄：關閉 opencode 的 workspace
  git snapshot（`snapshot: false`）、限制工具輸出大小（`tool_output`）、開啟
  opencode 自身的 context 修剪（`compaction`），並讓檔案監看略過建置產物
  （`watcher.ignore`）。opencode 會把每次串流更新以「完整快照」寫入 durable
  event log，回覆越長寫入放大越嚴重；這些設定用來壓低該放大效應的基數。

### 升級注意

- seed 只在 `opencode.json` 不存在時寫入。既有部署的 `opencode-config` volume
  已有該檔，需自行合併設定後重啟 `opencode-server`：

  ```bash
  docker exec wukong-opencode-server python3 - /home/wukong/.config/opencode/opencode.json <<'PY'
  import json,sys,os
  p=sys.argv[1]; c=json.load(open(p))
  c["snapshot"]=False
  c["compaction"]={"auto":True,"prune":True,"tail_turns":8}
  c["tool_output"]={"max_lines":500,"max_bytes":65536}
  c["watcher"]={"ignore":["**/node_modules/**","**/target/**","**/.git/**","**/dist/**","**/build/**"]}
  tmp=p+".tmp"; json.dump(c,open(tmp,"w"),indent=2,ensure_ascii=False); os.replace(tmp,p)
  PY
  docker compose restart opencode-server
  ```

- 關閉 `snapshot` 會同時停用 opencode 自身的 revert/undo；workspace 本身的 git
  歷史不受影響。

## [0.18.6] - 2026-08-04

### Added

- OpenCode server backend 現在使用原生 `POST /session/{id}/summarize` 與
  `DELETE /session/{id}`，並以 bounded message reads 取代無上限的 session history 讀取。
- 每 scope 新增持久 session lifecycle state、turn count、lease 與 memory session provenance；
  server-side planner、summarizer 與 helper session 會在完成後清理。
- schedulerd 新增 all-scope automatic memory maintenance，只會 consolidation event/note
  來源，不會自動刪除未折疊的低價值記憶。

### Changed

- 成功回合達到 `WUKONG_SESSION_COMPACT_EVERY_TURNS` 後，會在下一個 final turn 前嘗試
  compact；summarize 不支援時才建立 replacement session，暫時性錯誤保留原 session。

### 升級注意

- 新版預設會啟用 session compact 與 scheduler memory maintenance。如需保留舊行為，可在
  `.env` 加入：

  ```env
  WUKONG_SESSION_COMPACT_EVERY_TURNS=0
  WUKONG_MEMORY_AUTO_MAINTENANCE=0
  ```

## [0.18.5] - 2026-07-20

### Fixed

- OpenCode server 事件串流不再套用 20 分鐘的 reqwest 全域 timeout；長回合不會
  再於串流中途被切斷並回報難以診斷的「error decoding response body」。回合時限
  改由既有的 stream deadline 把關，逾時會回報明確訊息。
- 事件串流中斷後會主動呼叫 `POST /session/{id}/abort`，停止 server 端仍在執行
  的 prompt；避免殭屍 prompt 佔住 session 造成後續每一回合都逾時、必須重啟
  `opencode-server` 才能恢復。
- OpenCode server 的 HTTP 錯誤訊息現在會附上完整原因鏈（逾時、連線被 reset
  等），方便直接定位失敗原因。
- SSE 事件改以位元組緩衝、整行解碼，修正中文等多位元組字元跨 chunk 邊界時變成
  亂碼的問題。

## [0.18.4] - 2026-07-17

### Added

- Telegram 現在可將 `document` 與 `photo` 作為 OpenCode file part 傳入，並在同一
  session 內繼續追問；回覆先前的檔案訊息可重新帶入附件，上傳新檔並回覆舊檔可
  直接比較兩份內容。
- OpenCode 產出的檔案會從每回合專屬 artifact 目錄透過 Telegram `sendDocument`
  回傳；原始上傳、可操作工作副本與回傳成品分開保存。
- OpenCode server 的 `permission.asked` 事件會在 Telegram 顯示「允許一次」、
  「本次工作階段總是允許」與「拒絕」按鈕。

### Changed

- Docker 共用 workspace 時預設用 `file:///workspace/...` 傳送附件；沒有共享
  filesystem 的遠端 OpenCode server 可設定
  `WUKONG_AGENT_SERVER_FILE_MODE=inline`，以 Base64 data URL 傳送單檔不超過
  10 MiB 的附件。
- Telegram 上傳檔案限制為單檔 25 MiB、每則最多 5 份，並在傳給 OpenCode 前驗證
  canonical path、拒絕 symlink 及工作區外路徑。

### 升級注意

- Compose 部署不需額外操作，預設使用共享 `/workspace`。若
  `WUKONG_AGENT_SERVER_URL` 指向沒有掛載相同 workspace 的遠端服務，請在 `.env`
  設定 `WUKONG_AGENT_SERVER_FILE_MODE=inline`；否則只有附件請求會被拒絕，既有
  純文字對話不受影響。

## [0.18.3] - 2026-07-16

### Changed

- Telegram 私人聊天的 reasoning 與 tool use 改用原生 `sendMessageDraft`，以相同 draft
  漸進更新單一暫時區塊；問題、答案或錯誤送出時結束暫時進度。
- 群組或不支援 message draft 的環境會自動退回單一狀態訊息，並在問題或回合結束時清除。

## [0.18.2] - 2026-07-13

### Fixed

- Docker installer 在升級與 rollback 時會沿用既有 Compose project，從 release metadata 或現有 container labels 判斷 ownership，避免切換到空白 volumes 或因固定 container name 衝突而失敗。
- Docker upgrade 使用 staged release Compose 設定 pull images，並修復舊部署殘留 `build:` 設定時誤走 source build 或 pull failure 的問題。
- 同版本 `--upgrade` 會直接 no-op；release bundle 的 compatibility metadata 納入嚴格 archive allowlist。
- Stable release manifest 直接由 stable tag 產生，不再依賴 RC promotion metadata。
- Stable release 只對版本與 commit image tags 執行 immutable guard；`latest` 會更新到新 stable digest，避免 patch release 因既有 `latest` 而失敗。

### Changed

- Installer Phase 4：release manifest 現在包含 reviewed data-compatibility declaration，Binary rollback 已加入 release contract。
- Installer Phase 3：Docker 升級改為驗證 release manifest、全域 `SHA256SUMS` 與 GHCR digest 的 pull-only transaction，不再本機 build 或移除 volumes；只更新 release 擁有的部署檔。
- Binary 升級改為 staged、可重複執行且保留已選元件、設定與 workspace；`~/.wukong/install.json` 以 `0600` 原子寫入安裝 metadata，Linux 可用 `--with-schedulerd` 明確管理 Scheduler service。

## [0.18.0] - 2026-07-12

### Added

- 新增 `scripts/release.sh` 作為唯一維護者 release gate：驗證候選 commit、建立 annotated tag、監看 workflow，並驗證 GitHub Release channel 與 assets。
- Release workflow 在建置前驗證 annotated RC/stable tag 與 locked Cargo dependency graph。
- 新增 deterministic `release-manifest.json` 與 aggregate `SHA256SUMS` generator，供後續 GHCR 與 installer migration 使用。
- RC 與 stable 發佈會由 CI 建置的 musl binaries 產生 immutable `linux/amd64` GHCR image；product 與 commit tags 若已指向不同 digest 會拒絕覆寫。
- Stable tag 直接建置並發布 image、binaries 與 release assets，同時更新 GHCR `latest` tag。
- Docker release bundle 改為 pull-only Compose、`.env.example`、license、installer 與 release manifest 的最小內容，所有公開 release assets 由全域 `SHA256SUMS` 覆蓋。

## [0.17.1] - 2026-07-08

> 修復版：解決 v0.17.0 的 Web Console fail-closed 守門在 Docker Compose 預設下
> 導致升級後容器不斷重啟、`localhost:8787` 無法連線的問題。

### Fixed

- **docker**：修復 v0.17.0 的 Web Console fail-closed 守門在 Compose 預設下造成
  容器不斷重啟、`localhost:8787` 無法連線的問題。安全邊界改由 host 端 port
  mapping 控制：`docker-compose.yml` 預設只把 `wukong-web` 綁到 `127.0.0.1`
  （本機可達、不對區網開放），容器內固定綁 `0.0.0.0:8787` 並預設
  `WUKONG_WEB_ALLOW_INSECURE=1`。沿用舊 `.env`（無 token）升級後即可直接使用。
- **docker**：修復 `WUKONG_WEB_PORT` 覆寫時 host 埠與容器埠對不上的問題；此值
  現在只改 host 端映射埠，容器內固定聽 `8787`，healthcheck 同步固定為容器 `8787`。

### Changed

- **web**：不安全綁定（對外 + 空 token）由「`exit(1)` 崩潰」改為 fail-visible
  降級模式：照常綁定並對所有請求（含 `/healthz`）回 `503` 與修正說明頁，
  healthcheck 標記 unhealthy。避免 `restart: unless-stopped` 下的隱形重啟迴圈，
  使用者可直接在瀏覽器看到原因與解法。正確設定時行為不變。
- 新增 `WUKONG_WEB_BIND`（host 端綁定位址，預設 `127.0.0.1`）。要對區網開放請設
  `WUKONG_WEB_BIND=0.0.0.0` 並搭配 `WUKONG_WEB_TOKEN=<secret>`。

## [0.17.0] - 2026-07-07

> 本版收束 2026-07-07 的專案健康度改進批次（Phases 1–7 累積於 v0.16.35–38），
> 並含一項使用者可感知的相容性變更（移除 Intel Mac 預建二進位），故進 minor。

### ⚠️ 升級注意（Breaking）

- **Web Console 不安全綁定啟動即拒絕**：`wukong-web` 綁定非 loopback 位址
  （Docker 部署常見的 `0.0.0.0`）且 `WUKONG_WEB_TOKEN` 為空時會拒絕啟動；搭配
  `restart: unless-stopped` 會表現為容器不斷重啟、`localhost:8787` 無法連線。
  - **影響對象**：沿用舊 `.env`（未設 token）升級 Docker 部署者，以及照舊版快速
    開始操作的新安裝。
  - **解法（擇一）**：於 `.env` 設 `WUKONG_WEB_TOKEN=<secret>`（建議），或設
    `WUKONG_WEB_ALLOW_INSECURE=1`（僅限可信內網），再 `docker compose up -d`。
  - **診斷**：`docker compose logs wukong-web` 可見拒絕啟動的原因。
  - 註：下一版起 `docker-compose.yml` 已改為預設僅綁 `127.0.0.1`、開箱即用，
    無需上述設定即可存取（見 Unreleased）。

### Changed

- Release workflow 改為「並行建置 → 單一 `publish` job 統一上傳」，消除多個 matrix
  job 並行寫入同一 GitHub Release 造成的資產上傳競態。
- **（相容性）停止發佈 Intel Mac（`x86_64-apple-darwin`）預建二進位**，改為僅發佈
  Apple Silicon（`aarch64-apple-darwin`）。`scripts/install.sh` 於 Intel Mac 改給
  Docker／原始碼建置指引，而非下載不存在的資產。

## [0.16.38] - 2026-07-07

### Changed

- 純結構重構（無行為變更）：拆分兩個過大檔案，以測試守門。
  - `wukong-gateway/src/opencode_server.rs`（1401 → 640 行）抽出
    `opencode_server/sse.rs` 與 `opencode_server/event_map.rs`。
  - `wukong-web/src/lib.rs`（3772 → 2955 行）抽出 `static_assets.rs` 與 `chat_api.rs`。

## [0.16.37] - 2026-07-07

### Added

- 新增 `CHANGELOG.md`（Keep a Changelog）。
- `wukong-web` 新增免認證 `/healthz` liveness 端點與 docker-compose healthcheck。
- `Dockerfile` 新增 `ARG OPENCODE_VERSION`，可於 build 時 pin opencode-ai 版本。

### Changed

- `Dockerfile` 的 `ARG VERSION` 預設更新為 `v0.16.36`。

### Documentation

- 文件化 `opencode serve` backend 刻意不串流回答文字（收尾以 `list_messages`
  一次取回）之設計，與 CLI backend 的差異。

## [0.16.36] - 2026-07-07

### Changed

- 進入點膠水碼下沉至 `wukong-runtime`：`now_unix`、`upload_root`、`default_db_url`、
  `db_url_from_env`、`agent_command_from_env` 與 memory bootstrap 各只剩一份實作。
- 新增 `wukong-scheduler::run_claimed_job` 統一 CLI 與 daemon 的排程執行編排
  （`start_run → execute → finish_run → complete_claimed_job`），lease 語意不變。
- CLI `run_one` 改用 `render::StreamRenderer`；REPL 逐回合重載 settings，
  讓 `/set_models` 於下一題即生效。

### Removed

- 移除死碼 `wukong-gateway/src/pipeline.rs`（無 production 呼叫者，已被
  `wukong_runtime::run_turn` 取代）。

## [0.16.35] - 2026-07-07

### Added

- CI workflow（`fmt` / `clippy -D warnings` / `test`，push main 與 PR 觸發）；
  `Cargo.lock` 納入版控，release 建置全面 `--locked`。

### Fixed

- **render**：修正 `split_chunks` 的 CJK 分塊 panic 與 `<pre>` 標籤不平衡
  導致 Telegram 400；連結／圖片 URL scheme allowlist，擋掉 `javascript:` XSS。
- **gateway**：CLI backend 實際傳遞 `--agent` 旗標，修正規劃棒被靜默丟棄。
- **telegram**：傳輸層辨識 `ok:false` 錯誤並退避；token 輪替保留 offset 游標。
- **memory**：embedding 改走 `spawn_blocking` 避免阻塞 async runtime；
  修正向量召回被 recency 截斷；`touch`／`consolidate`／`delete` 改原子 `IN(...)` 更新；
  SQLite `busy_timeout` 降低鎖競爭。

### Security

- **web / memoryd**：認證改用 middleware 閘門並支援 `Authorization: Bearer`；
  不安全綁定（`0.0.0.0` + 空 token）啟動即拒絕（fail-closed，可用
  `WUKONG_WEB_ALLOW_INSECURE=1` 覆寫）；Telegram callback 加白名單檢查。

[Unreleased]: https://github.com/raybird/Wukong/compare/v0.18.4...HEAD
[0.18.4]: https://github.com/raybird/Wukong/compare/v0.18.3...v0.18.4
[0.18.3]: https://github.com/raybird/Wukong/compare/v0.18.2...v0.18.3
[0.18.2]: https://github.com/raybird/Wukong/compare/v0.18.0...v0.18.2
[0.18.0]: https://github.com/raybird/Wukong/compare/v0.17.1...v0.18.0
[0.17.1]: https://github.com/raybird/Wukong/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/raybird/Wukong/compare/v0.16.38...v0.17.0
[0.16.38]: https://github.com/raybird/Wukong/compare/v0.16.37...v0.16.38
[0.16.37]: https://github.com/raybird/Wukong/compare/v0.16.36...v0.16.37
[0.16.36]: https://github.com/raybird/Wukong/compare/v0.16.35...v0.16.36
[0.16.35]: https://github.com/raybird/Wukong/releases/tag/v0.16.35
