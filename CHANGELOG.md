# Changelog

本專案的所有重要變更都會記錄在此檔。

格式依循 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版本號採用 [Semantic Versioning](https://semver.org/lang/zh-TW/)（實際發佈 tag 為 `v0.16.x`）。

> 本 CHANGELOG 自 `v0.16.35` 起維護；更早的版本紀錄請見
> [GitHub Releases](https://github.com/raybird/Wukong/releases) 與 `git log`。
> 維護方式：每次發佈前，把 `Unreleased` 區塊的條目整理為新版本區塊並標上發佈日期。

## [Unreleased]

## [0.21.3] - 2026-08-13

### Changed

- Memoria runtime image 從 **2.15 GB 降到 1.26 GB**（-41%），並跟進 Memoria **1.27.0**。
  - 兩個缺陷都出在 layer 的語意：layer 記錄的是它結束當下的檔案狀態，所以**後面的
    `rm` 不會讓 image 變小**（被刪的位元組留在前面的 layer），而**後面的 `chmod -R`
    更糟**——改動每個檔案的 metadata 會把整棵樹複製進新 layer。`docker history` 實測：
    刪除層 6.78 kB（一個位元組都沒回收，而它的註解宣稱省了 ~200 MB），chmod 層 899 MB，
    是它上面那個 889 MB 安裝層的完整複本。
  - 那句「~200 MB off the image」是隨 v0.21.0 出貨的假註解——**它宣稱了一件從未發生
    的事，而且沒有任何測試會為此紅燈，因為斷言的對象是一段散文**。要驗證這類敘述只能
    去問產物（`docker history`），不能問文件。
  - 所有寫入 `/opt/memoria` 的步驟折進單一 `RUN`，刪除與 chmod 因此真正生效。代價是
    build cache 粒度變粗，但這個 image 只在 `MEMORIA_VERSION` 變動時重建。
  - `scripts/test-memoria-runtime.sh` 的預設 image tag 改為從 overlay 推導，不再寫死。
    寫死的 tag 會在版本一變就過期，而測試會轉去驗證一個沒有人部署的 image。
  - 順帶記錄實測的體積分佈，供未來壓縮參考：`onnxruntime-node` 513 MB、
    `onnxruntime-web` 130 MB（瀏覽器 WASM build，Node 下永不載入，但是
    `@huggingface/transformers` 的 `dependencies` 硬相依）、transformers 146 MB
    （含模型快取 130 MB）、`better-sqlite3` 僅 12 MB。

### Fixed

- **installer 會在執行中被自己的新版覆寫，導致 bash 解析錯位。** `scripts/install.sh`
  一直都在 `DOCKER_RELEASE_OWNED` 裡，而 bash 是按位元組偏移量惰性讀取腳本的——檔案在
  執行途中被換掉後，之後每次讀取都落在新檔案的舊偏移量上，bash 就會執行到那裡的任意片段。
  - v0.20.0 到 v0.21.0 的 `install.sh` 三版位元組完全相同（30068 bytes），偏移量不變，
    所以這個問題藏了三個版本沒有發作。v0.21.1（30965）與 v0.21.2（33159）改變了長度，
    升級 v0.21.1 → v0.21.2 時實際冒出 `line 603: gv[2]: command not found`——那是
    heredoc 裡 `sys.argv[2]` 的碎片。
  - 那一次的損害只是最後一行完成訊息被吃掉，但**落點完全取決於兩版之間的長度差**。
    新增的回歸測試就示範了更糟的情形：在測試條件下同樣的機制造成
    `syntax error near unexpected token ')'`，installer 直接失敗退出。
  - 修法是從私有副本重新執行（`exec bash "$copy"`），磁碟上的檔案之後怎麼變都不影響
    正在跑的行程。腳本本身沒有使用 `$0` 或 `BASH_SOURCE`，所以副本完全等價。
    把整個腳本包進 `main()` 不足以解決——`main "$@"` 回來之後 bash 仍會嘗試讀下一個
    指令，一樣落在錯位的偏移量上。
  - 回歸測試（`test-installer-upgrade.sh docker-self-replace`）有兩個容易做錯的地方，
    做錯就什麼也測不到：padding 必須插在檔案**開頭**（只有讀取位置**之前**的變動才會
    讓後面整體位移，附加在檔尾沒有效果），而且必須執行**部署目錄自己那份** installer
    （測試原本跑的是 repo 的副本，兩個路徑不同，永遠不會相撞）。

## [0.21.2] - 2026-08-13

### Fixed

- **既有部署無法升級到 v0.21.1。** installer 對 release bundle 的內容驗證是**完全相等**
  比對，所以任何在 bundle 增減檔案的版本，都會讓所有更早的部署在驗證階段中止——而能夠
  接受新 bundle 的 installer，只能從那個被拒絕的 bundle 裡送達。v0.21.0 加了五個檔案，
  第一次觸發了這個死結。
  - Docker bundle 的驗證改為「必要檔案至少存在」而非「完全相等」。保護解壓的檢查完全
    未變：絕對路徑、`..`、symlink／hardlink、非一般檔案一律拒絕；owned 清單裡的每個
    檔案仍必須存在，缺一就在拉映像檔前中止。
  - 放寬對完整性沒有損失：`verify_sha256sums_entry` 在此之前已把整個歸檔逐位元組比對過
    release 的 `SHA256SUMS`，所以「非預期」的成員只可能是我們自己發布的檔案。
  - 單一 binary 的 tarball 仍維持完全相等比對——那裡「一個歸檔、一個預期檔案」是對的語意。
  - 新增兩項測試：帶有未知額外檔案的 bundle **必須**能安裝（模擬未來版本），缺少 owned
    檔案的 bundle **必須**中止。前者在改動前會紅燈，已驗證。

> ⚠️ **v0.21.1 之前的部署需要一次性手動步驟。** 修正本身也只能透過 bundle 送達，所以
> 舊 installer 仍會拒絕它。請先從 bundle 取出新的 installer 再升級：
>
> ```bash
> curl -fsSL -o /tmp/b.tar.gz \
>   https://github.com/raybird/Wukong/releases/download/v0.21.2/wukong-docker-v0.21.2.tar.gz
> tar -xzf /tmp/b.tar.gz -C /tmp wukong-docker/scripts/install.sh
> cp /tmp/wukong-docker/scripts/install.sh scripts/install.sh
> bash scripts/install.sh --upgrade --version v0.21.2
> ```
>
> 這是最後一次需要這麼做；之後往 bundle 加檔案不會再破壞升級路徑。

## [0.21.1] - 2026-08-13

### Fixed

- **v0.21.0 無法透過 `install.sh` 安裝或升級。** 該版把 Memoria overlay 的五個檔案加進
  了 release bundle，卻沒有同步 `install.sh` 的清單。
  - `safe_list_archive` 是**完全相等**比對，不是「至少包含」。v0.21.0 的允許清單有六項、
    bundle 內有十一個檔案，所以 `install.sh` 會拒絕它自己的 bundle，以
    `unsafe or unexpected archive contents` 中止——發生在拉映像檔之前，**與有沒有啟用
    overlay 無關，所有使用者都受影響**。
  - 就算通過驗證也還有第二層問題：複製迴圈只走 `DOCKER_RELEASE_OWNED`，overlay 檔案不會
    落到磁碟上；而一起安裝的 `.env.example` 會叫使用者設
    `COMPOSE_FILE=docker-compose.yml:docker-compose.memoria.yml`，那會讓**每一個**
    compose 指令都失敗。
  - overlay 的五個檔案已納入 `DOCKER_RELEASE_OWNED`（因此也一併受既有的備份／回滾保護），
    `validate_archive_entries` 改為由該清單推導，兩層問題一次解決。
  - `test-installer-upgrade.sh` 的 fixture 補上這些檔案。先前的 fixture 只造六個檔案，
    形狀與真實 bundle 不同，所以測試全綠卻放行了一個裝不起來的版本——這正是 v0.21.0
    逃掉的原因。
  - `test-docker-runtime.sh` 原本用字面字串比對 `DOCKER_RELEASE_OWNED` 的單行寫法，
    改為逐項檢查成員。字面比對讓排版變更看起來像契約破壞，更糟的是清單一長它就不再
    斷言任何事——v0.21.0 加了五個檔案，這道檢查既沒察覺、也不需要跟著改。
  - `test-release-workflow.sh` 新增守門：release.yml 放進 bundle 的每個檔案，都必須
    要嘛被 `install.sh` 安裝、要嘛在測試中明列為「刻意不安裝」。這與 v0.20.0 是同一類
    錯誤——檔案從一份策展清單中安靜消失——只是發生在流程的下一段。原本的
    `test-release-image.sh context` 只驗 `Dockerfile.release` 的 COPY 來源，涵蓋不到
    installer 這份清單。

## [0.21.0] - 2026-08-13

### Added

- 發布流程新增兩道能真正擋下 v0.20.0 那類問題的檢查。原本的斷言都只是靜態文字比對，
  而且比對的是開發用 `Dockerfile`，所以測試全綠卻放行了一個功能完全失效的版本。
  - `test-release-image.sh context`（已納入預設 `all`，發版 preflight 會跑）：解析
    `Dockerfile.release` 的每一個 `COPY` 來源，逐一確認該路徑在 repo 內存在、且
    `release.yml` 確實把它複製進 `release-context/`。發布用的 build context 是逐檔
    組出來的，漏列的檔案不會讓 build 失敗，只會安靜地從映像檔消失。
  - `test-release-image.sh smoke` 改為檢查映像檔的實際內容：entrypoint 與 idle-restart
    supervisor 以絕對路徑確認存在且可執行，四支 binary 加 `opencode`、`agent-reach` 以
    `command -v` 確認可在 PATH 上解析（涵蓋符號連結與 pipx 安裝位置），並確認 tzdata
    已安裝。全部收斂成單一容器啟動，耗時約 0.2 秒。原本逐支跑 `--help` 要數分鐘，卻
    證明不了比「能被解析」更多的事——v0.20.0 壞的是檔案不存在，不是 binary 有問題。
    `release.yml` 會在**貼上任何公開 tag 之前**對剛建好的映像執行它，因此壞掉的映像
    不會取得公開 tag；另有斷言鎖住這個先後順序。

- 選用的 Memoria 記憶層：容器內的 agent 可以擁有跟 host 一樣的 `memoria` CLI（含語意召回）。
  預設不啟用，在 `.env` 設 `COMPOSE_FILE=docker-compose.yml:docker-compose.memoria.yml` 開啟。
  - 走 runtime volume 而非併進 Wukong image，兩者的發版節奏因此解耦——升級只要改
    `.env` 的 `WUKONG_MEMORIA_VERSION`，不必重建 Wukong image。
  - 純加法：資料 volume 的權限交接由 publisher 容器（本來就是 root）處理，而不是改
    Wukong 的 entrypoint，所以現有已發布的 image 直接就能用，不需要重建或升級。
  - 必須是 CLI 而不是 sidecar：Memoria 的 HTTP API 沒有 `brief` 的端點，而那是
    host workflow 每次開場要讀的東西。（`feedback` 有——`POST /v1/recall/:id/outcome`，
    只是 CLI 與 HTTP 命名不同。）
  - 容器記憶存在獨立的 `memoria-data` volume，與 host 的 `~/.memoria` 無關。
  - `memoria-vector-sync` 把 Memoria `OPERATIONS.md` 的三步 ingest 包成一個指令。少了它
    向量表是空的，而 `--mode vector` 仍會回傳字面召回的結果、看起來像正常運作。
  - `memoria` 包了一層 flock 並行閘門（`WUKONG_MEMORIA_VECTOR_MAX_CONCURRENCY`，預設 1）。
    Memoria 上游沒有並行上限（issue-8），每個 helper 峰值 450–624 MB，兩三個同時進來就
    足以打爆 agent 容器。被擋下的那次會退回字面召回並在 stderr 明講，不靜默降級。
  - `scripts/test-memoria-runtime.sh`：把 runtime 倒進真的 volume、掛進真的 Wukong image，
    跑 agent 真正會下的指令。守的是 ABI 配對——`better-sqlite3` 用 ABI 專屬 prebuild，
    build 期不會察覺不合，只在 agent shell 裡炸 `NODE_MODULE_VERSION`。
  - 上面那支要 build 2.2 GB 的 image，太重、不適合每次發版跑，所以
    `test-release-image.sh smoke` 另外加了一條零成本的斷言：發布映像檔的 node 主版號
    必須等於 `docker-compose.memoria.yml` 的 `WUKONG_MEMORIA_NODE_MAJOR` 預設值。
    base image 換 Debian 版本（node 18 → 20）就是靠這條擋下來，否則所有啟用 overlay
    的部署會靜默壞掉。

### Changed

- 修掉記憶層與 Web 層的效能瓶頸。以 20,000 筆記憶（含 embedding）實測：寫入
  26.6 秒 → 8.8 秒，帶 scope 的 hybrid recall 單次 554 ms → 96 ms。
  - `memories` 表除了 `dedupe_key` 之外沒有任何索引，`recent_candidates`、
    `embedded_candidates`、`list_records`、`rows_missing_embedding` 全都在做全表掃描
    加 temp B-tree 排序。補上 `created_at`、`(scope, created_at, id)` 兩個索引，以及
    `embedding IS NOT NULL` / `IS NULL` 兩個 partial index。後者尤其重要：backfill
    每批 32 筆，之前每次都要重新掃過愈來愈長的已嵌入前綴。
  - scope 過濾從「撈完再濾」改成下推到 SQL。之前 fetch limit 會被其他 scope 的資料
    吃掉——共用 DB 上每個 Telegram 對話各佔一個 scope，這個浪費會隨使用者數放大。
  - 向量召回不再把每一列的 embedding BLOB 解成 `Vec<f32>`。新增
    `cosine_similarity_blob` 直接在 bytes 上算，省下每列一次配置。
  - `list_records` 之前為了判斷 `has_embedding` 把整個 BLOB 撈回來；`snapshot` 為了
    算 prune 數量把所有 id 撈回來再取 `len()`。改為 SQL 層的 `IS NOT NULL` 與 `COUNT(*)`。
  - 兩個 SQLite store 在 WAL 下改用 `synchronous=NORMAL`，不再每次 commit 都 fsync；
    `wukong-chat-history` 先前完全沒設定 WAL 與 busy_timeout。
  - Web 每個 chat API 請求（含 SSE 迴圈與 turn 執行緒）都會 `ChatHistoryStore::open`
    一次，等於重建連線池並重跑整份 schema DDL。改為在 `AppState` 持有共用 store。

## [0.20.1] - 2026-08-08

修正 v0.20.0 的封裝疏漏：**該版的 opencode 閒置自動重啟完全不會運作。**

### Fixed

- **`opencode-idle-restart.sh` 沒有被打包進 v0.20.0 的映像檔。** 發布用的映像是以
  `Dockerfile.release` 搭配 `release.yml` 裡逐檔複製的 build context 建置的，而
  v0.20.0 只更新了開發用的 `Dockerfile`；`.dockerignore` 的 `scripts/*.sh` 也一併把它
  擋掉了。三處都沒有列到這個檔案，於是它安靜地從映像檔消失——**build 不會失敗、容器
  照常啟動、opencode 照常服務，只有自動重啟從未發生**。已補齊 `Dockerfile.release`
  的 `COPY`、`release.yml` 的 context 複製與 `.dockerignore` 的重新納入。
- entrypoint 在找不到 supervisor 時改為明確警告。先前是以背景工作啟動，缺檔的錯誤
  被 `&` 吞掉，沒有任何可見症狀。
- `Dockerfile.release` 補上 `tzdata`（先前只有開發用 `Dockerfile` 有）。v0.20.0 的
  映像剛好由相依套件間接帶進 tzdata 才沒出事，但那不是能依賴的行為。
- `test-docker-runtime.sh` 改為對 `Dockerfile`、`Dockerfile.release` 與
  `release.yml` 的 context 複製三處同時斷言。原本的斷言只檢查開發用 `Dockerfile`，
  所以測試全綠卻放行了一個功能完全失效的版本。

## [0.20.0] - 2026-08-08

> ⚠️ 本版的 opencode 閒置自動重啟因封裝疏漏而不會運作，請直接使用 v0.20.1。
> 其餘變更（cgroup 上限、healthcheck、`stop_signal`）在本版均正常。

`opencode-server` 長期維持 14-25% idle CPU。本版以同版本 opencode 的對照實驗查出
那不是 Bun 常駐 runtime 的固有成本——同版本、同量級 DB 的全新實例只有約 1%——而是
常駐程序內隨回合累積且從不釋放的狀態。CLI 模式沒有這個現象，不是因為它比較有效率，
而是 `opencode run` 每回合退出，累積在結構上不可能發生；差異在程序生命週期。本版為
server 模式補回那個 CLI 免費獲得的週期性重置，同時保留暖啟動的低延遲優勢，並為所有
容器補上先前完全缺席的 cgroup 硬上限。調查見
`docs/2026-08-08-system-freeze-opencode-resource-handover.md`，規劃見
`docs/superpowers/specs/2026-08-08-opencode-server-residency-remediation-design.md`。

### ⚠️ 升級注意

- **所有容器都加上了 cgroup 硬上限。** 先前四個 service 全是無限制狀態，任一 agent
  回合或工具都能用盡整台主機的 CPU、記憶體與 PID。預設值：`opencode-server` 與
  `cli` profile 為 `cpus 1.5` / `mem_limit 2g` / `pids_limit 256`，其餘 service 為
  `0.5` / `768m` / `128`。要調整請改 `.env` 的 `WUKONG_OPENCODE_*` 與
  `WUKONG_SVC_*`，**不要直接編輯 `docker-compose.yml`**——它由 release bundle 擁有、
  升級時會被覆寫，手改會無聲消失。另外請注意 `mem_limit` 引入了一種原本不存在的
  失敗模式：容器可能被 OOM kill 再由 `restart` 拉起，進行中的回合會遺失。

### Changed

- **`opencode-server` 改用 `SIGINT` 關閉。** opencode 只註冊 SIGINT 處理常式、沒有攔
  SIGTERM，而核心會丟棄送給 PID 1 的預設動作訊號——所以先前每次 `docker stop` 或
  `docker compose restart` 的 SIGTERM 都被忽略，空等 10 秒寬限期後才被 Docker 從外部
  SIGKILL，SQLite WAL 沒有乾淨 checkpoint 的機會。改設 `stop_signal: SIGINT` 後約
  0.1 秒乾淨退出。
- `opencode-server` 的 healthcheck 從每 2 秒改為每 30 秒，啟動期則以
  `start_interval: 2s` 維持快探。2 秒間隔每天在容器 cgroup 內 fork 約 43,200 次
  shell+curl，那些 CPU 會計入 `docker stats` 而被誤讀成 opencode 自身的閒置負載。
  改用 `start_interval` 是因為 web／telegram／schedulerd 都以 `depends_on` 等待
  server 健康，單純放寬 `interval` 會讓每次啟動多等最多 30 秒。需 Docker Engine 25.0+。

### Added

- **`opencode-server` 會在離峰時段閒置時自行重啟。** `opencode serve` 常駐不死，每回合
  的殘留（heap、快取、DB handle）全部留存，idle CPU 隨累積工作量上升；CLI 模式沒有這個
  問題，因為 `opencode run` 每回合退出、等於免費獲得重置。新增的 supervisor 在
  `WUKONG_OPENCODE_RESTART_WINDOW`（預設 `03:00-05:00`）內、且三項條件同時成立時讓
  server 自行退出並由 `restart` 拉起：無近期 session 更新、對外埠無 `ESTABLISHED`
  連線、`opencode.db` 已停止寫入。窗口內始終不閒置就跳過等隔天，**不會強制中斷進行中
  的回合**。設為空字串可完全停用。
  窗口是容器本地時間，compose 預設 `TZ=Asia/Taipei`（映像檔因此加裝 `tzdata`；
  缺它時 `TZ` 會靜默退回 UTC）。不在 +08:00 的部署請在 `.env` 覆寫 `TZ`。
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
