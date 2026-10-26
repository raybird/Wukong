# 審查報告

- 範圍：局部 diff（Issue #24 第二輪審查），TARGET=`main`
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`
- Reviewed HEAD SHA：`66ad52ae59ad97658b81bfc9ed07421f40b33560`
- Reviewed patch-id：`8689d9d2dbef3f3ba5331207e6884a99930450a0`（2026-10-26 以 `git diff BASE HEAD | git patch-id --stable` 計算，與預期相同）
- 增量 diff（7d1acc8..66ad52a）patch-id：`044c98dc2560326be4c6f1d6425622364ce2dbf7`（2026-10-26 以 `git diff 7d1acc8 HEAD | git patch-id --stable` 計算，與預期相同）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），非原實作者；審查日期 2026-10-26
- Review artifact：`docs/issues/issue-0024/review-66ad52a.md`

## 問題與風險

### MUST FIX
- 無。

### SHOULD FIX
- 無。

### NICE TO HAVE
- 無。

## 前次審查意見覆核（7d1acc8 -> 66ad52a）

- **M-1 `parse_local_stream_error` 在提取 `error.error` 時遇跳脫引號（`\"`）截斷訊息**：**已修復**。
  - 實作分析：在 `crates/wukong-gateway/src/local_process.rs:370-382` 新增 `find_closing_quote` 輔助函數，以狀態機追蹤跳脫字元 `\`，正確跳過轉義雙引號 `\"` 以及成對反斜線（如 `\\`），直到遇見未轉義的結尾雙引號 `"` 為止。
  - 單元測試：新增 `parse_local_stream_error_extracts_json_payload_without_truncating_escaped_quotes` 單元測試，證實包含 JSON 結構（`{\"message\": \"Free usage limit reached\"}`）的錯誤訊息完整保留，且成功歸類為 `UpstreamFailure::RateLimited` (429)。
  - 邊界驗證：經 UTF-8 多位元組字元探測與極端字串驗證，切片皆在 ASCII 引號字元邊界上，無 panic 或截斷風險。
- **S-1 `parse_local_stream_error` 增加日誌等級檢查（避免一般日誌誤觸）**：**已修復**。
  - 實作分析：`local_process.rs:325` 增加 `is_error_level = line.contains("level=ERROR") || line.contains("level=error")`，並要求同時滿足 `is_error_level && is_stream_error` 才進入串流錯誤解析流程。
  - 單元測試：新增 `parse_local_stream_error_ignores_non_error_levels` 單元測試，驗證 `level=INFO message="stream error"` 會回傳 `None`，不中斷正常回合。
- **S-2 `classify_text` 的配額關鍵字比對支援忽略大小寫**：**已修復**。
  - 實作分析：`crates/wukong-gateway/src/upstream_error.rs:145` 使用 `contains_any_ignore_ascii_case` 進行大小寫不敏感匹配，涵蓋 `usage limit reached` 之所有大小寫變體。
  - 單元測試：`upstream_error::tests` 補充 `classify_text("error: USAGE LIMIT REACHED FOR CURRENT TIER")`，驗證全大寫字串正確辨識為 `RateLimited`。
- **N-1 `tokio::select!` 加上 `biased;` 優先處理串流錯誤**：**已實作**。
  - 實作分析：`local_process.rs:178-181` 加入 `biased;`，並將 `stream_err_rx.recv()` 置於第一優先分支，確保串流錯誤一旦到達立即返回，避免與 run future 完成時產生不確定競爭。
- **N-2 補齊 `parse_local_stream_error` 單元測試**：**已實作**。
  - 實作分析：於 `crates/wukong-gateway/src/local_process.rs` 補齊 JSON payload 解析與非 ERROR 日誌忽略兩項獨立單元測試，使 `wukong-gateway` 單元測試總數由 132 項增至 134 項。

## 已查核維度

### 驗收與證據

- **規格修訂與核准**：
  - 核准 commit 為 `3e12253`，依 2026-10-26 唯讀事故調查授權建檔。
  - SCN-001 與 SCN-002 已核准。
  - SCN-003（恢復正式排程與更換模型）維持「待核准」狀態，無任何越權實作。
- **SCN-001**：
  - 契約測試 `upstream_quota_error_in_local_process_fails_immediately_with_cause`（`crates/wukong-gateway/src/local_process.rs:484`）：模擬 OpenCode 1.19.3 行為，斷言回合在 2 秒內中斷（實際耗時約 0.08 秒），錯誤包含原始額度訊息，且子行程正確終止回收。
  - 單元測試 `parse_local_stream_error_extracts_json_payload_without_truncating_escaped_quotes`（`crates/wukong-gateway/src/local_process.rs:457`）：驗證含 JSON 結構（`{\"message\": \"...\"}`）的錯誤訊息完整擷取，無截斷問題，且狀態碼為 429。
  - 整合測試 `turn_job_reports_quota_exhaustion_failure_with_cause`（`crates/wukong-scheduler/src/executor.rs:1924`）：驗證排程 run 結果標記為 failure、包含「達到請求頻率或額度上限」分類標籤及額度原始原因。
  - 結論：成立且邊界覆蓋完整。
- **SCN-002**：
  - 正常回覆回歸測試全數通過（`wukong-gateway` 134 passed、`wukong-scheduler` 51 passed）。
  - 完全無回應逾時機制維持既有 `deadline` 與逾時處理，未受 stderr 監聽干擾。
  - 結論：成立。
- **SCN-003**：
  - 待核准，本次無實作程式碼，符合計畫。
- **全量檢查**：
  - `cargo fmt --all -- --check` 通過。
  - `cargo clippy --all-targets --locked -- -D warnings` 通過。
  - `cargo test --workspace --locked` 全數通過（含 wukong-gateway 134 passed、wukong-scheduler 51 passed、wukong-runtime 34 passed、wukong-cli 27 passed、wukong-web 77 passed 等共 670+ 項測試）。

### 相關失敗面

| 失敗面 | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|---|
| 上游額度耗盡 | OpenCode stderr 輸出 `Free usage limit reached` | 立即失敗，保留原因，標記 RateLimited (429) | gateway 契約測試 + scheduler 整合測試 | 通過 |
| 錯誤訊息含轉義雙引號（JSON） | `error.error="...{\"message\": \"...\"}"` | 完整提取 JSON 訊息，不被截斷 | gateway 單元測試 + probe 驗證 | 通過（M-1 已解決） |
| 錯誤訊息含路徑反斜線 | `error.error="path \\\"C:\\\\foo\\\" usage limit reached"` | 成對反斜線正確處理，不截斷 | probe 驗證通過 | 通過 |
| 錯誤訊息無閉合引號 | `error.error="unclosed string` | fallback 返回全字串，不 panic | probe 驗證通過 | 通過 |
| 錯誤訊息含多位元組 UTF-8 | `error.error="AI_APICallError: {\"message\": \"額度已用盡\"}"` | 切片安全落於字元邊界，不 panic | probe 驗證通過 | 通過 |
| 非 ERROR 級別日誌含關鍵字 | `level=INFO message="stream error"` | 不中斷正常回合，回傳 None | gateway 單元測試通過 | 通過（S-1 已解決） |
| 配額關鍵字大小寫變體 | `USAGE LIMIT REACHED`、`Usage Limit Reached` | 正確識別為 RateLimited | upstream_error 單元測試通過 | 通過（S-2 已解決） |
| 正常無回應 | 模型無任何回應且無日誌輸出 | 逾時保護正常觸發（agent timeout） | 既有 deadline 機制維持 | 通過 |
| 正常完成 | 模型正常串流回覆並收尾 | 正常完成回合 | 既有整合測試通過 | 通過 |
| 進程異常退出或被取消 | 執行途中取消 future | 子進程與 stderr drain task 正確釋放 | `Drain` 與 `Process::stop` 正確終止 | 通過 |
| 管道阻塞 (Pipe buffer overflow) | 子進程產生大量 stderr / stdout | 管道持續讀取，不阻塞子進程 | stdout / stderr 皆有背景讀取 task | 通過 |

### 需求、架構、安全、品質

- **架構**：
  - 遵循 gateway → runtime → scheduler 單向相依。
  - 錯誤判定集中於 `upstream_error.rs`，結構清晰。
- **安全與邊界**：
  - 子進程生命週期妥善管理：`Process` 封裝 child 與 pid，stop 時依序發送 SIGTERM 與 SIGKILL，並終止整個 process group。
  - `Drain` struct 實作 `Drop` 自動呼叫 JoinHandle `abort()`，無非同步 task 外洩風險。
  - `find_closing_quote` 使用 `char_indices` 返回安全字元邊界，切片無 panic 風險。
- **品質與簡潔度**：
  - 手術式修改，修復精確聚焦，無過度設計或多餘抽象。

### 豁免、待確認與限制

- 無 gate 豁免。
- TBD-1（模型額度恢復或替代模型決策）與 SCN-003 維持待確認狀態，不影響本次錯誤處理基礎設施之交付。
- TBD-2 已於先前步驟完成。

### 本次實驗（Review Probes）

| 實驗項目 | 測試輸入 | 程式碼執行輸出 | 判定 |
|---|---|---|---|
| Probe 1: 含轉義 JSON 訊息（覆核 M-1） | `error.error="AI_APICallError: {\"message\": \"Free usage limit reached\"}"` | `AI_APICallError: {\"message\": \"Free usage limit reached\"}` | 完整提取，無截斷 |
| Probe 2: 配額大小寫變體（覆核 S-2） | `classify_text("error: USAGE LIMIT REACHED FOR CURRENT TIER")` | `Some(UpstreamFailure::RateLimited)` | 成功匹配 |
| Probe 3: 非 ERROR 等級日誌（覆核 S-1） | `level=INFO message="stream error" error.error="not an actual failure"` | `None` | 正確忽略 |
| Probe 4: 多位元組 UTF-8 與成對反斜線 | `error.error="AI_APICallError: {\"message\": \"額度耗盡 🚀: Free usage limit reached\"}"` | `AI_APICallError: {\"message\": \"額度耗盡 🚀: Free usage limit reached\"}` | 成功提取，邊界安全 |

## 流程判定

PASS

理由：第一輪審查提出的 M-1（處理轉義雙引號 `\"`）已被正確修復，S-1（日誌等級檢查）、S-2（配額關鍵字大小寫不敏感）、N-1（biased 分支優先權）及 N-2（獨立單元測試）亦已完整落實。經單元測試、整合測試、全量回歸測試（670+ 項測試）與獨立 probe 驗證，所有功能與邊界案例均表現正確，無任何 MUST FIX。
