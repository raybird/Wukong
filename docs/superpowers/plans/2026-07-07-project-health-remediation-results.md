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
