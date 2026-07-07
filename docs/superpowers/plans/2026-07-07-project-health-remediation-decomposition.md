# Implementation Plan Decomposition

> 來源計畫：`docs/superpowers/plans/2026-07-07-project-health-remediation.md`（2026-07-07 專案健康度改進計畫）
> 拆解原則：每個 Task 控制在 1～3 小時、可獨立完成與驗收；Phase 代表開發里程碑。原計畫中超出粒度的 Task 3／4／6／8／13 已於此細切。
> 執行方式：以 `execute-task` 指定 Phase / Task 執行；每個 Task 完成即為一個 commit 邊界，完成後執行 `cargo test --workspace && cargo clippy --all-targets -- -D warnings`。

## 原計畫任務對照

| 原計畫 Task | Decomposition |
|---|---|
| Task 1（CI） | 1.1 |
| Task 2（Cargo.lock） | 1.2 |
| Task 3（render 三修復） | 2.1、2.2、2.3 |
| Task 4（認證加固） | 3.1、3.2、3.3、3.4 |
| Task 5（agent 欄位） | 4.1 |
| Task 7（Telegram 韌性） | 4.2、4.3 |
| Task 6（memory 效能） | 5.1、5.2、5.3、5.4 |
| Task 12（confidence） | 5.5 |
| Task 8（膠水下沉） | 6.1、6.2、6.3、6.4、6.5 |
| Task 11（pipeline.rs 死碼） | 6.6 |
| Task 9（版本與文件） | 7.1、7.2 |
| Task 10（HEALTHCHECK） | 7.3 |
| Task 13（大檔拆分） | 7.4、7.5、7.6、7.7 |

---

## Phase 1 — CI 安全網與可重現建置

### Goal

建立自動化品質守門（fmt / clippy / test）與可重現的 release 建置，為後續所有修改提供回歸保護。

### Deliverables

* `.github/workflows/ci.yml` 於 push（main）與 PR 觸發並守門
* `Cargo.lock` 納入版控，release 建置帶 `--locked`

### Dependencies

* 無（本 Phase 為所有後續 Phase 的前置）

### Tasks

#### Task 1.1 建立 CI workflow

* 任務說明：新增 `ci.yml`，含 `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace` 三段，加 `Swatinem/rust-cache@v2` 快取；預設不開 `embed` feature。推分支驗證三段皆綠，並以一次故意失敗確認會紅。
* 預期輸出：PR 上可見 CI 狀態檢查；main push 亦觸發。
* 涉及模組或檔案：Create `.github/workflows/ci.yml`
* 預估：1～2 小時

#### Task 1.2 `Cargo.lock` 納入版控

* 任務說明：自 `.gitignore` 移除 `Cargo.lock` 並將現有 lockfile 入庫；檢查 `release.yml` 建置指令補上 `--locked`。
* 預期輸出：`git ls-files | grep Cargo.lock` 有輸出；release workflow 使用鎖定版本建置。
* 涉及模組或檔案：Modify `.gitignore`、`.github/workflows/release.yml`
* 預估：0.5～1 小時

---

## Phase 2 — render 高危修復

### Goal

消除 `wukong-render` 三個可實際觸發的缺陷：CJK 分塊 panic、分塊標籤不平衡導致 Telegram 400、`javascript:` 連結 XSS。

### Deliverables

* 超長中文輸出經 Telegram 路徑不 panic、每塊標籤平衡
* Web／Telegram 渲染皆過濾危險 URL scheme，附測試守護

### Dependencies

* Phase 1（CI 守門後進行；與 Phase 3 無依賴，可平行）

### Tasks

#### Task 2.1 修復 `split_chunks` CJK char boundary panic

* 任務說明：`lib.rs:164` 的 `split_at(max)` 以位元組切割會在多位元組字上 panic。失敗測試先行（>4096 bytes 無換行中文段落），切點以 `is_char_boundary` 向前回退至安全邊界（勿用 nightly API）。
* 預期輸出：新測試通過（各塊為合法 UTF-8 且 ≤ 上限）；既有 render 測試不回歸。
* 涉及模組或檔案：Modify `crates/wukong-render/src/lib.rs`
* 預估：1～2 小時

#### Task 2.2 分塊標籤平衡

* 任務說明：分塊時追蹤未閉合的 Telegram 標籤（`b/i/s/u/code/pre/blockquote/a`）堆疊，塊尾補閉合、次塊開頭重開，避免長 `<pre>`／`<blockquote>` 被切開後 Telegram 回 400。失敗測試先行（超長 `<pre>` 區塊）。
* 預期輸出：任一輸入分塊後每塊標籤自我平衡，附驗證測試。
* 涉及模組或檔案：Modify `crates/wukong-render/src/lib.rs`
* 依賴：Task 2.1（同函式區域，序列進行避免衝突）
* 預估：2～3 小時

#### Task 2.3 URL scheme allowlist

* 任務說明：`to_web_html`（及 `to_telegram_html` 同步套用）於 pulldown-cmark 事件層攔截 `Tag::Link`／`Tag::Image`，allowlist 取 `http/https/mailto` 與相對路徑，不合格者降級為純文字。失敗測試先行（`javascript:`、`data:`）。
* 預期輸出：危險 scheme 不產生 `href`；合法連結保留；附測試。
* 涉及模組或檔案：Modify `crates/wukong-render/src/lib.rs`
* 預估：1～2 小時

---

## Phase 3 — 認證與部署加固

### Goal

將 Web Console 與 memoryd 的認證改為 fail-closed 且結構性防漏（middleware 統一），修補次要注入與白名單缺口。

### Deliverables

* Web 認證由 middleware 統一套用，支援 Bearer header
* 空 token + 非 loopback 時拒絕啟動；memoryd 具 token 驗證
* compose 與文件的安全警示到位

### Dependencies

* Phase 1（與 Phase 2 可平行）

### Tasks

#### Task 3.1 Web 認證 middleware 化

* 任務說明：把逐 handler 的 `authorized()`（`web/lib.rs:406-411`）抽為 axum middleware／extractor 統一掛在 router；同時支援 `Authorization: Bearer` header，保留 query token 相容並於文件標註棄用。
* 預期輸出：所有路由經單一認證層；既有 web 測試調整後通過。
* 涉及模組或檔案：Modify `crates/wukong-web/src/lib.rs` 及各 `*_api.rs`
* 預估：2～3 小時

#### Task 3.2 Web 空 token fail-closed

* 任務說明：`WUKONG_WEB_TOKEN` 為空且綁定非 loopback 時拒絕啟動，除非設 `WUKONG_WEB_ALLOW_INSECURE=1`；更新 `docker-compose.yml` 註解與 `docs/docker.md` 警示。
* 預期輸出：不安全組合啟動即失敗並有明確錯誤訊息；附啟動檢查測試。
* 涉及模組或檔案：Modify `crates/wukong-web/src/main.rs`、`crates/wukong-web/src/lib.rs`、`docker-compose.yml`、`docs/docker.md`
* 依賴：Task 3.1（同檔案，序列進行）
* 預估：1 小時

#### Task 3.3 memoryd 認證與預設綁定

* 任務說明：新增 `WUKONG_MEMORY_TOKEN` bearer 驗證 middleware；預設綁定改 `127.0.0.1`（env 可覆寫）；`tests/http.rs` 補 401／200 測試。
* 預期輸出：未帶 token 回 401；正確 token 正常服務；預設不對外。
* 涉及模組或檔案：Modify `crates/wukong-memoryd/src/lib.rs`、`crates/wukong-memoryd/src/main.rs`；Test `crates/wukong-memoryd/tests/http.rs`
* 預估：1～2 小時

#### Task 3.4 次要注入與白名單修補

* 任務說明：index.html 的 token 注入改 JSON 序列化嵌入（覆蓋 `</script>` 案例，`web/lib.rs:83-87`）；Telegram callback query 入口補白名單檢查（`telegram/main.rs:191-193`、`dispatch.rs:402`）並附測試。
* 預期輸出：兩處缺口關閉，附對應測試。
* 涉及模組或檔案：Modify `crates/wukong-web/src/lib.rs`、`crates/wukong-telegram/src/main.rs`、`crates/wukong-telegram/src/dispatch.rs`
* 預估：1 小時

---

## Phase 4 — 進入點行為修正

### Goal

修復 CLI backend 丟棄規劃意圖的行為分歧，補齊 Telegram 傳輸層的錯誤處理與退避。

### Deliverables

* CLI backend 實際傳遞 agent 參數，兩 backend 行為一致
* Telegram token 失效有退避與明確 log；送出失敗可觀察

### Dependencies

* Phase 1

### Tasks

#### Task 4.1 CLI backend 傳遞 `agent` 欄位

* 任務說明：查 `opencode run --help` 確認旗標名（預期 `--agent`）；失敗測試先行（`assemble_argv` 帶 `Some("plan")` 時 argv 含旗標、`None` 不含）；`run`／`run_streaming` 兩路徑皆生效。以假 agent 指令驗證整體流程不回歸。
* 預期輸出：規劃棒實際以 plan agent 執行；新測試通過。
* 涉及模組或檔案：Modify `crates/wukong-gateway/src/backend.rs`
* 預估：1 小時

#### Task 4.2 Telegram API `ok:false` 錯誤化與退避

* 任務說明：`get_updates`／`send_message` 檢查回應 `ok` 欄位，`false` 轉為帶 `error_code`／`description` 的 `Err`（mock 失敗測試先行）；主迴圈對此類錯誤套用既有 3s 退避，401 額外輸出 token 失效提示 log。
* 預期輸出：失效 token 下無忙迴圈、log 可診斷。
* 涉及模組或檔案：Modify `crates/wukong-tg-client/src/client.rs`、`crates/wukong-telegram/src/main.rs`
* 預估：1～2 小時

#### Task 4.3 送出錯誤 log 化與 offset 保留

* 任務說明：全部 `let _ = client.send_*`（`dispatch.rs:902,919,1214,1217`、`schedulerd/notify.rs`）改為記 log，不改控制流；token 輪替時保留 offset 不歸零（`telegram/main.rs:128`）並加註解。
* 預期輸出：送出失敗於 log 可見；token 輪替不重複處理舊 update。
* 涉及模組或檔案：Modify `crates/wukong-telegram/src/dispatch.rs`、`crates/wukong-telegram/src/main.rs`、`crates/wukong-schedulerd/src/notify.rs`
* 依賴：Task 4.2（send 錯誤語意先確立）
* 預估：1 小時

---

## Phase 5 — memory 效能、一致性與計分

### Goal

消除 embedding 對 async executor 的阻塞與向量召回的 recency 截斷，修正寫入一致性與 confidence 退化。

### Deliverables

* embedding 走 `spawn_blocking`；向量召回不漏舊而相關的記憶
* 批次寫入具原子性；confidence 在退化情形有合理語意

### Dependencies

* Phase 1（與 Phase 3／4 可平行；全程在 `wukong-memory` 內，與其他 Phase 檔案不重疊）

### Tasks

#### Task 5.1 embedding 改走 `spawn_blocking`

* 任務說明：`remember`（`lib.rs:169`）、`recall`（`lib.rs:270`）與 backfill `embed_batch` 的同步 ONNX 推論包進 `tokio::task::spawn_blocking`；確認 `Embedder` trait bounds（`Send + Sync`，必要時 `Arc` 持有）。
* 預期輸出：async 路徑不再被 CPU 推論阻塞；既有測試全綠。
* 涉及模組或檔案：Modify `crates/wukong-memory/src/lib.rs`、`crates/wukong-memory/src/embed/mod.rs`
* 預估：1～2 小時

#### Task 5.2 向量召回去 recency 截斷

* 任務說明：失敗測試先行（舊而語意相關 vs 新而不相關）；候選選取改為不依 recency 截斷（全量掃描 + 上限保護；ANN／sqlite-vec 另立設計文件，本任務不做）。
* 預期輸出：語意相關的舊記憶可被召回，附測試守護。
* 涉及模組或檔案：Modify `crates/wukong-memory/src/store/mod.rs`、`crates/wukong-memory/src/lib.rs`；Test `crates/wukong-memory/tests/integration.rs`
* 預估：2～3 小時

#### Task 5.3 N+1 批次化與 transaction

* 任務說明：`touch_recalled`（`store/mod.rs:234-247`）、`mark_consolidated`（`:324-333`）、`delete_memories`（`:397-407`）改 `IN (...)` 單句或 transaction 包裹；`mark_consolidated` 與 summary 寫入同一 transaction。
* 預期輸出：批次操作具原子性；中途失敗不留半套狀態。
* 涉及模組或檔案：Modify `crates/wukong-memory/src/store/mod.rs`、`crates/wukong-memory/src/consolidate.rs`
* 預估：1～2 小時

#### Task 5.4 連線池寫入序列化

* 任務說明：`Store::open`（`store/mod.rs:98-106`）設 `max_connections(1)` 或明確 `busy_timeout`；補「backfill 與前景 remember 並發」測試。
* 預期輸出：並發寫入無 `SQLITE_BUSY`；附並發測試。
* 涉及模組或檔案：Modify `crates/wukong-memory/src/store/mod.rs`；Test `crates/wukong-memory/tests/integration.rs`
* 依賴：Task 5.1（backfill 執行模型先定案）
* 預估：1 小時

#### Task 5.5 confidence 退化修正

* 任務說明：單候選時 `lexical_norm` 恆 1.0（`recall/mod.rs:156-160`）改 bm25 絕對值映射或保守值；recency-only 恆 0 改保守非零值或於 `RecallExplanation` 標記來源。失敗測試先行。執行前先確認 `2026-07-05-memory-optimization-parity` 計畫的 confidence 任務狀態，勿重工。
* 預期輸出：兩種退化情形有合理 confidence 且有測試守護。
* 涉及模組或檔案：Modify `crates/wukong-memory/src/recall/mod.rs`、`crates/wukong-memory/src/lib.rs`
* 依賴：Task 5.2（同檔案區域，序列進行）
* 預估：2 小時

---

## Phase 6 — 共用化與死碼清理

### Goal

把跨進入點複製貼上的膠水程式碼下沉至共用位置，移除 v1 遺留死碼，消除「一改多處」的維護風險。

### Deliverables

* `now_unix`／`upload_root`／settings 套用／memory bootstrap 各只剩一份實作（依賴方向限制者除外）
* 排程執行編排收斂至 `wukong-scheduler`；`pipeline.rs` 死碼移除

### Dependencies

* Phase 3 與 Phase 4（本 Phase 大量觸碰 `web/lib.rs`、`telegram/dispatch.rs`，待前兩者行為修改落地後再重構，避免衝突）

### Tasks

#### Task 6.1 建立共用 util 模組

* 任務說明：於 `wukong-runtime` 新增 `util.rs`／`bootstrap.rs`：`now_unix`、`upload_root`、`apply_settings_to_config`、`open_memory_from_env`（含 embed feature 透傳），附單元測試；本任務不改任何呼叫端。底層 crate（`wukong-memory`／`wukong-scheduler` 等）因依賴方向不可倒灌，保留本地副本並加註。
* 預期輸出：共用模組與測試就緒，workspace 建置通過。
* 涉及模組或檔案：Create/Modify `crates/wukong-runtime/src/`（`util.rs`、`bootstrap.rs`、`lib.rs` 匯出）、`crates/wukong-runtime/Cargo.toml`（feature 透傳）
* 預估：1～2 小時

#### Task 6.2 CLI 與 schedulerd 改用共用 util

* 任務說明：改寫 `cli/main.rs:32-48,335,388-397`、`cli/repl.rs:82-91`、`schedulerd/main.rs:210-228,265` 的重複實作為共用模組呼叫，逐檔改完即跑該 crate 測試。
* 預期輸出：兩 crate 無本地重複實作；測試不回歸。
* 涉及模組或檔案：Modify `crates/wukong-cli/src/main.rs`、`crates/wukong-cli/src/repl.rs`、`crates/wukong-schedulerd/src/main.rs`
* 依賴：Task 6.1
* 預估：1～2 小時

#### Task 6.3 Web 與 Telegram 改用共用 util

* 任務說明：改寫 `web/lib.rs:440,447-455,543-551`、`web/main.rs:30-46`、`telegram/dispatch.rs:524,534-542,855-863,927-935`、`telegram/main.rs:61-77` 為共用模組呼叫。
* 預期輸出：兩 crate 無本地重複實作；測試不回歸。
* 涉及模組或檔案：Modify `crates/wukong-web/src/lib.rs`、`crates/wukong-web/src/main.rs`、`crates/wukong-telegram/src/dispatch.rs`、`crates/wukong-telegram/src/main.rs`
* 依賴：Task 6.1
* 預估：1～2 小時

#### Task 6.4 排程執行編排收斂

* 任務說明：`wukong-scheduler` 新增 `run_claimed_job()` 封裝「start_run → execute → finish_run → complete + lease 檢查」序列；`cli/main.rs:265-309` 與 `schedulerd/main.rs:121-169` 改用之。既有 lease 併發測試守護行為不變。
* 預期輸出：編排邏輯單一實作；lease 測試全綠。
* 涉及模組或檔案：Modify `crates/wukong-scheduler/src/`（新增函式）、`crates/wukong-cli/src/main.rs`、`crates/wukong-schedulerd/src/main.rs`
* 依賴：Task 6.2（同檔案，序列進行）
* 預估：2 小時

#### Task 6.5 CLI REPL 與串流渲染統一

* 任務說明：真實 stdin REPL（`cli/main.rs:81-117`）改走測試良好的 `run_repl_loop`，統一「每回合重載 settings」行為；`run_one`（`cli/main.rs:359-371`）改用 `render.rs::StreamRenderer` 取代內嵌分流閉包。
* 預期輸出：REPL 邏輯單一實作、settings 重載行為一致。
* 涉及模組或檔案：Modify `crates/wukong-cli/src/main.rs`、`crates/wukong-cli/src/repl.rs`
* 依賴：Task 6.2
* 預估：1～2 小時

#### Task 6.6 移除 `gateway/pipeline.rs` 死碼

* 任務說明：先確認 `2026-07-05-memory-optimization-parity` 計畫中對 `pipeline.rs` 的 dedupe key 任務狀態並協調（刪除後同步更新該計畫）；`grep -rn "pipeline::" crates/` 確認無呼叫者後刪除模組、`lib.rs` 匯出與相關測試。
* 預期輸出：死碼移除、無編譯警告、無測試回歸、跨計畫文件一致。
* 涉及模組或檔案：Delete `crates/wukong-gateway/src/pipeline.rs`；Modify `crates/wukong-gateway/src/lib.rs`、`docs/superpowers/plans/2026-07-05-memory-optimization-parity.md`
* 預估：1 小時

---

## Phase 7 — 治理與結構整理

### Goal

補齊版本／文件／健康檢查等工程治理缺口，拆分過大檔案，收斂串流行為差異的結論。

### Deliverables

* 版本與 tag 同步機制、CHANGELOG、文件與實際狀態一致
* Web／memoryd 具 healthcheck；三個過大檔案完成拆分
* server backend 串流行為有結論（修復或文件化）

### Dependencies

* 無硬依賴；建議於 Phase 2～6 之後進行（7.4／7.5 觸碰的檔案與 Phase 3 重疊，須在其後）

### Tasks

#### Task 7.1 版本同步與 CHANGELOG

* 任務說明：制定 release 時同步 `workspace.package.version` 與 tag 的機制（release workflow sed 或 `cargo-release`）；新增 `CHANGELOG.md`（可用 git-cliff 由 conventional commits 生成）並掛入 release workflow；更新相關 SOP 文件。
* 預期輸出：下一次 release 產物版本一致且有變更紀錄。
* 涉及模組或檔案：Modify `Cargo.toml`、`.github/workflows/release.yml`；Create `CHANGELOG.md`
* 預估：1～2 小時

#### Task 7.2 文件與依賴 pin 同步

* 任務說明：更新 README 測試徽章（242→實際數）；CLAUDE.md 補 `wukong-chat-history` 並改為 15 個 crate；`Dockerfile:3` `ARG VERSION` 改由 release workflow 注入或文件化更新步驟；pin `opencode-ai` 至明確版本、agent-reach 改用 tag/commit。
* 預期輸出：文件與實際狀態相符；Docker build 可重現。
* 涉及模組或檔案：Modify `README.md`、`CLAUDE.md`、`Dockerfile`、`.github/workflows/release.yml`
* 預估：1 小時

#### Task 7.3 healthcheck

* 任務說明：`wukong-web` 與 memoryd 各加免認證 `/healthz`（僅回 200 不洩漏資訊）；compose 為兩者加 `healthcheck`；telegram／schedulerd 評估 heartbeat 檔方案或文件註明不適用；`scripts/test-docker-runtime.sh` 補驗證。
* 預期輸出：`docker compose ps` 顯示 healthy。
* 涉及模組或檔案：Modify `crates/wukong-web/src/lib.rs`、`crates/wukong-memoryd/src/lib.rs`、`docker-compose.yml`、`scripts/test-docker-runtime.sh`
* 依賴：Task 3.1（healthz 需繞過認證 middleware）
* 預估：1～2 小時

#### Task 7.4 拆分 `opencode_server.rs`

* 任務說明：純搬移不改行為：拆出 `sse.rs`（串流解析）與 `event_map.rs`（事件映射），測試隨模組搬移。
* 預期輸出：單檔 production 程式碼 < 1000 行；測試全綠。
* 涉及模組或檔案：Modify `crates/wukong-gateway/src/`（新增 `sse.rs`、`event_map.rs`，調整 `opencode_server.rs`、`lib.rs`）
* 預估：2 小時

#### Task 7.5 拆分 `web/lib.rs`

* 任務說明：依既有 `*_api.rs` 慣例續拆（chat handlers、SSE、static assets 各自成模組），純搬移不改行為。
* 預期輸出：`lib.rs` 大幅縮減；測試全綠。
* 涉及模組或檔案：Modify `crates/wukong-web/src/`（新增模組檔）
* 依賴：Phase 3 完成（同檔案）
* 預估：2～3 小時

#### Task 7.6 拆分 `wukong-chat.js`

* 任務說明：抽出 SSE 事件處理群組為獨立模組，前端 `.test.mjs` 隨遷；`include_str!` 清單同步更新。
* 預期輸出：`wukong-chat.js` 縮減；`node --test` 全綠。
* 涉及模組或檔案：Modify `crates/wukong-web/static/components/`、`crates/wukong-web/src/lib.rs`（資產內嵌清單）
* 預估：1～2 小時

#### Task 7.7 server backend text 串流行為決議

* 任務說明：確認 `map_server_event` 對 `text` part 一律 Ignore（`opencode_server.rs:706-738`）是否刻意設計：若否，發 `Text` 事件並以 `list_messages` 結果去重；若是，於 `docs/entrypoints.md` 註明兩 backend 串流行為差異。
* 預期輸出：行為修復或文件化結論，二擇一落地。
* 涉及模組或檔案：Modify `crates/wukong-gateway/src/opencode_server.rs` 或 `docs/entrypoints.md`
* 依賴：Task 7.4（若拆分先行，改動落在 `event_map.rs`）
* 預估：1～2 小時

---

## 全域驗收與備註

1. 每個 Task 完成即執行 `cargo test --workspace && cargo clippy --all-targets -- -D warnings`，綠燈方可進下一個 Task。
2. Phase 1 完成前不開始其他 Phase；Phase 2／3／4／5 之間無檔案重疊，可依人力平行。
3. 原計畫的「低優先觀察項」（dedupe TOCTOU、`redact_token`、O(n·m) 合併、query token 移除、shell 測試入 CI）不在本次拆解範圍，留待後續評估。
4. 完成各 Phase 後，回頭更新原計畫與本文件的對應狀態。
