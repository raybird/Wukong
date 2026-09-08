# 上游錯誤靜默失效修復規劃

日期：2026-09-08

關聯事故文件：`docs/2026-09-08-model-eol-silent-failure-handover.md`（來源：TeleNexus v2.27.2）

基準版本：HEAD `57eceee`；部署 `v0.21.9`；opencode `1.18.18`

## 背景

TeleNexus 2026-09-08 修掉一個「模型被上游下架但所有監控全綠」的事故：模型 EOL 時
opencode 吞掉 `AI_APICallError`、吐一段降級文字、以 `exit 0` 收場，任何「不是 0 就算
失敗」的判定都會記成成功。他們因此靜默九天。詳見 handover。

本文件在該分析上做三點**範圍修正**，並據此排定修復項目。三點都由 WuKong 這側實測
或讀碼確認，與 handover 的結論不同之處已在該文件第 9 節記錄。

### 修正一：生產路徑只有 `run_streaming`，不是 `run`

handover 指認的 `backend.rs:417`（`AgentCliBackend::run`）確實丟棄 `stderr_buf`，但
**它在生產路徑上沒有呼叫端**。`crates/wukong-runtime/src/turn.rs` 的四個呼叫點
（:229、:244、:295、:460）全部走 `run_streaming` / `run_streaming_ephemeral`；
`.run(req)` 在 `wukong-scheduler`、`wukong-telegram`、`wukong-web` 的命中全部位於
`mod tests` 內的假 backend。

修 `run` 是防未來有人接上去，不是修現在會發生的事。優先級因此下調。

### 修正二：兩條 streaming 路徑都拿得到**具型別**的錯誤，不需要 regex 當主要偵測

handover 建議的 regex 樣式（`"statusCode"\s*:\s*410` 等）是 TeleNexus **只有 stderr
純文字可用**時的必要妥協，連帶背上「成交量 429 億美元會誤判」的風險，他們為此另寫
迴歸測試。WuKong 不必繼承這個妥協：

opencode 的 `/doc` 定義 `AssistantMessage.error` 與 `session.error` 事件的 payload 為
八選一的具名錯誤，其中 `APIError` 帶**整數欄位**：

```json
{"name":"APIError","data":{"message":"...","statusCode":410,"isRetryable":false}}
```

比對 `statusCode == 410` 是型別安全的整數比較，**結構上不可能被 prompt 或工具輸出裡
的數字誤觸**。這正是 CLAUDE.md「診斷訊號不可與被診斷的機制同源，要用結構性訊號」
所要求的形狀。regex 只在退無可退的純文字路徑（非串流 `run`）作為第二道。

### 修正三：缺口的正確描述不是「exit 0 被當成功」，是「`session.error` 被 `Ignore`」

這是本次最重要的一點，而且**已實測復現，不需要 EOL 模型**。

對執行中的 opencode server 開 SSE 事件流、送一則訊息、中途 abort，觀察到：

```
{"type":"session.error","properties":{"sessionID":"ses_...",
 "error":{"name":"MessageAbortedError","data":{"message":"Aborted"}}}}
接著 "type":"session.idle" ×2
```

`session.error` **確實送達事件流**。而 `map_server_event`
（`crates/wukong-gateway/src/opencode_server/event_map.rs:239-288`）處理
`session.idle`、`session.status`、`question.asked`、`permission.asked`，其餘一律
`if event_type != "message.part.updated" { return Ignore }` —— `session.error`
落在這個 fall-through 裡被丟掉，隨後的 `session.idle` 讓 `run_streaming` 以 `Ok`
收場。

也就是說：**一個被中止的回合，今天就已經對 WuKong 回報成功。** 上游 410 走的是同一
條路徑，只是 `error.name` 從 `MessageAbortedError` 變成 `APIError`。缺口不是「exit
code 判定太寬」，是「錯誤事件根本沒被讀」。

CLI 串流路徑同構：`--format json` 的 NDJSON 事件經
`parse_event`（`crates/wukong-gateway/src/stream.rs:70-104`），只認
`text` / `reasoning` / `tool_use` / `step_start` / `step_finish` 五種 type，其餘回
`None` 丟棄。

## 目標

- 讓上游錯誤（模型下架 410、限流 429、其他 `APIError`）在**回合結束前**變成 `Err`，
  不再以 `Ok` + 降級文字收場。
- 讓被中止的回合（`MessageAbortedError`）也不再回報成功。
- 讓排程紀錄的 `success: true` 恢復「上游真的服務了這次請求」的語意。
- 偵測以**具型別的結構化訊號**為主，正常任務內容不可能誤觸。
- Server 與 CLI 兩條 backend 都涵蓋，不留另一條。

## 非目標

- **不做自動切換模型。** 沿用 TeleNexus 的取捨：偵測可信之後才輪得到它。
- 不新增告警或通知管道；schedulerd 的 Telegram 通知目前停用，本次不啟用。
- 不改 `WUKONG_AGENT_TIMEOUT_SECS` 或 deadline 模型。
- 不做模型可用性的主動探針（probe）。
- 不移植 TeleNexus 的 `scripts/probe-models.mjs`。

## 現況查核

以下皆已對照原始碼或實測確認。**證據欄標「實測」者為本次執行取得，標「讀碼」者為
靜態確認**——兩者不混用。

| 事實 | 依據 | 證據 |
|---|---|---|
| 生產路徑只走 `run_streaming`，`run()` 呼叫端全在 `mod tests` | `wukong-runtime/src/turn.rs:229,244,295,460`；`executor.rs:456,564`、`dispatch.rs:2235+`、`web/lib.rs:1485+` 均在 `mod tests` 之後 | 讀碼 |
| `session.error` 會送達 SSE 事件流，payload 為具名錯誤物件 | abort 實驗觀察到 `MessageAbortedError` | **實測** |
| `map_server_event` 對 `session.error` 回傳 `Ignore` | `event_map.rs:286-288` fall-through | 讀碼 |
| `session.error` 之後仍會送 `session.idle`，使回合以 `Ok` 收場 | 同一次 abort 實驗觀察到 `session.idle` ×2 | **實測** |
| `APIError.data.statusCode` 是 integer 欄位 | opencode `/doc` OpenAPI schema | 實測（查端點） |
| `AssistantMessage.error` 為八選一具名錯誤 | 同上；含 `APIError`、`ContextOverflowError`、`ContentFilterError` 等 | 實測（查端點） |
| CLI `parse_event` 只認五種 type，其餘丟棄 | `stream.rs:70-104` | 讀碼 |
| `AgentCliBackend::run` 成功路徑丟棄 `stderr_buf` | `backend.rs:410-421`；`run_fixed` 同構於 `:362-375` | 讀碼 |
| `execute_job` 把 `Ok` 一律記成 `success: true` | `executor.rs:97-107` | 讀碼 |
| 未知模型**不是**靜默的：CLI `exit=1`、Server `HTTP 500` | 兩者實跑 `definitely-not-a-real-model-xyz` | **實測** |
| opencode 1.18.18 有 `--print-logs` / `--log-level` | `opencode run --help` | **實測** |
| 目前模型 `opencode/big-pickle`，三層皆未指定、落 provider 預設 | `/data/settings.json` 不存在、`WUKONG_AGENT_CMD` 無 `--model`、`opencode.json` 無 `model`；`/config/providers` 回 `"default":{"opencode":"big-pickle"}` | **實測** |
| 目前未受影響 | `big-pickle` 實跑 `exit=0`、無 410/429；opencode log 160 筆 ERROR 全為 `Failed to fetch models.dev` | **實測** |
| `/config/providers` 的 `status` 欄位**不可**當可用性訊號 | TeleNexus 以已下架的 `gpt-oss-120b` 實測仍為 `active`，該 provider 101 顆全 active | 實測（TeleNexus 提供） |
| 觸發條件不成立：schedulerd 通知停用 | 日誌持續印「未設定 Telegram token」 | **實測** |

## 工作項目

### W1（P0）Server 串流路徑：處理 `session.error`

`event_map.rs` 新增 `session.error` 分支，與既有 `session.idle` 一樣先比對
`sessionID`，再把 `properties.error` 解析成新的 `ServerEventAction::Failed(UpstreamError)`。
`opencode_server.rs:533` 的 `match` 收到 `Failed` 時中止串流並回 `Err`。

`UpstreamError` 攜帶 `name`（如 `APIError`）與 `status_code: Option<u16>`，兩者原樣
來自事件，不做文字解析。

### W2（P0）分類與判定集中在一處

新增 `crates/wukong-gateway/src/upstream_error.rs`，作為**唯一**的分類真相來源：

- `classify(name: &str, status_code: Option<u16>) -> UpstreamFailure`
- `UpstreamFailure`：`ModelEol`(410) / `RateLimited`(429) / `Aborted` / `Other`
- 判定順序：**先看結構化 `status_code`，取不到才退回名稱與文字**（沿用 TeleNexus
  `classify()` 開頭那條「先看結構化的 HTTP 訊號，它比 exit code 精確」）
- 純文字退路的樣式沿用 handover 的嚴格版（`"status(?:Code)?"\s*:\s*410` 等），
  **不採寬鬆的 `\b410\b`**，且**不認 404**

### W3（P1）CLI 串流路徑：`parse_event` 認得錯誤事件

`stream.rs` 的 `parse_event` 增加 `session.error`（與 opencode NDJSON 實際 type 對齊，
實作前需以 `--format json` 實跑一次確認 type 字串），映射為
`StreamEvent::UpstreamError`；`backend.rs` 的 `run_streaming` 迴圈收到即中止並回 `Err`。

### W4（P2）非串流路徑補防護

`AgentCliBackend::run` 成功路徑改為：先以 W2 的純文字退路檢查 `stderr_buf`，命中則
回 `Err`。`OpencodeServerBackend::run` 檢查回應中最後一則 assistant message 的
`info.error`。此二路徑目前無生產呼叫端，屬防未來。

### W5（P2）`--print-logs --log-level ERROR`

僅在 W4 的純文字退路需要資料時才有意義（串流路徑靠事件，不靠 stderr）。實測健康回合
stderr 僅 32 bytes，成本可忽略。加在 `assemble_argv`，不加在 `WUKONG_AGENT_CMD` 預設
——後者會被使用者的 `.env` 覆蓋掉。

### W6（P1）排程紀錄語意

`execute_job` 不需改判定（`Err` 會自然變成 `success: false`），但失敗訊息要能指出
是上游錯誤而非一般失敗：`GatewayError` 新增 `UpstreamFailed { kind, status_code }`
變體，`executor.rs` 的失敗訊息帶上分類。

## 執行順序與相依

```
W2（分類模組，無相依）
 └→ W1（Server 串流，P0，生產路徑）
 └→ W3（CLI 串流，P1）
      └→ W6（排程訊息，P1）
           └→ W5 → W4（P2，防未來）
```

W1 單獨完成即可關掉生產路徑的缺口。W4/W5 可另案。

## 驗收標準

風險評估為 **High**（見下節），故每項另寫失敗路徑。

- **AC-1｜`session.error` 不再被忽略（結構性斷言）**
  給定一則 `{"type":"session.error","properties":{"sessionID":"ses_1","error":{"name":"APIError","data":{"statusCode":410,"message":"..."}}}}`，
  `map_server_event(&value, "ses_1", ...)` 回傳 `ServerEventAction::Failed`，且其
  `status_code == Some(410)`、`name == "APIError"`。
  *失敗路徑*：`sessionID` 不符時回 `Ignore`（不得誤殺他人 session）。
  *檢查方式*：`cargo test -p wukong-gateway event_map::`

- **AC-2｜錯誤事件後不得以 `Ok` 收場**
  以「先送 `session.error`(APIError/410)、再送 `session.idle`」的假 SSE 串流驅動
  `run_streaming`，回傳為 `Err`，且錯誤可辨識為 `ModelEol`。
  *失敗路徑*：只送 `session.idle`（無錯誤事件）時仍回 `Ok`。
  *檢查方式*：`cargo test -p wukong-gateway opencode_server::`
  *註*：此即今日實測到的事件順序，fixture 依實測形狀撰寫。

- **AC-3｜中止的回合不得回報成功**
  `session.error` 帶 `MessageAbortedError` 時 `run_streaming` 回 `Err`，分類為
  `Aborted`（與 `ModelEol` 可區分）。
  *失敗路徑*：不得把 `Aborted` 誤報成 `ModelEol`，告警原因會說錯。
  *檢查方式*：同 AC-2；fixture 直接使用今日 abort 實測抓到的原文 payload。

- **AC-4｜429 與 410 分類正確且互不混淆**
  `classify("APIError", Some(429))` → `RateLimited`；`Some(410)` → `ModelEol`。
  *失敗路徑*：`classify("APIError", Some(404))` **不得**回 `ModelEol`（非對話類模型
  打錯端點也回 404，混進來會讓告警說錯原因）。
  *檢查方式*：`cargo test -p wukong-gateway upstream_error::`

- **AC-5｜純文字退路不得誤判正常內容（迴歸）**
  一段含「本季成交量 410 億美元，較上季 429 億美元下滑」的正常輸出，經純文字退路
  判定為**無上游錯誤**。
  *失敗路徑*：若改用寬鬆 `\b410\b` 此項必紅——這正是它存在的理由。
  *檢查方式*：`cargo test -p wukong-gateway upstream_error::`

- **AC-6｜排程失敗訊息指得出原因**
  上游 410 導致的排程回合，`ExecutionOutput.success == false`，且 `message` 含可辨識
  的上游錯誤分類字樣。
  *失敗路徑*：一般 backend 失敗不得被標成上游錯誤分類。
  *檢查方式*：`cargo test -p wukong-scheduler executor::`

- **AC-7｜既有行為不回歸**
  `cargo test` 全綠、`cargo clippy --all-targets -- -D warnings` 無警告。
  *失敗路徑*：`execute_job` 的 9 個既有直接呼叫端（見風險節）任一紅燈即不通過。
  *檢查方式*：`cargo test && cargo clippy --all-targets -- -D warnings`

- **AC-8｜真實環境不誤殺**
  修改後的映像跑一次真實回合（`big-pickle` 存活狀態），回合正常完成、回傳非空文字、
  排程紀錄為成功。
  *失敗路徑*：若偵測過寬，此項會以「本來會成功的任務被判失敗」呈現。
  *檢查方式*：`docker compose` 起容器後跑一次 `wukong schedule` 回合並觀察紀錄。

## 風險與回滾

**impact 分析：`execute_job` upstream 風險 `HIGH`** —— 13 個受影響符號、9 個直接呼叫端、
3 條執行流程（`run_scan`、`run`、`run_schedule_op`）、4 個模組。9 個直接呼叫端中 8 個是
`executor.rs` 內的既有測試，這代表任何判定改動都會立刻在該檔測試上顯形，是好事。

最大風險是**偵測過寬導致本來會成功的任務被判失敗**——比靜默失效更糟，因為它會實際
中斷服務。三道防線：AC-4 的 404 排除、AC-5 的正常內容迴歸、AC-8 的真實回合。

回滾：W1/W3 為新增分支與新增事件，`git revert` 即可回到「一律 `Ok`」的舊行為，不涉及
資料格式或持久狀態。

## 實作紀錄與偏離（2026-09-08）

實作時有兩處與上面的規劃不同，都記在這裡：

### 偏離一：W3 不新增 `StreamEvent` 變體

原規劃寫「映射為 `StreamEvent::UpstreamError`」。實際改成在 `run_streaming` 的迴圈
直接以 `parse_upstream_error_line` 判讀後回 `Err`，**沒有動 `StreamEvent`**。

兩個理由：`StreamEvent` 在 gateway 之外有 68 處引用，加變體要動遍所有 renderer；
更重要的是**上游錯誤本來就不是「可渲染的事件」**，它是回合的控制流結果，該以 `Err`
回給呼叫端。這也與 Server 路徑對稱——那邊的 `ServerEventAction::Failed` 同樣不是
`StreamEvent`。

### 偏離二：W5 的旗標只在真的是 opencode 時才加

原規劃是無條件加在 `assemble_argv`。實作時被既有測試抓到：`agent_cli_backend_captures_stdout`
用 `echo` 當假 agent，無條件加旗標會讓它收到 `--print-logs --log-level ERROR hello wukong`。

`--agent-cmd "printf fixer"` / `echo` 的假 agent 測試法是 CLAUDE.md 記載的既有除錯
手段，弄壞它是實打實的回歸。改為以 basename 判斷（`is_opencode`），並補了
`assemble_argv_omits_opencode_flags_for_fake_agents` 這條測試——絕對路徑的
`/usr/local/bin/opencode` 仍會加。

**這是「要驗證一句宣稱，去問產物」的又一個實例**：規劃階段讀碼沒看出來，是測試跑
出來的。

### 另一處收緊：純文字退路不認裸的 `Gone`

handover 的樣式表列了 `\bGone\b`。實作時刻意排除：一句「the opportunity is gone」
就會誤觸，與「成交量 429 億美元」是同一類誤判。要判 410 就去比對 `statusCode`。
已寫進 `classify_text` 的 doc comment 與迴歸測試。

**TeleNexus 已獨立確認並跟進修正**：他們實測四個無害句子（「the opportunity is
gone」「Gone are the days of cheap compute」「BTC 的漲勢 gone，但 ETH 還在」「此檔
股票的動能已經 gone」）在原樣式下全部誤觸；移除裸 `Gone` 後全部安全，而真實下架的
stderr 仍判得出來（410 命中 3 次、`end of life` 3 次，都不依賴 `Gone`）——純改善，
沒有損失偵測能力。

## 驗證結果（2026-09-08）

8 條驗收標準全部通過。

| AC | 結果 | 證據 |
|---|---|---|
| AC-1 | 通過 | `event_map::session_error_becomes_failed_with_structured_status_code`；失敗路徑另有 `session_error_for_another_session_is_ignored`（含無 sessionID 的情況） |
| AC-2 | 通過 | `opencode_server::upstream_error_event_fails_the_turn_even_when_idle_follows`；失敗路徑 `plain_idle_still_succeeds` |
| AC-3 | 通過 | `aborted_turn_does_not_report_success` + `aborted_session_error_is_classified_as_aborted`，fixture 用實測原文 |
| AC-4 | 通過 | `does_not_treat_404_as_model_eol`、`structured_status_code_decides_first` |
| AC-5 | 通過 | `text_fallback_does_not_misjudge_ordinary_output`（含「成交量 410 億美元／429 億美元」與 `4100` 不得判成 `410`） |
| AC-6 | 通過 | `executor::turn_job_reports_upstream_failure_with_its_classification`；失敗路徑 `ordinary_backend_failure_is_not_labelled_as_upstream` |
| AC-7 | 通過 | `cargo test` 全 workspace 綠（38 個 test binary，0 FAILED）；`cargo clippy --all-targets -- -D warnings` exit 0 |
| AC-8 | 通過 | 見下方陰性對照 |

### 真實環境對照組

兩組都對執行中的部署跑（opencode server `172.20.0.4:4096`，模型 `big-pickle`）：

**陰性對照（AC-8，不得誤殺）**

```
$ WUKONG_AGENT_SERVER_URL=… cargo run -p wukong-cli -- --no-stream "Reply with exactly: OK"
🐵 悟空·fixer
OK
EXIT=0
```

**陽性對照（偵測要真的會觸發）** —— 跑一個長回合，中途對它新建的 session 送 abort：

```
error: backend error: 上游模型錯誤（回合被中止）：MessageAbortedError: Aborted
EXIT=1
```

修復前這個情境會回 `exit 0` 加一段部分文字。**這是整個修復在真實部署裡的端到端證明，
而且不需要 EOL 模型**——410 走同一條路徑，差別只在 `error.name` 與 `statusCode`。

分類也正確：報的是「回合被中止」而不是誤報成「模型已下架」（AC-3 的失敗路徑在真實
環境同樣成立）。

### 變更範圍

`gitnexus detect_changes`：risk **medium**，32 個 changed symbol、3 條受影響流程，
全部落在 `wukong-gateway` 與 `wukong-scheduler`，無外溢。測試 session 無殘留（前後
皆 6 個）。

## 待確認事項

1. ~~**（未驗證，影響 W2 分類正確性）** 上游 410 是否確實以
   `APIError{statusCode:410}` 的形狀送達。~~ **已由真實 EOL 實例證實（2026-09-08，
   TeleNexus 提供）。** 原本這是全案唯一的推論：schema 定義了欄位（描述）、abort
   實驗證實了送達與外層形狀（產物），但中間那一步沒有 EOL 模型可復現。

   TeleNexus 對已下架的模型實跑 `--format json`，得到 `EXIT=0`、stdout 815 bytes，
   而且**裡面只有一則 error 事件、沒有任何 text 事件**：

   ```json
   {"type":"error","sessionID":"…","error":{"name":"APIError","data":{
     "message":"Gone: {\"title\":\"Gone\",\"status\":410,\"detail\":\"…has reached its end of life…\"}",
     "statusCode":410,"isRetryable":false,"responseHeaders":{…}}}}
   ```

   三點都對上了：CLI 用扁平的 `{"type":"error", sessionID, error}`（不是 SSE 的
   `session.error`/`properties`）、`APIError` 帶 integer `data.statusCode`、
   `EXIT=0`。已寫成迴歸測試
   `stream::real_end_of_life_payload_is_classified_as_model_eol`，直接用該原文。

   附帶一提，那則 `message` 字串裡同時有 `"title":"Gone"` 與 `"status":410`——
   **裸 `Gone` 會誤觸的同時，正確的結構化欄位就在旁邊**，這正是「該比對哪一個」
   的實證。

   順帶解答了 handover 裡「每次都固定 `outputLen=815`」那個指紋：那 815 bytes 不是
   降級文字，就是 error 事件本身。
2. ~~**（未確認，影響 W3）** CLI `--format json` 事件流中錯誤事件的實際 `type`
   字串是否為 `session.error`。~~ **已實測解決（2026-09-08）：不是。** CLI 送的是
   `{"type":"error","sessionID":"…","error":{…}}`，SSE 送的是 `session.error`
   ——**兩條路徑的事件詞彙不同**（CLI 用 snake_case：`step_start`、`text`）。幸好
   `error` 物件的形狀相同，故解析共用 `parse_error_field`。當初沒有從 SSE 直接推定
   是對的。
3. **（待蒐證，跨專案）** `/config/providers` 的 `status` 欄位在**其他 provider**
   下架時會不會變。第 5 節的推翻只證實了 nvidia 那一顆（101 顆全 `active`），無法
   推廣到所有 provider。**下次任一邊遇到真實下架時，順手打一次該端點並互相回報**
   ——TeleNexus 明確提了這個請求。目前結論維持「目錄類資料來源不可信」，不因單一
   provider 的樣本而改變修法。

4. **（工具可靠度，影響閘門判讀）** GitNexus `impact` 的直接呼叫者計數會低報。
   本次實測：`map_server_event` 回報 2 個、grep 實際 22 個（21 測試 + 1 生產）；
   `assemble_argv` 回報 9 個、grep 實際 13 個。**兩次它都正確抓到了生產呼叫端**，
   少算的都是測試呼叫點——所以它可以當方向指引，不能當覆蓋率保證。TeleNexus 那邊
   對 `interpretEvent` 遇到更極端的情況（回報 0，實際 2）。
   本次真正擋住問題的是 Rust 的窮盡比對（編譯器直接擋下沒處理 `ServerEventAction::Failed`
   的 match）與全套測試，兩者都不依賴那張圖。

5. **（已知風險，不阻塞）** TeleNexus 2026-09-08 起也使用 `opencode/big-pickle`，
   兩系統共用同一顆模型與同一個 provider 預設，它下架會同時影響兩邊，沒有交叉驗證
   可用。狀態：已知會，是否分散模型另案決定。
