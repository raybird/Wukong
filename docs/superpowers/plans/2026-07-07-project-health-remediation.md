# Wukong 專案健康度改進計畫（Project Health Remediation）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 依 2026-07-07 全專案審查結果，分三階段修復高風險缺陷（render panic / XSS / 部署安全 / CI 缺口）、行為與效能問題（CLI backend agent 欄位、同步 embedding、Telegram 韌性），以及工程治理與程式碼衛生（版本、文件、重複膠水、死碼）。

**背景:** 審查涵蓋全部 15 個 crate（約 22,000 行 Rust）、前端、部署腳本與 CI。整體評價：架構分層與測試文化屬中上偏高，但（1）約 420 個測試無 CI 把關、（2）預設部署姿態無認證、（3）render 有可實際觸發的 panic 與 XSS 缺口。

**Tech Stack:** Rust workspace（tokio / axum / sqlx / reqwest / pulldown-cmark）、GitHub Actions、Docker Compose。

**執行順序:** Phase 0 各任務彼此獨立可平行；Phase 1 依賴 Phase 0 的 CI 安全網；Phase 2 可穿插進行。每個任務完成後執行 `cargo test --workspace && cargo clippy --all-targets -- -D warnings`。

---

## 問題總覽

| # | 問題 | 嚴重度 | 位置 | 任務 |
|---|------|--------|------|------|
| 1 | 無 PR/push CI，測試不守門 | 高 | `.github/workflows/`（僅 release.yml） | Task 1 |
| 2 | `Cargo.lock` 未納版控 | 高 | `.gitignore` | Task 2 |
| 3 | 分塊在 CJK 多位元組字上 panic | 高 | `crates/wukong-render/src/lib.rs:164` | Task 3 |
| 4 | 分塊切開 `<pre>` 等標籤 → Telegram 400 靜默失敗 | 中 | `crates/wukong-render/src/lib.rs:151-177` | Task 3 |
| 5 | `to_web_html` 未過濾 URL scheme → `javascript:` XSS | 高 | `crates/wukong-render/src/lib.rs:31-47` | Task 3 |
| 6 | Web Console 預設空 token + 0.0.0.0 對外開放 | 高 | `docker-compose.yml:109-111` | Task 4 |
| 7 | memoryd HTTP API 完全無認證 | 中 | `crates/wukong-memoryd/src/lib.rs:91-99` | Task 4 |
| 8 | Web 認證逐 handler 手動檢查，非 middleware | 中 | `crates/wukong-web/src/lib.rs:406-411` | Task 4 |
| 9 | CLI backend 靜默忽略 `AgentRequest.agent` | 高 | `crates/wukong-gateway/src/backend.rs:66-92` | Task 5 |
| 10 | 同步 embedding 阻塞 tokio executor | 高 | `crates/wukong-memory/src/lib.rs:169,270` | Task 6 |
| 11 | 向量召回被 recency 截斷、全表 cosine | 高 | `crates/wukong-memory/src/store/mod.rs:262` | Task 6 |
| 12 | N+1 逐列 UPDATE/DELETE 且無 transaction | 中 | `crates/wukong-memory/src/store/mod.rs:234,324,397` | Task 6 |
| 13 | Telegram `ok:false` 無退避忙迴圈 | 中 | `crates/wukong-tg-client/src/client.rs:151-160` | Task 7 |
| 14 | 送出錯誤被 `let _ =` 吞掉無 log | 中 | `crates/wukong-telegram/src/dispatch.rs:1217` 等 | Task 7 |
| 15 | 膠水程式碼跨進入點重複（8 處 `now_unix` 等） | 中 | 見 Task 8 清單 | Task 8 |
| 16 | 版本 0.1.0 與 tag v0.16.x 脫節、無 CHANGELOG | 中 | `Cargo.toml:7` | Task 9 |
| 17 | 文件漂移（README 徽章、CLAUDE.md crate 數等） | 低 | 見 Task 9 清單 | Task 9 |
| 18 | Docker 常駐服務無 HEALTHCHECK | 中 | `Dockerfile`、`docker-compose.yml` | Task 10 |
| 19 | `gateway/pipeline.rs` 死碼（161 行無呼叫者） | 低 | `crates/wukong-gateway/src/pipeline.rs` | Task 11 |
| 20 | confidence 計分退化（單候選恆 1.0 / recency-only 恆 0） | 中 | `crates/wukong-memory/src/recall/mod.rs:156-160` | Task 12 |
| 21 | 過大檔案（web/lib.rs 3647 行等） | 低 | 見 Task 13 清單 | Task 13 |

---

# Phase 0（P0）：安全網與高危修復

## Task 1: 建立 CI Workflow

**問題:** 唯一的 workflow 是 tag 觸發的 `release.yml`。全 workspace 約 420 個測試、clippy、fmt 在合併前不會自動執行，品質守門完全依賴開發者本機。

**Files:**
- Create: `.github/workflows/ci.yml`

**Steps:**

- [ ] **Step 1:** 新增 `ci.yml`，觸發條件 `push`（main）與 `pull_request`，內容三段：
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --workspace`
- [ ] **Step 2:** 加上 Rust 快取（`Swatinem/rust-cache@v2`）縮短建置時間。預設不開 `embed` feature，避免拉入 ONNX 重依賴。
- [ ] **Step 3:** 推分支驗證三段皆綠；故意弄壞一個測試確認會紅。

**驗收:** PR 上可見 CI 狀態；main 分支 push 亦觸發。

## Task 2: `Cargo.lock` 納入版控

**問題:** `.gitignore` 含 `Cargo.lock`。本 workspace 產出 4 個 release binary，不鎖 lock 導致 release build 不可重現、供應鏈無法稽核。

**Files:**
- Modify: `.gitignore`（移除 `Cargo.lock` 一行）

**Steps:**

- [ ] **Step 1:** 從 `.gitignore` 移除 `Cargo.lock`，`git add Cargo.lock` 納入版控。
- [ ] **Step 2:** 確認 `release.yml` 建置未帶 `--locked`；補上 `--locked` 確保 release 使用鎖定版本。

**驗收:** `git ls-files | grep Cargo.lock` 有輸出；release workflow 帶 `--locked` 可過。

## Task 3: render 三項修復（panic / 標籤平衡 / URL scheme）

**問題（三項，皆在 `crates/wukong-render/src/lib.rs`）:**
1. `split_chunks` 於 `lib.rs:164` 用 `rest.split_at(max)` 以位元組切割（`max=4096`，`lib.rs:24`）。切點落在 CJK 多位元組字中間會 panic（"byte index 4096 is not a char boundary"）。LLM 輸出一段超過 4096 bytes、無內部換行的中文段落即觸發。影響 `to_telegram_html` 的兩條路徑：Telegram 回覆（`dispatch.rs:1211`）與排程通知（`notify.rs:84`）。
2. 分塊會把長度 >4096 的 `<pre>`/`<blockquote>` 從開閉標籤之間切開，前塊標籤未閉合 → Telegram 回 400 → 錯誤被 `let _ =` 吞掉 → 使用者靜默收不到訊息。
3. `to_web_html`（`lib.rs:31-47`）擋掉了原始 HTML 但未過濾連結協定，`[x](javascript:...)` 產出可點擊的 `<a href="javascript:...">`；前端在 `chat-message.js:32` 與 `wukong-chat.js:1003` 以 `innerHTML` 直接信任這段 HTML。內容來自 LLM／記憶／工具結果，有 prompt-injection 放大風險。

**Files:**
- Modify: `crates/wukong-render/src/lib.rs`
- Test: 同檔 `mod tests`

**Steps:**

- [ ] **Step 1（失敗測試先行）:** 補三組測試：
  - 超長（>4096 bytes）無換行中文段落經 `to_telegram_html` + `split_chunks` 不 panic，各塊皆為合法 UTF-8 且 ≤ 上限。
  - 超長 `<pre>` 區塊分塊後，每一塊的 Telegram 標籤（`b/i/s/u/code/pre/blockquote/a`）皆自我平衡。
  - `[x](javascript:alert(1))`、`[x](data:text/html,...)` 經 `to_web_html` 不產生對應 `href`；`https:`、`mailto:`、相對路徑保留。
- [ ] **Step 2:** 修 `split_chunks`：切點以 `is_char_boundary` 向前回退到安全邊界（勿用 nightly `floor_char_boundary`）。
- [ ] **Step 3:** 分塊時追蹤未閉合標籤堆疊：塊尾補閉合、次塊開頭重開（Telegram 標籤子集小，手寫堆疊即可，不需完整 HTML parser）。
- [ ] **Step 4:** `to_web_html` 於 pulldown-cmark 事件層攔截 `Tag::Link`/`Tag::Image`，scheme allowlist 取 `http/https/mailto` + 相對路徑；不合格者降級為純文字。`to_telegram_html` 同步套用（`lib.rs:98` 目前僅 escape）。
- [ ] **Step 5:** `cargo test -p wukong-render` 全綠。

**驗收:** 三組新測試通過；既有 14 個 render 測試不回歸。

## Task 4: 部署與 API 認證加固

**問題:**
1. `docker-compose.yml:109-111` 預設 `WUKONG_WEB_HOST=0.0.0.0`、`WUKONG_WEB_TOKEN=`（空）、發佈 8787 埠。空 token 時 `authorized()`（`web/lib.rs:406-411`）直接放行 → Web Console 預設無認證對外開放，而它可觸發 agent turn（下游是 `--dangerously-skip-permissions` 的 opencode）、讀記憶、改設定。
2. memoryd 所有路由（`memoryd/lib.rs:91-99`）無任何認證且綁 `0.0.0.0`（`main.rs:13`）。
3. Web 認證為「每個 handler 手動呼叫 `authorized()`」，新增路由忘了加就裸奔；token 走 query string 會進 access log／瀏覽歷史。
4. 次要：`web/lib.rs:83-87` 將 token 注入 index.html 行內 script 僅逃逸 `\` 與 `"`，未處理 `</script>`；Telegram callback query 未檢白名單（`telegram/main.rs:191-193`、`dispatch.rs:402`）。

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`、`crates/wukong-web/src/main.rs`
- Modify: `crates/wukong-memoryd/src/lib.rs`、`crates/wukong-memoryd/src/main.rs`
- Modify: `crates/wukong-telegram/src/main.rs`、`crates/wukong-telegram/src/dispatch.rs`
- Modify: `docker-compose.yml`、`docs/docker.md`
- Test: `crates/wukong-memoryd/tests/http.rs`、web 單元測試

**Steps:**

- [ ] **Step 1:** Web 認證抽成 axum middleware（或 extractor）統一掛在 router 上，移除各 handler 內的手動檢查；同時支援 `Authorization: Bearer` header（保留 query token 相容一版，文件標註棄用）。
- [ ] **Step 2:** 空 token 行為改為 fail-closed：`WUKONG_WEB_TOKEN` 為空且 host 非 loopback 時拒絕啟動，除非明確設 `WUKONG_WEB_ALLOW_INSECURE=1`。`docker-compose.yml` 註解與 `docs/docker.md` 加強警示。
- [ ] **Step 3:** memoryd 加 `WUKONG_MEMORY_TOKEN` bearer 驗證 middleware；`tests/http.rs` 補 401（無 token）與 200（正確 token）測試；預設綁定改 `127.0.0.1`（env 可覆寫）。
- [ ] **Step 4:** index.html token 注入改為 JSON 序列化後再嵌入（覆蓋 `</script>` 案例）；callback query 入口補白名單檢查與測試。
- [ ] **Step 5:** 全 workspace 測試 + `scripts/test-web-chat-scope.sh` 手動驗證。

**驗收:** 無 token + 非 loopback 啟動即失敗；memoryd 未帶 token 回 401；所有既有 web 測試調整後通過。

---

# Phase 1（P1）：行為正確性與效能

## Task 5: CLI backend 傳遞 `agent` 欄位

**問題:** `AgentRequest.agent`（`backend.rs:32`）只有 server backend 消費（`opencode_server.rs:103,122`）。CLI backend 的 `assemble_argv`（`backend.rs:66-92`）從未讀取 `req.agent`。orchestrator 規劃時設 `agent: Some("plan")`（`router.rs:239,256,272`）想走低成本 plan agent，在預設 `opencode run` 配置下被靜默丟棄——兩個 backend 行為分歧。

**Files:**
- Modify: `crates/wukong-gateway/src/backend.rs`
- Test: 同檔 `mod tests`

**Steps:**

- [ ] **Step 1:** 查 `opencode run --help` 確認 agent 參數旗標名稱（預期 `--agent <name>`）。
- [ ] **Step 2（失敗測試）:** `assemble_argv` 帶 `agent: Some("plan")` 時 argv 含該旗標；`None` 時不含。
- [ ] **Step 3:** 實作並確認 `run` / `run_streaming` 兩條路徑皆生效。
- [ ] **Step 4:** 以假 agent 驗證整體流程不回歸：`cargo run -p wukong-orchestrator --bin wukong-orchestrate -- --agent-cmd "printf fixer" "fix the bug"`。

**驗收:** 新測試通過；規劃棒實際帶 agent 參數執行。

## Task 6: memory 效能與寫入一致性

**問題:**
1. `emb.embed()` 在 `remember`（`lib.rs:169`）與 `recall`（`lib.rs:270`）中同步呼叫 ONNX 推論，阻塞 tokio worker；全 workspace 無任何 `spawn_blocking`。backfill 的 `embed_batch`（`lib.rs:486-502`）同理。
2. 向量召回先撈「最近 `fetch_limit` 筆有 embedding 的列」（`store/mod.rs:262`）再算 cosine——較舊但語意更相關的記憶被 recency 截斷漏掉；且每次召回全量解碼 blob 算 cosine，記憶成長後是 O(n) 瓶頸。
3. `touch_recalled`（`store/mod.rs:234-247`）、`mark_consolidated`（`:324-333`）、`delete_memories`（`:397-407`）迴圈逐列發 SQL，未批次未包 transaction，中途失敗留下半套狀態。
4. `Store::open`（`store/mod.rs:98-106`）用預設連線池（max 10），背景 backfill 與前景寫入並發時可能 `SQLITE_BUSY`。

**Files:**
- Modify: `crates/wukong-memory/src/lib.rs`、`crates/wukong-memory/src/store/mod.rs`、`crates/wukong-memory/src/embed/mod.rs`
- Test: `crates/wukong-memory/tests/integration.rs`

**Steps:**

- [ ] **Step 1:** embedding 呼叫改包 `tokio::task::spawn_blocking`（確認 `Embedder` trait bounds 需 `Send + Sync`，必要時以 `Arc` 持有）。
- [ ] **Step 2（失敗測試）:** 建立「舊但語意最相關」與「新但不相關」記憶各若干，驗證向量召回不因 recency 截斷漏掉前者。
- [ ] **Step 3:** 候選選取改為不依 recency 截斷（全量掃描 + 上限保護即可；ANN／sqlite-vec 另開設計文件，本任務不做）。
- [ ] **Step 4:** N+1 改為 `IN (...)` 單句或以 transaction 包裹；`mark_consolidated` 與 summary 寫入同一 transaction。
- [ ] **Step 5:** 連線池設定寫入序列化（`max_connections(1)` 或明確 `busy_timeout`），補「backfill 與前景 remember 並發」測試。

**驗收:** `cargo test -p wukong-memory` 全綠含新測試；召回不再有 recency 截斷行為。

## Task 7: Telegram 傳輸韌性

**問題:**
1. `get_updates`（`tg-client/client.rs:151-160`）不檢查 Telegram 回應 `ok` 欄位。token 失效（401 `{"ok":false}`）時解析得空結果，主迴圈（`telegram/main.rs:130-154`）不 sleep 直接重打 → 無退避忙迴圈。
2. 送出錯誤被靜默吞掉：`let _ = client.send_message(...)`（`dispatch.rs:902,919,1214,1217`、`schedulerd/notify.rs`）。Telegram 回 400 時使用者收不到答案且無任何 log。
3. 低優先：token 輪替時 `offset = 0`（`main.rs:128`）可能重拉舊 update 重複處理。

**Files:**
- Modify: `crates/wukong-tg-client/src/client.rs`
- Modify: `crates/wukong-telegram/src/main.rs`、`crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-schedulerd/src/notify.rs`
- Test: 各檔 `mod tests`（`mock` feature）

**Steps:**

- [ ] **Step 1（失敗測試）:** mock 回傳 `{"ok":false,"error_code":401}` 時 `get_updates` 回 `Err` 而非空成功。
- [ ] **Step 2:** `get_updates`／`send_message` 檢查 `ok` 欄位，`false` 轉為帶 `error_code`/`description` 的 `Err`。
- [ ] **Step 3:** 主迴圈對此類錯誤套用既有 3s 退避路徑；401 額外輸出明確 log（token 失效提示）。
- [ ] **Step 4:** 全部 `let _ = client.send_*` 改為記 log（`eprintln!` 或既有 log 慣例），不改變控制流。
- [ ] **Step 5:** token 輪替保留 offset（不歸零），加註解說明。

**驗收:** 失效 token 下迴圈有退避、有明確 log；送出失敗可在 log 中觀察到。

## Task 8: 膠水程式碼下沉共用

**問題:** 跨進入點複製貼上（一改就要改多處）：
- `now_unix()` 重複 8 份：`cli/main.rs:335`、`web/lib.rs:440`、`telegram/dispatch.rs:524`、`schedulerd/main.rs:265`、`schedulerd/notify.rs:25`、`scheduler/store.rs:380`、wukong-memory、wukong-chat-history。
- `upload_root()` 逐字重複：`web/lib.rs:447-455` 與 `telegram/dispatch.rs:534-542`。
- 「載入 settings → 套用 default_model 與 planner preferences」重複 5 份：`cli/main.rs:388-397`、`cli/repl.rs:82-91`、`telegram/dispatch.rs:855-863`、`dispatch.rs:927-935`、`web/lib.rs:543-551`。
- memory bootstrap（embed feature + `WUKONG_MD_DIR` + 預設 db 路徑）重複 4 份：`cli/main.rs:32-48`、`web/main.rs:30-46`、`telegram/main.rs:61-77`、`schedulerd/main.rs:210-228`。
- 排程執行編排（start_run → execute → finish_run → complete，含 lease 檢查）重複：`cli/main.rs:265-309` 與 `schedulerd/main.rs:121-169`。
- CLI 真實 REPL（`cli/main.rs:81-117`）未重用測試良好的 `run_repl_loop`（`cli/repl.rs`），且兩者 settings 重載行為不一致；`run_one`（`cli/main.rs:359-371`）內嵌一份與 `render.rs::StreamRenderer` 等價的分流邏輯。

**Files:**
- Modify: `crates/wukong-runtime/src/`（新增 `util.rs` / `bootstrap.rs`，或評估新 crate `wukong-common`）
- Modify: `crates/wukong-scheduler/src/`（新增 `run_claimed_job()`）
- Modify: 上列各重複點呼叫端

**Steps:**

- [ ] **Step 1:** 決策共用位置：優先放 `wukong-runtime`（CLI/Web/Telegram/Scheduler 皆已依賴）；`wukong-scheduler`／`wukong-memory` 等底層 crate 的 `now_unix` 因依賴方向不可倒灌，保留本地副本並加註。
- [ ] **Step 2:** 抽 `now_unix`、`upload_root`、`apply_settings_to_config`、`open_memory_from_env`（memory bootstrap）至共用模組，逐一改寫呼叫端（每改一處跑該 crate 測試）。
- [ ] **Step 3:** `wukong-scheduler` 新增 `run_claimed_job()` 封裝執行編排，`cli` 與 `schedulerd` 改用之。
- [ ] **Step 4:** CLI 真實 REPL 改走 `run_repl_loop`，統一「每回合重載 settings」行為；`run_one` 改用 `StreamRenderer`。
- [ ] **Step 5:** `cargo test --workspace` 全綠。

**驗收:** 上列重複點各只剩一份實作（除依賴方向限制者）；行為測試不回歸。

---

# Phase 2（P2）：工程治理與清理

## Task 9: 版本與文件治理

**問題:** workspace 版本停在 0.1.0（`Cargo.toml:7`）而 tag 已到 v0.16.34；無 CHANGELOG；README 徽章寫 242 個測試（實際約 420）；CLAUDE.md 寫 14 個 crate（實際 15，`wukong-chat-history` 未入列）；`Dockerfile:3` `ARG VERSION` 預設落後；`opencode-ai@latest` 與 agent-reach `main.zip` 未 pin 版本。

**Steps:**

- [ ] **Step 1:** 版本策略：release 時同步 `workspace.package.version` 與 tag（手動或 `cargo-release`／release workflow sed），並更新 `docs/superpowers/` 相關 SOP。
- [ ] **Step 2:** 新增 `CHANGELOG.md`（可用 git-cliff 由 conventional commits 生成），release workflow 附掛。
- [ ] **Step 3:** 更新 README 測試徽章；CLAUDE.md 補 `wukong-chat-history`（共享聊天歷史與附件路徑解析）並改為 15 個 crate。
- [ ] **Step 4:** `Dockerfile` 的 `ARG VERSION` 改由 release workflow 注入或文件化更新步驟；pin `opencode-ai` 至明確版本、agent-reach 改用 tag/commit 而非 `main.zip`。

**驗收:** 下一次 release 產物版本一致；文件與實際狀態相符。

## Task 10: Docker HEALTHCHECK

**問題:** `docker-compose.yml` 五個 `restart: unless-stopped` 常駐服務皆無健康檢查，hang 死無法自動偵測。

**Steps:**

- [ ] **Step 1:** `wukong-web` 新增免認證 `/healthz` 路由（僅回 200，不洩漏資訊）；memoryd 同（若已有 `/v1/stats` 需認證則另設 `/healthz`）。
- [ ] **Step 2:** compose 為 web／memoryd 加 `healthcheck`（curl/wget `--spider`）；telegram／schedulerd 為非 HTTP 服務，評估以「touch heartbeat 檔 + test -f && find -mmin」方式檢查，或文件註明不適用。
- [ ] **Step 3:** `scripts/test-docker-runtime.sh` 補 healthcheck 驗證。

**驗收:** `docker compose ps` 顯示 healthy 狀態。

## Task 11: 移除 `gateway/pipeline.rs` 死碼

**問題:** `crates/wukong-gateway/src/pipeline.rs` 的 `run_turn`（161 行）為 v1 遺留簡化版回合流程（無 planner／persona／streaming），workspace 內無任何外部呼叫者，與 `wukong-runtime::run_turn` 概念重複。

**Steps:**

- [ ] **Step 1:** 先確認 `docs/superpowers/plans/2026-07-05-memory-optimization-parity.md` 中「Modify `crates/wukong-gateway/src/pipeline.rs`: pass stable dedupe keys」任務的狀態——若該計畫尚未執行完畢，先協調（刪除後同步更新該計畫），避免兩份計畫互相衝突。
- [ ] **Step 2:** `grep -rn "pipeline::run_turn\|pipeline::" crates/` 再次確認無呼叫者後刪除模組與 `lib.rs` 匯出；相關測試一併移除。
- [ ] **Step 3:** `cargo build --workspace && cargo test --workspace` 全綠。

**驗收:** 死碼移除、無編譯警告、無測試回歸。

## Task 12: 召回 confidence 計分修正

**問題:** `rank`（`recall/mod.rs:156-160`）在 `bmax - bmin < 1e-9` 時 `lexical_norm` 一律設 1.0 → 單一關鍵字命中不論品質 confidence 恆為滿分；Tree 模式或無 embedder 的 Hybrid 下候選全為 recency → relevance=0 → 有結果但 confidence 恆 0（`lib.rs:298-302`）。兩者皆使信心分數失真。

**Steps:**

- [ ] **Step 1（失敗測試）:** 單候選弱 bm25 命中 → confidence < 1.0；recency-only 有命中 → confidence 不為 0 或回應中明確標記 `source: recency`。
- [ ] **Step 2:** 單候選改用 bm25 絕對值映射（如 sigmoid／固定保守值）取代退化的 min-max；recency-only 的 confidence 語意擇一：給予保守非零值，或在 `RecallExplanation` 標記來源讓上層自行解讀。與 `2026-07-05-memory-optimization-parity` 計畫的 confidence 任務對齊，勿重工。
- [ ] **Step 3:** `cargo test -p wukong-memory` 全綠。

**驗收:** 兩種退化情形有合理的 confidence 值與測試守護。

## Task 13: 大檔拆分與遺留清理

**問題:** `web/lib.rs` 3647 行、`gateway/opencode_server.rs` 1395 行（HTTP client + SSE 解析 + 事件映射 + question 解析多職責）、前端 `wukong-chat.js` 1037 行。另有行為疑點：server backend 對 `text` part 一律 Ignore（`opencode_server.rs:706-738`），答案不逐字串流、回合結束才整段出現，與 CLI backend 行為不一致。

**Steps:**

- [ ] **Step 1:** `opencode_server.rs` 拆為 `sse.rs`（串流解析）與 `event_map.rs`（事件映射），測試隨模組搬移。
- [ ] **Step 2:** `web/lib.rs` 依既有 `*_api.rs` 慣例續拆（chat handlers、SSE、static assets 各自成模組）。
- [ ] **Step 3:** `wukong-chat.js` 抽出 SSE 事件處理群組為獨立模組，前端 `.test.mjs` 隨遷。
- [ ] **Step 4:** 確認 server backend text 不串流是否刻意設計：若否，`map_server_event` 對 `text` part 發 `Text` 事件並以 `list_messages` 結果去重；若是，於 `docs/entrypoints.md` 註明兩 backend 的串流行為差異。
- [ ] **Step 5:** 純搬移不改行為，`cargo test --workspace` + 前端 `node --test` 全綠。

**驗收:** 單檔 production 程式碼皆 < 1000 行；串流行為差異有結論（修復或文件化）。

---

## 低優先觀察項（backlog，不排任務）

- **dedupe TOCTOU**：`store/mod.rs:133-157` 先 SELECT 再 INSERT，並發同 key 時其一得唯一約束錯誤而非既有 id。可改為 `INSERT ... ON CONFLICT ... RETURNING`。實務單回合序列化，風險低。
- **空輸出修復重送側效應**：`turn.rs:272-286` 修復分支重跑最終步 prompt 至同一 session，理論上可能重複已執行的工具動作。屬邊角情形，觀察即可。
- **`redact_token` 位元組切片**：`settings/lib.rs:112` 假設 ASCII，非 ASCII token 會 panic（Telegram token 恆為 ASCII，實務安全）。
- **O(n·m) 線性合併**：`recall/mod.rs:83-93,245-255`、`consolidate.rs:38` 的 `iter_mut().find()`，候選數大時可改 HashMap。
- **Web token 走 query string**：Task 4 加入 header 支援後，追蹤前端全面改用 header 再移除 query 相容。
- **CI 納入 shell 級測試**：`scripts/test-*.sh` 六支目前僅手動執行，可評估納入 CI 的 nightly job。

---

## 全域驗收標準

1. `cargo test --workspace` 全綠（含各任務新增測試）。
2. `cargo clippy --all-targets -- -D warnings` 無警告。
3. Phase 0 完成後：CI 於 PR 守門、`Cargo.lock` 入版控、render 三缺陷有測試守護、預設部署 fail-closed。
4. 每完成一個 Phase，回頭更新本文件 checkbox 與 README／CLAUDE.md 相關敘述。
