# Task Implementation — 專案健康度改進計畫執行記錄

> 對應 Decomposition：`docs/superpowers/plans/2026-07-07-project-health-remediation-decomposition.md`
> 分支：`chore/project-health-remediation`

---

## Phase 1 — CI 安全網與可重現建置 ✅（2026-07-07）

### Summary

建立 push/PR 觸發的 CI（fmt / clippy / test 三段），並將 `Cargo.lock` 納入版控。引入 `-D warnings` 的 clippy gate 後，一次性清除既有程式碼累積的格式漂移與 lint（含一個真實的「MutexGuard 跨 await」正確性缺陷），使三段在本地全綠。

### Implemented Changes

**Task 1.1 — CI workflow**
- 新增 `.github/workflows/ci.yml`：`push`（main）與 `pull_request` 觸發；`fmt --check` → `clippy --all-targets --locked -D warnings` → `test --workspace --locked` 三段；`Swatinem/rust-cache@v2` 快取；`concurrency` 取消過期 run；預設不開 `embed` feature（避免拉入 ONNX）。
- 為使 gate 實際變綠，處理既有程式碼的既存問題：
  - **fmt**：`cargo fmt --all` 正規化 8 個檔案的格式漂移（wukong-chat-history / gateway / memory / orchestrator / runtime / scheduler / web）。
  - **clippy 風格 lint**：
    - `wukong-tg-client/src/client.rs`：測試 mock 的 4-tuple 欄位抽 `InlineEditLog` 型別別名（沿用同檔既有 `MockFiles` 慣例）。
    - `wukong-memory/src/store/mod.rs`：`insert_memory`（低階 DB helper，8 參數）加針對性 `#[allow(clippy::too_many_arguments)]` + 理由註解。
    - `wukong-telegram/src/dispatch.rs`：`handle_message_with_pending`(8)、`handle_message_with_responder`(9) 加針對性 allow；測試 mock 抽 `QuestionReplyLog` 別名。
    - `wukong-telegram/src/main.rs`：`dispatch_update_event`(8) 加針對性 allow。
    - `wukong-web/src/lib.rs`：測試 mock 抽 `QuestionReplyLog` 別名。
  - **clippy 正確性 lint（真實缺陷）**：`wukong-telegram/src/dispatch.rs` 的 `handle_callback_query` 在持有 `std::sync::Mutex` guard 期間 `.await`（`QuestionAction::Custom` 分支呼叫 `edit_message_text`）——有阻塞 executor 的死鎖風險。重構為「鎖內只計算 flag、不 await，釋放後才做所有網路呼叫」，與該函式既有的 defer 模式一致；兩個早退 await（問題失效）併為 `invalid` flag 於鎖外處理。行為不變，測試全綠。

**Task 1.2 — Cargo.lock 納入版控**
- `.gitignore` 移除 `Cargo.lock`；`git add Cargo.lock` 納入追蹤。
- `release.yml` 兩處 `cargo build --release` 補 `--locked`；CI 的 clippy/test 亦帶 `--locked`，確保 lockfile 不漂移。

### Verification

- `cargo fmt --all -- --check` ✓
- `cargo clippy --all-targets --locked -- -D warnings` ✓（EXIT 0）
- `cargo test --workspace --locked` ✓（全部 crate 通過，0 失敗）

### Modified Files

- Create `.github/workflows/ci.yml`
- Modify `.gitignore`、`.github/workflows/release.yml`
- Add `Cargo.lock`（納入版控）
- Modify `crates/wukong-telegram/src/dispatch.rs`（callback 鎖跨 await 修復 + allow + 型別別名）
- Modify `crates/wukong-telegram/src/main.rs`、`crates/wukong-tg-client/src/client.rs`、`crates/wukong-memory/src/store/mod.rs`、`crates/wukong-web/src/lib.rs`（lint 修復）
- 格式正規化：`crates/wukong-chat-history/src/lib.rs`、`crates/wukong-gateway/src/{backend.rs,opencode_server.rs}`、`crates/wukong-orchestrator/src/lib.rs`、`crates/wukong-runtime/src/turn.rs`、`crates/wukong-scheduler/src/executor.rs`

### 備註 / 偏離計畫處

- Task 1.1 原僅「新增 CI」，但 `-D warnings` gate 要求既有碼先達標，故一次性納入 fmt 正規化與既有 lint 清除——屬引入 gate 的必要步驟。
- clippy 意外揪出一個真實正確性缺陷（callback 鎖跨 await），已在本 Phase 順手修復並記錄，非單純風格修改。

---

## Phase 2 — render 高危修復 ✅（2026-07-07）

### Summary

修復 `wukong-render` 三個可實際觸發的缺陷，全部集中在 `crates/wukong-render/src/lib.rs`：CJK 分塊 panic、分塊標籤不平衡導致 Telegram 400、`javascript:`/`data:` 連結 XSS。新增 9 個測試（render crate 由 14 → 23 個測試）。

### Implemented Changes

**Task 2.1 + 2.2 — `split_chunks` 重寫（char boundary 安全 + 標籤平衡）**
- 舊版以 `rest.split_at(max)` 按位元組切割，切在多位元組字上會 panic；且按 `\n` 切割會把長 `<pre>`/`<blockquote>` 從開閉標籤之間切開。
- 新版改為 tokenizer 導向：`tokenize()` 把 HTML 拆成 `Tag`／`Text` token（因為 `render_html` 已逃逸所有文字中的 `<`/`>`，任何字面 `<` 必為真標籤，拆解無歧義）。維護「未閉合標籤堆疊」，塊尾以 `flush_balanced()` 補閉合、次塊開頭重開；`best_break()` 只在 char boundary 切割並優先斷在換行。含小 `max` 病態情形的前進保證。
- close/reopen 標籤的位元組數已計入每塊預算，確保含補標籤後仍 ≤ 4096。

**Task 2.3 — URL scheme allowlist**
- 新增 `is_safe_url()`：只放行 `http`/`https`/`mailto`/`tel` 與相對 URL，其餘（`javascript:`、`data:`、`vbscript:` 等）拒絕；忽略 scheme 內的空白／控制字元以防 `java\tscript:` 混淆。
- `to_web_html()`：攔截 `Tag::Link`／`Tag::Image`，不安全 scheme 者丟棄標籤但保留內文（連結文字仍顯示、不可點）。
- `render_html()`（Telegram）：同一 allowlist，防禦縱深。

### Verification

- `cargo test -p wukong-render` ✓（23 passed）
- `cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓、`cargo test --workspace --locked` ✓（無下游回歸）
- 對抗性測試輸入：9000 bytes 無換行中文段落（不 panic、各塊 ≤4096、字元零遺失）、超長 `<pre>`/`<blockquote>`（每塊標籤平衡）、`javascript:`/`data:` 連結（不產生可點 href、內文保留）。

### Modified Files

- Modify `crates/wukong-render/src/lib.rs`（is_safe_url、to_web_html/render_html scheme 過濾、split_chunks 重寫、9 個新測試）

---

## Phase 3 — 認證與部署加固 ✅（2026-07-07）

### Summary

將 Web Console 與 memoryd 的認證改為結構性防漏且 fail-closed，並修補 token 注入與 Telegram callback 白名單缺口。新增 8 個測試。

### Implemented Changes

**Task 3.1 — Web 認證 middleware + Bearer header**
- 新增 `require_token` axum middleware，套在「受保護路由群組」（`/chat` + 所有 `/api/*`）的 `route_layer`；`build_router` 拆成 public（靜態資產 + index shell）與 protected 兩組。這是單一 choke point，日後新增受保護路由自動被涵蓋。
- Token 來源支援 `Authorization: Bearer <t>` 標頭 **或** `?token=`；header 驗證通過後由 `ensure_query_token` 回填 query，讓既有 per-handler 檢查（保留為 defense-in-depth）對 header 客戶端也成立。
- `authorized` 改用常數時間比較 `ct_eq`。
- 決策：保留 28 處 per-handler 檢查（防禦縱深），不做高風險的大量刪除；middleware 為主閘門。

**Task 3.2 — Web 空 token fail-closed**
- 新增 `should_refuse_insecure_start(token, host, allow_insecure)`（可測純函式）；`main.rs` 在「空 token + 非 loopback + 未設 `WUKONG_WEB_ALLOW_INSECURE=1`」時拒絕啟動並輸出明確指引。
- `docker-compose.yml` 補 `WUKONG_WEB_ALLOW_INSECURE` 與安全註解；`docs/docker.md` 更新警示。**注意：既有以 0.0.0.0 + 空 token 部署者升級後需設 token 或 `WUKONG_WEB_ALLOW_INSECURE=1`。**

**Task 3.3 — memoryd 認證與預設綁定**
- `Config` 新增 `host`（預設 `127.0.0.1`，不再 `0.0.0.0`）與 `token`（`WUKONG_MEMORY_TOKEN`）。
- `build_router(mem, token)` 新增 bearer middleware，保護 `/v1/{stats,snapshot,remember,recall}`，`/v1/health` 維持公開（供 liveness）。
- 測試補 401（缺 token / 錯 token）、200（正確 token）、health 公開。

**Task 3.4 — 次要注入與白名單修補**
- `index()` token 注入改用 `serde_json` 序列化 + `<`/`>`/`&` → `\uXXXX`，防 `</script>` 突破 inline script。
- `handle_callback_query` 新增 `allow: &[i64]` 參數與 `is_allowed` 白名單檢查（與訊息處理一致）；`main.rs` callback 路徑傳入 `allow`；補「非白名單 callback 被忽略」測試。

### Verification

- `cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓（EXIT 0）、`cargo test --workspace --locked` ✓（438 passed）
- 新測試：web Bearer header accept/reject、fail-closed 邏輯、memoryd 401/200/health、Telegram 非白名單 callback 忽略。

### Modified Files

- Modify `crates/wukong-web/src/lib.rs`（middleware、router 拆分、ct_eq、index JSON 注入、fail-closed helper、5 個新測試）
- Modify `crates/wukong-web/src/main.rs`（fail-closed 啟動檢查）
- Modify `crates/wukong-memoryd/src/lib.rs`（Config host/token、bearer middleware）、`crates/wukong-memoryd/src/main.rs`、`crates/wukong-memoryd/tests/http.rs`（4 個新測試）
- Modify `crates/wukong-telegram/src/dispatch.rs`（callback 白名單 + 測試）、`crates/wukong-telegram/src/main.rs`
- Modify `docker-compose.yml`、`docs/docker.md`

### 備註 / 偏離計畫處

- Task 3.1 原計畫「移除各 handler 內的手動檢查」；改為「middleware 為主閘門 + 保留 per-handler 檢查為防禦縱深」，避免在 3647 行安全關鍵檔案上大量刪除造成回歸風險。header 支援以 query 回填達成，不動 28 個 handler 簽章。完整移除 per-handler 檢查併入 Phase 7 Task 7.5（web/lib.rs 拆分）時處理較安全。
- fail-closed 是刻意的破壞性變更（提升安全預設）；已在 compose／docs 標註升級遷移路徑。

---

## Phase 4 — 進入點行為修正 ✅（2026-07-07）

### Summary

修復 CLI backend 丟棄規劃意圖的行為分歧，並補齊 Telegram 傳輸層的錯誤處理、退避與 offset 保留。新增 6 個測試。

### Implemented Changes

**Task 4.1 — CLI backend 傳遞 `agent` 欄位**
- 確認 `opencode run --agent <name>` 為實際旗標。`assemble_argv` 新增 `agent: Option<&str>` 參數，在 model 之後、attachments 之前推入 `--agent <name>`（trim 後空字串略過）；`run` 與 `run_streaming` 兩路徑皆傳 `req.agent.as_deref()`。
- 消除兩 backend 分歧：orchestrator 的 `agent: Some("plan")` 現於預設 CLI backend 生效。
- 以假 agent 驗證整體流程不回歸（`--agent-cmd "printf fixer" "fix the bug"` → 正常輸出）。

**Task 4.2 — Telegram API `ok:false` 錯誤化與退避**
- `wukong-tg-client` 新增 `check_ok`，`post`／`get_updates` 皆套用：`ok != true` 時回 `Err(TgError::Api("telegram api error <code>: <desc>"))`，不再把 401/400 當成功。
- 主迴圈既有的 `Err → eprintln + sleep(3s)` 退避因此自動涵蓋失效 token（消除無退避忙迴圈）；401 額外輸出 token 失效提示 log。

**Task 4.3 — 送出錯誤 log 化與 offset 保留**
- dispatch.rs 新增 `log_send` helper，將答案／錯誤／指令回覆的 `let _ = client.send_*` 改為記 log（不改控制流；typing 等純裝飾送出維持靜默以免刷屏）。
- token 輪替不再 `offset = 0`，保留 cursor 避免重拉舊 update。
- schedulerd/notify.rs 本就以 `?` 傳遞錯誤，無需改動。

### Verification

- `cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓、`cargo test --workspace --locked` ✓（443 passed）
- 新測試：`assemble_argv` 帶 agent / 略過空白（2）、`check_ok` accept/reject/missing（3）、以及既有測試回歸。以假 agent E2E 驗證 orchestrator 流程。

### Modified Files

- Modify `crates/wukong-gateway/src/backend.rs`（assemble_argv agent 參數 + 2 測試）
- Modify `crates/wukong-tg-client/src/client.rs`（check_ok + 3 測試）
- Modify `crates/wukong-telegram/src/main.rs`（callback 已於 Phase 3 傳 allow；本階段：offset 保留、401 log）
- Modify `crates/wukong-telegram/src/dispatch.rs`（log_send helper + 套用內容送出）

---

## Phase 5 — memory 效能、一致性與計分 ⚠️部分（2026-07-07）

### Summary

消除 embedding 對 async executor 的阻塞與向量召回的 recency 截斷，並將批次寫入改為單一原子語句、連線加上 busy_timeout。**Task 5.5（confidence）刻意延後**（見備註）。新增 1 個測試。

### Implemented Changes

**Task 5.1 — embedding 走 `spawn_blocking`**
- 新增 `embed_blocking(embedder, text)`：clone `Arc<dyn Embedder>` + 文字進 `tokio::task::spawn_blocking`，join 錯誤映射為 `MemoryError::Embed`。
- `remember`、`recall` 的同步 `emb.embed` 改走 `embed_blocking`；`backfill` 改收 `&Arc<dyn Embedder>` 並把 `embed_batch` 包進 spawn_blocking（兩個呼叫端同步更新）。ONNX 推論不再阻塞 tokio worker。

**Task 5.2 — 向量召回去 recency 截斷**
- `recall` 改以 `MAX_VECTOR_SCAN = 10_000` 取代 `fetch_limit`（約 50）呼叫 `embedded_candidates`，避免「舊但語意最相關」記憶在排序前被 recency 截斷。`apply_vector_sims` 本就會 append vector-only 候選（`recall/mod.rs:251`），故舊記憶得以浮現。
- 新增整合測試 `vector_recall_is_not_truncated_by_recency`（1 個舊相關 + 60 個新無關；query 僅經向量源可達）。

**Task 5.3 — N+1 批次化**
- `touch_recalled`、`mark_consolidated`、`delete_memories` 改為單一 `... WHERE id IN (?,?,…)` 語句（原子、單次 round-trip）；新增 `sql_placeholders(n)` helper；空輸入早退。

**Task 5.4 — 連線池寫入韌性**
- `Store::open` 的 `SqliteConnectOptions` 加 `busy_timeout(5s)`：背景 backfill 與前景寫入重疊時，寫入者等待而非立即 `SQLITE_BUSY`。（採 busy_timeout 而非 `max_connections(1)`，保留 WAL 的讀並發。）

### Verification

- `cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓、`cargo test --workspace --locked` ✓（444 passed）
- 既有 touch/delete/consolidate/semantic 測試守護 N+1 與 spawn_blocking 改動不回歸。

### Modified Files

- Modify `crates/wukong-memory/src/lib.rs`（embed_blocking、MAX_VECTOR_SCAN、remember/recall/backfill）
- Modify `crates/wukong-memory/src/store/mod.rs`（busy_timeout、IN 批次化、sql_placeholders）
- Modify `crates/wukong-memory/tests/integration.rs`（recency 截斷測試）

### 備註 / 偏離計畫處

- **Task 5.5（confidence 退化修正）延後**：`docs/superpowers/plans/2026-07-05-memory-optimization-parity.md` 的「Task 4: Recall Telemetry And Confidence Relevance」正在重做 confidence 語意（`recall_confidence_uses_decay_free_relevance`、`confidence == explanation.relevance`）。為避免與該進行中計畫衝突／重工，5.5 待該計畫落地後再依其成果調整。
- 向量全量掃描設 10_000 上限：超大 store 仍會截斷最舊列，已於程式碼註記；真正的 ANN／sqlite-vec 另立設計（原計畫已載明本任務不做）。

---

## Phase 6 — 共用化與死碼清理 ✅（2026-07-07，v0.16.35 發佈後）

> 於 Phase 1–5 合併並發佈 `v0.16.35` 之後進行；本 Phase 以獨立分支承載（大型跨 crate 重構，適合單獨 review／PR）。

### Summary

把跨進入點複製貼上的膠水碼下沉到 `wukong-runtime` 共用模組，並移除 v1 遺留死碼。執行前先以精確測繪校正 decomposition 的依賴假設與重複數量（見「偏離計畫處」），只對**真實重複**下手，避免為了對齊誇大的數字而改壞分歧行為。全程 `fmt`／`clippy -D warnings`／`test` 綠燈（446 passed）。

### Implemented Changes

**Task 6.1 — 共用 util／bootstrap 模組**
- 新增 `crates/wukong-runtime/src/util.rs`：`now_unix`、`upload_root`、`default_db_url`、`db_url_from_env`、`agent_command_from_env`（純 std，零新依賴）+ 4 個單元測試。
- 新增 `crates/wukong-runtime/src/bootstrap.rs`：`open_memory_from_env(db_url)` 封裝「`Memory::open` → embed gate（`WUKONG_EMBED`）→ markdown（`WUKONG_MD_DIR`）」三步。
- `wukong-runtime/Cargo.toml` 新增 `embed = ["wukong-memory/embed"]`；`wukong-cli`／`wukong-web`／`wukong-telegram`／`wukong-schedulerd` 的 `embed` feature 皆補上 `wukong-runtime/embed` 透傳。

**Task 6.2／6.3 — 進入點改用共用 util**
- `now_unix`／`upload_root` 下沉：`wukong-cli`、`wukong-web`（lib.rs）、`wukong-telegram`（dispatch.rs）、`wukong-schedulerd`（main.rs、notify.rs）改 `use wukong_runtime::util::…`，刪除各自本地副本。
- memory bootstrap 下沉：cli/web/telegram/schedulerd 四處 `Memory::open + embed + markdown` 區塊改呼叫 `bootstrap::open_memory_from_env`。
- `agent_command_from_env`／`db_url_from_env` 取代 web／telegram main.rs 的 `WUKONG_AGENT_CMD`／`WUKONG_MEMORY_DB` 重複區塊；schedulerd `resolve_config` 改用共用 `default_db_url`。
- `wukong-web`／`wukong-telegram` 補 `wukong-runtime` 直接依賴（原僅透過 `wukong-cli` 間接依賴；無循環）。

**Task 6.4 — 排程執行編排收斂**
- `wukong-scheduler` 新增 `run_claimed_job()` + `ClaimedJobOutcome{Completed,LeaseLost}`，封裝 `start_run → execute → finish_run → complete_claimed_job`（含 lease 檢查）。
- `wukong-cli::trigger_job` 與 `wukong-schedulerd::run_scan` 改用之，各自保留原尾巴（cli：println + Ok/Err；schedulerd：eprintln + continue + Telegram 通知）。lease 語意不變（呼叫順序完全相同）。

**Task 6.5 — CLI 串流渲染統一 + REPL 逐回合 reload**
- `run_one` 串流分支改用 `render::StreamRenderer`（原內嵌閉包與其 `on_event` 路由完全相同），消除「渲染器閒置、閉包重複」的壞味道。
- 真實 stdin REPL 的 Turn 分支補上**逐回合 settings 重載**（`apply_settings_to_config`），修掉「REPL 中 `/set_models` 後下一題不生效」的真實不一致。

**Task 6.6 — 移除 `gateway/pipeline.rs` 死碼**
- 確認零 production 呼叫者（`pub mod pipeline` 無 re-export、`grep pipeline::` 無匹配）、已被 `wukong_runtime::run_turn` 取代 → 刪除模組與 `lib.rs` 匯出。
- 協調 `2026-07-05-memory-optimization-parity` 計畫（39 checkbox 全未執行）：移除其 4 處 `pipeline.rs` 引用，導向它已涵蓋的 `runtime/turn.rs`（唯一活路徑），並加日期註記。

### Verification

- `cargo build --workspace` ✓、`cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓、`cargo test --workspace --locked` ✓（**446 passed**，原 444：+4 util 測試、−2 pipeline 自帶測試）。
- lease 併發、REPL loop、scheduler executor 既有測試守護行為不回歸。

### Modified Files

- Create `crates/wukong-runtime/src/util.rs`、`crates/wukong-runtime/src/bootstrap.rs`
- Modify `crates/wukong-runtime/src/lib.rs`、`crates/wukong-runtime/Cargo.toml`
- Modify `crates/wukong-scheduler/src/executor.rs`、`crates/wukong-scheduler/src/lib.rs`
- Modify `crates/wukong-cli/src/main.rs`、`crates/wukong-cli/Cargo.toml`
- Modify `crates/wukong-web/src/main.rs`、`crates/wukong-web/src/lib.rs`、`crates/wukong-web/Cargo.toml`
- Modify `crates/wukong-telegram/src/main.rs`、`crates/wukong-telegram/src/dispatch.rs`、`crates/wukong-telegram/Cargo.toml`
- Modify `crates/wukong-schedulerd/src/main.rs`、`crates/wukong-schedulerd/src/notify.rs`、`crates/wukong-schedulerd/Cargo.toml`
- Delete `crates/wukong-gateway/src/pipeline.rs`；Modify `crates/wukong-gateway/src/lib.rs`
- Modify `docs/superpowers/plans/2026-07-05-memory-optimization-parity.md`

### 備註 / 偏離計畫處

- **依賴假設校正**：decomposition 假設 `wukong-scheduler` 在 `wukong-runtime` 之下、須保留本地副本；實際上 scheduler **依賴** runtime（在其之上）。真正在 runtime 之下、保留本地 `now_unix` 的只有 `wukong-memory`、`wukong-chat-history`（依賴方向不可倒灌）。
- **`apply_settings_to_config` 不搬 runtime**：decomposition 稱其重複 5 處，實測只有 1 處（cli）；且各進入點的 settings 套用**行為本就分歧**（cli 全套、schedulerd 只 model、web／telegram 不套）。強制統一會改變行為（非純 dedup），故保留於 cli，僅由 REPL 逐回合 reload 復用，避免給 runtime 增加 `wukong-settings` 依賴。
- **Task 6.5 完整 loop 合一延後**：`run_repl_loop` 是「消費 iterator + 遇錯即中止」語意，真實 stdin REPL 是「互動式提示 + 遇錯續跑」；直接替換會改變錯誤韌性與互動行為。故本次只落地計畫的真實意圖（`StreamRenderer` 復用 + 逐回合 reload），完整 loop 合一待重整 `run_repl_loop` 契約時再做。
- **embed feature 本地未能完整編譯驗證**：`cargo check -p wukong-runtime --features embed` 於 `openssl-sys`（fastembed→hf-hub 的原生傳遞依賴）build script 失敗，屬 sandbox 缺 OpenSSL 的環境限制，非程式碼問題；cargo 已成功解析並開始建置 fastembed，證明 feature 透傳佈線正確，且搬移的 cfg 區塊與原可編譯之進入點程式碼逐字相同。embed 於 CI／release 皆不啟用。

---

## Phase 7 — 治理與結構整理（部分完成，2026-07-07，v0.16.36 發佈後）

> 本 Phase 異質性高（治理 + 3 個獨立大檔拆分）。治理／正確性項目於此 commit 完成；三個純結構的大檔拆分（7.4/7.5/7.6）體量大、彼此獨立，各自更適合獨立 PR，暫緩。

### Implemented Changes

**Task 7.1 — CHANGELOG**
- 新增 `CHANGELOG.md`（Keep a Changelog 格式），自 `v0.16.35` 起維護，含 v0.16.35／v0.16.36 條目與 compare 連結；更早版本指向 GitHub Releases。維護方式（發佈前整理 `Unreleased`）寫在檔頭。

**Task 7.2 — 文件與依賴 pin 同步**
- `Dockerfile`：`ARG VERSION` 由過期的 `v0.16.27` 更新為 `v0.16.36`，並註明 release workflow 會覆寫、本地可用 `--build-arg VERSION=` 指定。
- opencode 依賴可 pin：新增 `ARG OPENCODE_VERSION=latest`，`npm install -g "opencode-ai@${OPENCODE_VERSION}"`（預設 latest，可於 build 時鎖版），不硬編可能過期／破壞的版本。
- （README 測試徽章、CLAUDE.md 15 crate + `wukong-chat-history` 已於前批文件同步處理。）

**Task 7.3 — healthcheck**
- `wukong-web` 新增免認證 `/healthz`（回 200、空 body、屬 public 路由群，繞過 auth middleware）+ 測試 `healthz_is_public_and_returns_200_even_with_token_set`。
- `docker-compose.yml`：為 `wukong-web` 加 `healthcheck`（`CMD-SHELL` 內插 `WUKONG_WEB_PORT`，curl 已在 image 內）。
- memoryd 的 `/v1/health` 本就存在且公開；惟 memoryd 未納入本 compose，故 compose 側只加 web。

**Task 7.7 — server backend text 串流行為決議**
- 確認為**刻意設計**：server backend（`opencode serve`）的 `map_server_event` 對 `text` part 走 `_ => Ignore`，最終回答文字於 `run` 收尾由 `list_messages` + `extract_latest_assistant_text` 一次取回；若同時吐 text delta 會重複渲染。
- 於 `opencode_server.rs` 該 match arm 加註解說明；於 `docs/entrypoints.md` 新增「兩種 agent backend 的串流行為差異」段落記錄（CLI 逐字串流 vs server 收尾整段）。

### Verification

- `cargo fmt --all -- --check` ✓、`cargo clippy --all-targets --locked -- -D warnings` ✓、`cargo test --workspace --locked` ✓（**447 passed**，+1 healthz 測試）。
- `docker compose config -q` ✓（compose 檔含新 healthcheck 仍有效）。

### Modified Files

- Create `CHANGELOG.md`
- Modify `Dockerfile`、`docker-compose.yml`
- Modify `crates/wukong-web/src/lib.rs`（healthz handler + route + test）
- Modify `crates/wukong-gateway/src/opencode_server.rs`（text-ignore 決議註解）
- Modify `docs/entrypoints.md`（backend 串流差異）

### 大檔拆分（各一 commit，純搬移、以測試守門）

- **Task 7.4 ✅ 拆分 `opencode_server.rs`**（commit `aee5a44`）：1401 → 640 行，抽出 `opencode_server/sse.rs`（`SseParser`，57 行）與 `opencode_server/event_map.rs`（`ServerEventAction` + `map_server_event` + tool/question 格式化解析，721 行）。跨模組項用 `pub(super)`；`cargo test -p wukong-gateway` 70 passed 不變。
- **Task 7.5 ✅ 拆分 `web/lib.rs`**（commit `2292861`）：3772 → 2955 行，依既有 `*_api.rs` 慣例抽出 `static_assets.rs`（include_str! 資產 + handler，80 行）與 `chat_api.rs`（chat/attachment/SSE handler，794 行）。`AppState`／`build_router`／`index`／`healthz`／auth 留 lib.rs，共用 helper 提 `pub(crate)`；web 74 passed 不變。
- **Task 7.6 ⏸ 拆分 `wukong-chat.js`（延後，不做）**：經評估，此檔**整檔為單一 `WukongChat extends HTMLElement` 類別（16–1037 行），無任何模組級函式**——SSE 處理全是與 `this` 緊耦合的實例方法。要「抽出 SSE 群組」需把實例方法重構為 free function／mixin 並手動穿引 `this`，屬**結構性重構而非純搬移**；且唯一的 `wukong-chat.test.mjs` 是**正規表達式比對原始碼字串**（非執行期行為測試），方法一旦搬出反而使該測試失效，等於幾乎無安全網。此項為整份計畫**最低價值、最高風險**的變更，故**不在此輪強行進行**，建議待補上元件執行期測試後另立專門工作處理。

> 7.4／7.5 皆為純結構搬移、無行為變更，各自一個 commit、以 `cargo test` + `clippy -D warnings` 全綠守門並各自合併回 main。
