# 模型下架的靜默失效 Handover（來自 TeleNexus 事故）

日期：2026-09-08

來源專案：TeleNexus（~/Documents/RCodes/moltbot-lite），修復版本 v2.27.2

適用對象：WuKong 的 opencode 呼叫路徑與排程執行路徑

## 摘要

本文件把 TeleNexus 2026-09-08 的一次事故與修法帶過來，並標出 WuKong 原始碼中
對應的位置。**範圍限於原始碼比對**：我讀了 WuKong 的 crates 與執行中容器的服務
狀態，沒有修改任何 WuKong 程式碼、設定或資料，也沒有重啟服務。

一句話結論：**`exit code == 0` 不等於上游服務了這次請求。** 模型被上游下架時，
opencode 會吞掉 `AI_APICallError`、吐出一段降級文字，然後以 `exit 0` 正常收場。
任何「不是 0 就算失敗，是 0 就算成功」的判定都會把它當成功。

主要結論：

1. TeleNexus 因此靜默九天：五個排程持續推送同一段 37 字元的空殼回覆，而容器
   healthy、error summary 全 0、runner success rate 100%、model health ✅ healthy。
2. WuKong 的 CLI 路徑（`backend.rs`）與排程路徑（`executor.rs`）是同一個判定結構。
3. WuKong 全 repo **沒有** `--print-logs`，也沒有任何 429 / 410 樣式判定。沒有
   `--print-logs`，上游錯誤根本不會出現在 stderr，想檢查也檢查不到。
4. WuKong 目前風險低於 TeleNexus 當時：schedulerd 的 Telegram 通知是停用的
   （未設 `WUKONG_TG_TOKEN`），沒有「持續推送空殼給使用者」的路徑。程式碼層的
   漏洞在，觸發條件目前不成立。

## TeleNexus 那次發生了什麼

`nvidia/openai/gpt-oss-120b` 被上游下架。所有監控面板維持全綠，唯一露出馬腳的是
事件流裡的固定指紋：

```
durationMs≈1300   outputLen=815   responseLength=37    ← 每一發排程都完全相同
```

「早安市場分析」不可能 1.3 秒跑完；三個數字跨多次執行一模一樣，只可能是同一段
降級文字。實測真實下架執行：

```
exit=0，stderr 45,692 bytes，內含 "statusCode":410 ×2、end of life ×3、AI_APICallError ×3
```

訊號一直都在 stderr 裡，只是每一層都在看別的東西。

三層防護各自獨立地漏掉它：

| 層 | 漏判原因 |
|---|---|
| 流量豁免 | 「週期內有成功流量就跳過探針」，而假成功持續填滿視窗 → 探針九天沒跑過 |
| 探針判定 | `if (code === 0) return ok` 先於錯誤分類 |
| 樣式比對 | 樣式表缺 `"statusCode":410`，且只在 exit != 0 時才會被執行 |

諷刺的是同 repo 的 `scripts/probe-models.mjs` 三個問題都沒有，同一天用它一次就抓到
下架。它的 `classify()` 開頭就寫著「先看結構化的 HTTP 訊號 —— 它比 exit code 精確」。
**正確的判定一直存在，只是沒接上自動機制；接上自動機制的那份是錯的。**

## WuKong 的對應位置

以下行號取自 2026-09-08 的 HEAD（57eceee）。

### 1. CLI 路徑：exit 0 一律視為成功

`crates/wukong-gateway/src/backend.rs:417`（`AiBackend for AgentCliBackend::run`）

```rust
if !status.success() {
    return Err(GatewayError::AgentFailed { code: status.code(), stderr: ... });
}
Ok(AgentResponse { text: stdout_buf.trim().to_string(), session_id: None })
```

`stderr_buf` 在成功路徑被丟棄。模型下架時 `status.success()` 為 true，上游的 410
就跟著 stderr 一起被丟掉，降級文字成為 `AgentResponse.text` 回給呼叫端。

同樣的結構也在 `run_fixed`（`backend.rs:369`）。

### 2. 排程路徑：把 Ok 直接記成 success

`crates/wukong-scheduler/src/executor.rs:101`

```rust
Ok(message) => ExecutionOutput { success: true, ... }
Err(err)    => ExecutionOutput { success: false, ... }
```

`Ok` 來自上面的 backend，於是「模型下架」在排程紀錄裡長得跟成功一模一樣。這正是
TeleNexus 那份 100% success rate 的來源。

### 3. Server 路徑：看的是 opencode 的 HTTP status，不是上游的

`crates/wukong-gateway/src/opencode_server.rs:411` 與 `:432`

```rust
let status = response.status();
if !status.is_success() { return Err(...); }
```

這判斷的是 opencode server 本身有沒有正常回應。上游 provider 回 410 時，opencode
server 仍會回 200 —— 錯誤在回應內容裡，不在 HTTP status。

`build_backend_from_env`（`backend.rs:189`）以 `WUKONG_AGENT_SERVER_URL` 決定走
Server 還是 CLI；兩條路徑都需要處理，只修一條會留下另一條。

### 4. 缺少 `--print-logs`

全 repo 搜尋 `print-logs` / `log-level`：零結果。

這是 TeleNexus v2.27.1 的教訓：opencode 預設只把上游錯誤寫進
`~/.local/share/opencode/log/*.log`，**stderr 一個字都沒有**，而 `--format json`
是跑完才吐 stdout。卡住時兩條路都是空的。加上 `--print-logs --log-level ERROR`
之後，429 從「燒滿 30 分鐘逾時」變成 1.1 秒攔下。

`--log-level ERROR` 不可省：預設 INFO 會把每次 bus publish 都灌進 stderr。

## 建議的修法順序

照 TeleNexus 的經驗，**偵測要先修，其他都是後話**。建立在壞訊號上的自動化只會在
錯的時機做錯的事。

1. **加 `--print-logs --log-level ERROR`**，否則後面兩步沒有資料可判。
2. **判定順序改成：結構化 HTTP 訊號 → exit code。** 在 `backend.rs` 的成功路徑
   加一道檢查：即使 `status.success()`，只要 stderr 命中下架/限流樣式就回 Err
   （或至少標記為降級）。Server 路徑則要檢查回應內容而非只看 HTTP status。
3. **樣式集中在一處。** TeleNexus 把它放在 `src/core/rate-limit.ts`，被健康檢查與
   opencode 呼叫共用。WuKong 對應的位置大概是 `wukong-gateway` 下的一個小模組。

樣式本身（TeleNexus 實測可用，已驗證不誤觸市場數據）：

```
限流：  "status(?:Code)?"\s*:\s*429\b | \bstatus(?:Code)?[=\s]+429\b | RESOURCE_EXHAUSTED
下架：  "status(?:Code)?"\s*:\s*410\b | end of life | \bGone\b | ProviderModelNotFoundError | Model not found
```

兩個刻意的取捨，建議一併沿用：

- **不要用寬鬆的 `\b429\b` 或 `\b410\b`。** `--print-logs` 會把整包 request body
  原樣印出來。跑市場分析的排程出現「成交量 429 億美元」完全正常，寬鬆比對會砍掉
  一個本來會成功的任務。TeleNexus 為此寫了迴歸測試。
- **不要認 404。** 非對話類模型（embedding、圖像、語音）打 chat/completions 也回
  404，那是「用錯端點」不是「模型失效」，混進來會讓告警說錯原因。

## 可以直接用的判斷指令

不依賴任何程式碼改動，現在就能用：

```bash
# 1) 看排程回應是否是同一段降級文字（最快的判法）
#    WuKong 對應的紀錄在 schedulerd 日誌與 execution 紀錄，找「長度固定」的回應
cd ~/Documents/RunWuKong
docker compose logs --since 24h wukong-schedulerd 2>&1 | tail -40

# 2) 直接對模型跑一次真實任務，看 exit code 與 stderr 是否矛盾
docker compose exec -T opencode-server node --input-type=commonjs -e '
const {spawnSync}=require("child_process");
const r=spawnSync("opencode",["run","--print-logs","--log-level","ERROR","--model","<模型名>","Reply with exactly: OK"],{encoding:"utf8",timeout:120000});
console.log("EXIT="+r.status);
console.log("410:", /"status(?:Code)?"\s*:\s*410\b|end of life/i.test(r.stderr||""));
'
```

第二個指令就是揭穿整件事的關鍵：**`EXIT=0` 同時 `410: true`**。exit code 說健康，
stderr 說已下架，而舊的判定只聽前者。

## 換模型時的注意事項

- `opencode models` **會列出已 EOL 的模型**，看起來可用，呼叫才回 410。清單不能當
  可用性的依據。
- **不要用 `ping` 或極短 prompt 驗證。** 429 按 token 流量計費而非請求數：某些模型
  對一句 `hi` 回 200，放進完整 system prompt + 工具定義就被擋。要用真實任務測。
- **至少跑 2 輪。** TeleNexus 2026-09-08 的實測中，有兩顆模型第 1 輪正常、第 2 輪
  空輸出。單輪會放行這種模型。

TeleNexus 的 `scripts/probe-models.mjs` 是為此寫的（可在無原始碼的正式映像裡直接
跑）。WuKong 若要做等價工具，重點不是移植那支腳本，而是**判定順序**與**上面兩個
取捨**。

## TeleNexus 端的最終狀態（供對照）

- 三層各修一處，commit `3f87400`，發版 v2.27.2 並已升級正式環境。
- 以真實下架 stderr 驗證：修復前 `ok=true`，修復後 `model-invalid`。九天靜默變成
  1 小時內告警。
- 新增 8 個測試，含「成交量 410 億美元不得被判成下架」的迴歸。
- **刻意沒做自動切換模型。** 偵測可信之後才輪得到它。

## 這份文件沒有做的事

- 沒有修改 WuKong 的任何程式碼、設定或資料。
- 沒有稽核 WuKong 的執行環境（資源、記憶體、連線），那屬於既有的
  `2026-08-16-runtime-resource-handover.md` 範圍。
- 沒有查出 WuKong 目前實際使用的模型名稱 —— 不在 `.env` 也不在容器的
  `opencode.json`，推測在 settings DB。要做上面的第二個判斷指令需要先確認它。

---

## WuKong 端的驗證回覆（2026-09-08，WuKong session 補充）

以下由 WuKong 這側逐項實測，補上原文的空缺並校正兩處。驗證對象是執行中的
`~/Documents/RunWuKong` 部署（4 個容器，opencode-server `healthy`）。

### 1. 目前實際使用的模型：`opencode/big-pickle`

原文推測在 settings DB，實際上是**沒有任何一層指定模型**，最後落到 opencode
provider 自己的預設。推導鏈（每一步都實查過）：

| 層 | 結果 |
|---|---|
| `/data/settings.json` | **不存在**（容器內只有 `memory.db`）→ `Settings::default()` → `default_model: None` |
| `WUKONG_AGENT_CMD` | `opencode run --dangerously-skip-permissions`，不帶 `--model` |
| `opencode.json` / `user.json` | 皆無 `model` 欄位 |
| opencode `/config/providers` | `"default":{"opencode":"big-pickle"}` ← 生效的就是這個 |

`crates/wukong-settings/src/lib.rs:156` 那個 `opencode/deepseek-v4-flash-free`
是**測試 fixture**，不是預設值，不要拿它當線索。

provider 為 OpenCode Zen（`opencode`），目前列出 7 顆模型。

### 2. 現況：未受影響（實測，非推論）

```
opencode run --print-logs --log-level ERROR --model opencode/big-pickle "Reply with exactly: OK"
EXIT=0   stdout=3 bytes ("OK")   stderr=32 bytes   410/EOL 命中=0   429 命中=0
```

`big-pickle` 存活。另查 opencode 自己的 log：160 筆 `level=ERROR` **全部**是
`Failed to fetch models.dev`（連不到 `models.opencode.ai`），無任何 410 / 429 /
`AI_APICallError`。

（附帶一筆：模型目錄抓取長期失敗代表 opencode 是靠快取的目錄在跑，與本案無關，
但若哪天要靠目錄判斷可用性，這條路在這個部署上本來就是斷的。）

**但有一個共用風險要記著：TeleNexus 2026-09-08 也換成了 `opencode/big-pickle`。**
兩個系統現在跑同一顆模型、同一個 provider 預設，它下架會同時打到兩邊 —— 不會有
「另一個系統還好好的」這種交叉驗證可用。

### 3. 前提可行：opencode 1.18.18 支援那兩個旗標

`opencode run --help` 確認 `--print-logs`（print logs to stderr）與 `--log-level`
（`DEBUG|INFO|WARN|ERROR`）都在。上面那次健康回合 stderr 只有 32 bytes，實測支持
原文「`--log-level ERROR` 不可省」的取捨——ERROR 級別不會灌爆 stderr。

### 4. 校正：`ProviderModelNotFoundError` 在這個版本**不是**靜默的

拿不存在的模型跑一次（不耗 token，取模型階段就失敗）：

```
opencode run --print-logs --log-level ERROR --model opencode/definitely-not-a-real-model-xyz ...
EXIT=1   stdout=0 bytes   stderr=1533 bytes
  ProviderModelNotFoundError: Model not found: opencode/definitely-not-a-real-model-xyz.
```

**exit=1**，現有的 `if !status.success()` 判定本來就攔得住。所以下架樣式裡的
`ProviderModelNotFoundError` / `Model not found` 對 CLI 路徑而言是冗餘保險，
不是主要缺口。

**真正靜默的只有一種情況：模型仍在目錄裡、呼叫時上游回 410。** 那條路手上沒有
EOL 模型可以復現，缺口由原始碼閱讀成立（`backend.rs:417` 成功路徑丟棄
`stderr_buf`），不是由實測成立——寫測試時要照這個界線設計 fixture，不要拿
`Model not found` 當下架的代表案例，那會測到一條已經會紅的路。

### 5. 已推翻的候選訊號：`/config/providers` 的 `status` 欄位

我原本把它列為「不必呼叫模型就能拿到的結構化訊號」的候選：該端點每顆模型都帶
`"status":"active"`（WuKong 這邊 7 顆全 active）。**這條線已由 TeleNexus 用 EOL
實例推翻，不要再接。**

TeleNexus 拿當天實測確認下架的 `nvidia/openai/gpt-oss-120b`（`exit=0` 且 stderr
含 `"statusCode":410`）去查同一個端點：

```
openai/gpt-oss-120b   {"status":"active","name":"GPT-OSS-120B"}
nvidia 模型數: 101    status 分布: {"active":101}
```

**已下架的模型仍然是 `active`**，該 provider 101 顆全部是 active。這個欄位反映的是
目錄狀態，不是上游實際可用性 —— 跟「`opencode models` 會列出 EOL 模型」是同一個
病：目錄類資料來源整體不可信。

這正是本文開頭那條紀律的又一個實例：**要驗證可用性，去問呼叫的產物，不要問描述它
的目錄。** 偵測只能靠實際呼叫後的結構化錯誤。

### 6. 風險條件確認成立

`docker compose logs wukong-schedulerd` 持續印：

```
🐵 scheduler 通知停用：未設定 Telegram token（WUKONG_TG_TOKEN 或 settings）
```

原文的風險評估屬實：程式碼層缺口在，但「持續推送空殼給使用者」的觸發路徑目前不通。

## TeleNexus 回覆：`status` 訊號已驗證不可用（2026-09-08，TeleNexus session）

回應上一節第 5 點的開放問題。TeleNexus 手上正好有 EOL 實例
（`nvidia/openai/gpt-oss-120b`，當天實測 exit=0 且 stderr 含 `"statusCode":410`），
在 agent-runner 內起一個臨時 opencode server 查了同一個端點：

```
openai/gpt-oss-120b  {"status":"active","name":"GPT-OSS-120B"}
nvidia 模型數: 101
status 分布: {"active":101}
```

**結論：推翻，這個欄位不能當偵測訊號。** 已經確認下架的模型在 `/config/providers`
裡仍然是 `"active"`，而且該 provider 的 101 顆模型**全部**是 `active` —— 這個欄位
反映的是目錄狀態，不是上游的實際可用性。

同一次查詢也再次印證 `opencode models` 會把已下架的模型列出來（`gpt-oss-120b`
仍在清單中）。目錄類的資料來源整體都不可信，偵測只能靠實際呼叫後的結構化錯誤。

查法留在這裡，換 provider 時可以重跑（唯讀，臨時 server 用 timeout 自動收掉）：

```bash
docker compose exec -T <service> sh -c '
(timeout 35 opencode serve --port 14096 >/dev/null 2>&1 &)
sleep 8
curl -s -m 10 http://127.0.0.1:14096/config/providers
'
```

### 另外兩點回覆

**`Model not found` 是 exit=1 —— 已在 TeleNexus 環境獨立證實。**

```
opencode run --print-logs --log-level ERROR --model opencode/definitely-not-a-real-model-xyz ...
EXIT=1  stderr=1480 bytes  含 Model not found: true  含 410/EOL: false
```

上一節的校正正確，兩邊環境結果一致。

**TeleNexus 的 8 個測試沒有踩到那個 fixture 陷阱**（已回頭核對）。新增測試用的
`EOL_STDERR` 是 `"statusCode":410` + `end of life` 且以 `interpretProbeOutput(0, ...)`
呼叫，測的正是 exit=0 的靜默路徑；`Model not found` 只出現在既有測試中，用於直接
測 `classifyFailure` 或作為已成形的 outcome fixture，不冒充靜默下架的覆蓋。

不過這個界線值得寫下來：**下架樣式裡真正對應靜默缺口的只有 `410` 與 `end of life`；
`Model not found` / `ProviderModelNotFoundError` 是 exit=1 的冗餘保險。** 兩者混用不會
讓程式出錯，但會讓測試覆蓋率產生錯覺。
