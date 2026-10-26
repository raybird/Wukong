# 審查報告

- 範圍：局部 diff（Issue #24），TARGET=`main`
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`
- Reviewed HEAD SHA：`7d1acc85235ff94b4f34f0087ee840a97568b38d`
- Reviewed patch-id：`0675ff25b143c8e5263747e471ecbca4f706b03a`（2026-10-26 以 `git diff BASE HEAD | git patch-id --stable` 計算，與預期相同）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），非原實作者；審查日期 2026-10-26
- Review artifact：`docs/issues/issue-0024/review-7d1acc8.md`

## 問題與風險

### MUST FIX

- **M-1 `parse_local_stream_error` 在提取 `error.error` 時未處理跳脫引號（escaped quotes `\"`），會截斷包含 JSON 的錯誤訊息並導致分類降級**（`crates/wukong-gateway/src/local_process.rs:325-331`）
  - 現況：
    ```rust
    let detail = if let Some(idx) = line.find("error.error=\"") {
        let rest = &line[idx + "error.error=\"".len()..];
        if let Some(end) = rest.find('"') {
            rest[..end].to_string()
        } else {
            rest.to_string()
        }
    ...
    ```
    `rest.find('"')` 僅比對第一個出現的字元 `"`，未判斷前驅字元是否為反斜線 `\`。
  - 實驗（review probe）：
    模擬常見包含 JSON payload 的上游 provider 錯誤：
    - `sample1`：`level=ERROR message="stream error" error.error="AI_APICallError: Free usage limit reached. Free models work in OpenCode"`
      - 解析結果：`AI_APICallError: Free usage limit reached. Free models work in OpenCode`（原測試字串，無引號，可通過）
    - `sample2`：`level=ERROR message="stream error" error.error="AI_APICallError: {\"message\": \"Free usage limit reached\"}"`
      - 解析結果：`AI_APICallError: {\`（截斷）
      - 分類結果：`classify_text(&detail)` 為 `None`，退化為 `UpstreamFailure::Other`，`status_code` 變成 `None`。
    - `sample3`：`level=ERROR message="stream error" error.error="Provider returned error: {\"error\": \"usage limit reached\"}"`
      - 解析結果：`Provider returned error: {\`（截斷）
      - 分類結果：`classify_text(&detail)` 為 `None`，退化為 `UpstreamFailure::Other`。
  - 影響：
    上游模型 provider（如 OpenAI、Anthropic、Google 等）回傳 429 或限流錯誤時，其 HTTP response body 通常為 JSON 結構，被 OpenCode 序列化進 logfmt 的 `error.error="..."` 欄位中，內部的引號必然會被轉義為 `\"`。現有程式碼會在第一個 `\"` 處立即截斷，導致：
    1. 錯誤詳細內容遺失，只剩殘破字串 `...: {\`，使用者與排程日誌無法得知真實原因；
    2. SCN-001 要求「排程結果為 failure 且包含額度不足的原因」，在包含 JSON 格式錯誤時失效；
    3. 截斷字串使關鍵字遺失，錯誤分類由 `RateLimited`（429）降級為 `Other`（上游錯誤）。
  - 建議：
    尋找閉合雙引號時應跳過反斜線轉義（unescaped quote），例如掃描字元時記錄轉義狀態，找到第一個未被轉義的 `"` 作為閉合邊界；或反轉義後還原完整訊息。並於 `local_process.rs` 增加 `parse_local_stream_error` 針對帶轉義雙引號之 JSON 錯誤訊息的單元測試，先確認測試在現行程式碼下失敗再修復。

### SHOULD FIX

- **S-1 `parse_local_stream_error` 未檢查 `level=ERROR`，存在一般日誌或非錯誤訊息誤觸風險（地雷 W-7）**（`crates/wukong-gateway/src/local_process.rs:324`）
  - 現況：比對條件僅為 `if line.contains("message=\"stream error\"") || line.contains("message='stream error'")`，若未匹配到已知錯誤分類，後續無條件 `unwrap_or(UpstreamFailure::Other)`。
  - 風險：若 OpenCode 擴充套件、plugin 或特定環境的 debug/info 日誌碰巧印出該片段，或排程/對話涉及除錯含有該字串之文字時，若該行出現在 stderr，會導致回合被無條件判定為上游失敗並中止。
  - 建議：增加日誌等級過濾，例如要求 `line.contains("level=ERROR") || line.contains("level=error")`，使判定更具結構性，降低誤殺機率。

- **S-2 `classify_text` 的配額關鍵字比對建議支援忽略大小寫**（`crates/wukong-gateway/src/upstream_error.rs:148-150`）
  - 現況：文字比對清單列出 `"usage limit reached"` 與 `"Usage limit reached"`，對於全大寫 `"USAGE LIMIT REACHED"` 或首字皆大寫之變體無法涵蓋。
  - 建議：比對前轉換為全小寫，或增加常見大小寫變體，提升對不同 provider 回應風格的容錯能力。

### NICE TO HAVE

- **N-1 `crates/wukong-gateway/src/local_process.rs:177` 的 `tokio::select!` 建議加上 `biased;`**
  - 現況：`tokio::select!` 預設以偽隨機順序輪詢。若 `run` 與 `stream_err_rx.recv()` 同步就緒，加上 `biased;` 並將 `stream_err_rx` 置於優先位置可確保串流錯誤絕對優先被捕捉，行為更具決定性（deterministic）。
- **N-2 補齊 `local_process.rs` 中 `parse_local_stream_error` 的獨立單元測試**
  - 現況：目前測試僅有透過 python mock server 的整合契約測試，缺少對字串解析 helper 函數各種邊界輸入（無引號、轉義引號、無 error.error 欄位、非錯誤訊息等）的直接單元測試。

## 已查核維度

### 驗收與證據

- **規格修訂與核准**：
  - 核准 commit 為 `3e12253`，依 2026-10-26 唯讀事故調查授權建檔。
  - SCN-001 與 SCN-002 已核准。
  - SCN-003（恢復正式排程與更換模型）維持「待核准」狀態，無任何越權實作（未修改 `/data/settings.json`，未引入自動 fallback 或 retry 機制）。
- **SCN-001**：
  - 契約測試 `upstream_quota_error_in_local_process_fails_immediately_with_cause`（`crates/wukong-gateway/src/local_process.rs:436`）：以 Python TCPServer 模擬 OpenCode 1.19.3 行為（`/event` 保持連線開啟但無事件，`/prompt_async` 回應 204 並在 stderr 輸出 stream error）。斷言回合在 2 秒內中斷（小於 3 秒逾時設定），且錯誤包含原始額度訊息，並確認子行程被正確終止回收。
  - 整合測試 `turn_job_reports_quota_exhaustion_failure_with_cause`（`crates/wukong-scheduler/src/executor.rs:1924`）：以 MockBackend 回傳 `RateLimited` 錯誤，驗證排程 run 結果標記為 failure、包含「上游限流」標籤及額度原始原因。
  - 結論：在無跳脫引號的標準 payload 下驗證有效；但如 M-1 所述，在帶有轉義引號之 JSON payload 下存在截斷缺陷。
- **SCN-002**：
  - 正常回覆回歸測試全數通過（`wukong-gateway` 132 passed、`wukong-scheduler` 51 passed）。
  - 完全無回應逾時機制維持既有 `deadline` 與逾時處理（`本機 OpenCode 回合逾時，已中止程序。`），未受新加入之 stderr 監聽干擾。
  - 結論：成立。
- **SCN-003**：
  - 待核准，本次無實作程式碼，符合計畫。
- **全量檢查**：
  - `cargo fmt --all -- --check` 通過。
  - `cargo clippy --all-targets --locked -- -D warnings` 通過。
  - `cargo test --workspace --locked` 全數通過（含 wukong-gateway, wukong-scheduler, wukong-runtime, wukong-cli, wukong-web 等 670+ 項測試）。

### 相關失敗面

| 失敗面 | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|---|
| 上游額度耗盡 | OpenCode stderr 輸出 `Free usage limit reached` | 立即失敗，保留原因，標記 RateLimited (429) | gateway 契約測試 + scheduler 整合測試 | 通過（無引號情境） |
| 錯誤訊息含轉義引號 | `error.error="...{\"message\": \"...\"}"` | 完整提取 JSON 訊息，不被截斷 | 無測試覆蓋；本次 probe 證實截斷為 `{\` | **M-1** |
| 正常無回應 | 模型無任何回應且無日誌輸出 | 逾時保護正常觸發（agent timeout） | 既有 deadline 機制維持 | 通過 |
| 正常完成 | 模型正常串流回覆並收尾 | 正常完成回合 | 既有整合測試通過 | 通過 |
| 非 ERROR 級別日誌含關鍵字 | `level=INFO message="stream error"` | 不中斷正常回合 | 無等級過濾，會誤判為 UpstreamFailure::Other | S-1 |
| 進程異常退出或被取消 | 執行途中取消 future | 子進程與 stderr drain task 正確釋放 | `Drain` 與 `Process::stop` 正確終止 | 通過 |
| 管道阻塞 (Pipe buffer overflow) | 子進程產生大量 stderr / stdout | 管道持續讀取，不阻塞子進程 | stdout / stderr 皆有背景讀取 task | 通過 |

### 需求、架構、安全、品質

- **架構**：
  - 遵循 gateway → runtime → scheduler 單向相依。
  - 錯誤判定集中於 `upstream_error.rs`，分類定義清晰。
- **安全與邊界**：
  - 子進程生命週期妥善管理：`Process` 封裝 child 與 pid，stop 時依序發送 SIGTERM 與 SIGKILL，並終止整個 process group。
  - `Drain` struct 實作 `Drop` 自動呼叫 JoinHandle `abort()`，無非同步 task 外洩風險。
- **品質與重複性**：
  - 變更極度專注，僅動 gateway stderr 監聽與關鍵字分類，無多餘抽象或推測性改動。

### 豁免、待確認與限制

- 無 gate 豁免。
- TBD-1（模型額度恢復或替代模型決策）與 SCN-003 維持待確認狀態，不影響本次錯誤處理基礎設施之交付。
- TBD-2 已由步驟 1 調查完成（確認 1.19.3 在 stream error 時連線掛起，需由 stderr 擷取）。

### 本次實驗

| 實驗項目 | 測試輸入 | 現行程式碼輸出 | 判定 |
|---|---|---|---|
| Probe 1: 無引號錯誤訊息 | `error.error="AI_APICallError: Free usage limit reached..."` | `AI_APICallError: Free usage limit reached...` | 正常 |
| Probe 2: 含轉義 JSON 訊息 | `error.error="AI_APICallError: {\"message\": \"Free usage limit reached\"}"` | `AI_APICallError: {\` | 截斷（**M-1**） |
| Probe 3: 429 含轉義 JSON | `error.error="Provider returned status 429: {\"error\": \"usage limit reached\"}"` | `Provider returned status 429: {\` | 截斷（**M-1**） |

## 流程判定

RETURN TO execute-task

理由：存在 **M-1**（解析 `error.error` 時遇轉義雙引號 `\"` 截斷詳細訊息與分類退化）。請於 `local_process.rs` 中補齊對跳脫引號的正確處理，並新增針對此邊界的單元測試（確保修正前紅燈、修正後綠燈），再行提交審查。S-1 與 S-2 建議一併考量評估。
