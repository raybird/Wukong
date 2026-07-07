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
